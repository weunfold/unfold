// The DSH client's routes, on DSH's own /api channel (`connection.fetch`): DSH admits each
// request the way it admits its own API (its browser session, a trusted Host and Origin) before
// the route runs, so a process without DSH's browser session gets 401 here. Bodies are JSON
// only. Routes are exact paths, so ids ride in the query.
//   GET    /api/unfold-bridge/view                          the view stream (SSE)
//   POST   /api/unfold-bridge/drafts/send?effect_id=…  {token}   send that draft's registered text
//          (502 send_failed: DSH refused it; 504 send_unknown: no answer in time. Either way the
//          draft is spent and `cancelled {reason}` follows.)
//   POST   /api/unfold-bridge/drafts/cancel?effect_id=… {token}  drop it
//   POST   /api/unfold-bridge/subscriptions            {session_id}  ask Unfold to watch a Session
//   DELETE /api/unfold-bridge/subscriptions?id=…        {}     ask Unfold to drop a subscription
//   POST   /api/unfold-bridge/undo?change_id=…         {}     ask Unfold to undo its own change
// A send passes the registered text, never text from the request; the other taps only tell
// Unfold, as control events on its stream.
import { randomUUID } from 'node:crypto'
import { fetchStream, sseHub } from './sse.js'
import { Refusal } from './watch.js'

export const CLIENT_ROUTE = '/api/unfold-bridge'
const ID = /^[A-Za-z0-9_-]{1,128}$/
const SEND_TIMEOUT_MS = 30_000

const fail = error => {
  const refusal = error instanceof Refusal ? error : new Refusal(500, 'bridge_error')
  return Response.json({ error: { code: refusal.code } }, { status: refusal.status, headers: { 'cache-control': 'no-store' } })
}
const ok = body => Response.json(body, { headers: { 'cache-control': 'no-store' } })

async function jsonBody(request) {
  const type = (request.headers.get('content-type') ?? '').split(';')[0].trim().toLowerCase()
  if (type !== 'application/json') throw new Refusal(415, 'unsupported_media')
  let body
  try { body = JSON.parse(await request.text()) } catch { throw new Refusal(400, 'body_refused') }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) throw new Refusal(400, 'body_refused')
  return body
}
const only = (body, keys) => {
  if (Object.keys(body).length !== keys.length || !keys.every(key => Object.hasOwn(body, key))) throw new Refusal(400, 'body_refused')
  return body
}
const queryId = (request, name) => {
  const value = new URL(request.url).searchParams.get(name)
  if (value === null || !ID.test(value)) throw new Refusal(404, 'not_found')
  return value
}

// A send raced against its timeout: 'sent', 'failed' (prompt threw), or 'unknown' (no answer in
// time: DSH may still have taken it, so the draft is never sent again).
async function sendBounded(send, timeoutMs) {
  let timer
  const late = new Promise(resolve => { timer = setTimeout(() => resolve('unknown'), timeoutMs) })
  try {
    return await Promise.race([Promise.resolve().then(send).then(() => 'sent', () => 'failed'), late])
  } finally {
    clearTimeout(timer)
  }
}

// `register(route)` is `connection.fetch.register` (it returns the disposer); `prompt(sessionId,
// text, signal)` sends a person's message to a Session (sessionController.prompt).
export function clientRoutes({ watch, register, prompt, log = () => {}, sendTimeoutMs = SEND_TIMEOUT_MS }) {
  const hub = sseHub()
  const offChange = watch.on('change', () => hub.send('view', watch.snapshot()))
  const route = (path, methods, fetch) => register({
    path: `${CLIENT_ROUTE}${path}`,
    methods,
    requestBody: 'buffered',
    fetch: async request => {
      try {
        return await fetch(request)
      } catch (error) {
        if (!(error instanceof Refusal)) log(`${request.method} ${CLIENT_ROUTE}${path} failed: ${error?.message ?? error}`)
        return fail(error)
      }
    },
  })

  const disposers = [
    route('/view', ['GET'], async request => fetchStream(hub, request, 'view', watch.snapshot())),
    route('/drafts/send', ['POST'], async request => {
      const { token } = only(await jsonBody(request), ['token'])
      const effectId = queryId(request, 'effect_id')
      if (!watch.canControl()) throw new Refusal(503, 'agentd_unavailable')
      const { session_id, text } = watch.takeDraft(effectId, token)
      // The draft is taken first, so a second tap can't send it again, even if this one fails. Its
      // outcome is recorded past the control bound: the tap was admitted before the send.
      const outcome = await sendBounded(() => prompt(session_id, text, AbortSignal.timeout(sendTimeoutMs)), sendTimeoutMs)
      if (outcome !== 'sent') {
        log(`send ${outcome === 'unknown' ? 'timed out' : 'failed'}`)
        watch.control({ kind: 'cancelled', effect_id: effectId, reason: outcome === 'unknown' ? 'send_unknown' : 'send_failed' }, { outcome: true })
        throw outcome === 'unknown' ? new Refusal(504, 'send_unknown') : new Refusal(502, 'send_failed')
      }
      watch.control({ kind: 'sent', effect_id: effectId }, { outcome: true })
      return ok({ ok: true })
    }),
    route('/drafts/cancel', ['POST'], async request => {
      const { token } = only(await jsonBody(request), ['token'])
      const effectId = queryId(request, 'effect_id')
      if (!watch.canControl()) throw new Refusal(503, 'agentd_unavailable')
      watch.takeDraft(effectId, token)
      watch.control({ kind: 'cancelled', effect_id: effectId })
      return ok({ ok: true })
    }),
    route('/subscriptions', ['POST', 'DELETE'], async request => {
      const body = await jsonBody(request)
      if (request.method === 'POST') {
        const { session_id } = only(body, ['session_id'])
        if (typeof session_id !== 'string' || !ID.test(session_id)) throw new Refusal(400, 'body_refused')
        watch.control({ kind: 'sub_add', session_id })
      } else {
        only(body, [])
        watch.control({ kind: 'sub_remove', id: queryId(request, 'id') })
      }
      return ok({ ok: true })
    }),
    route('/undo', ['POST'], async request => {
      only(await jsonBody(request), [])
      watch.control({ kind: 'sub_undo', change_id: queryId(request, 'change_id') })
      return ok({ ok: true })
    }),
  ]

  return () => {
    offChange()
    hub.close()
    for (const dispose of disposers.reverse()) dispose?.()
  }
}

// sessionController.prompt as the client's Send uses it: the person's message, queued like a
// message typed in DSH.
export const promptWith = sessionController => (sessionId, text, signal) =>
  sessionController.prompt({ requestId: randomUUID(), sessionId, mode: 'queue', content: [{ type: 'text', text }] }, signal)
