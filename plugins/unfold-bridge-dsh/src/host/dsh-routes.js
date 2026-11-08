// Unfold's routes to the work view, served by the bridge behind the same key, Host and no-Origin
// checks as the model routes. They read and write the view only: none of them sends a prompt.
// The one stream is Unfold's connection: a new one replaces the old.
//   POST   /v1/dsh/events               {subscriptions}   the filtered event stream (SSE)
//   POST   /v1/dsh/agents               {subscriptions}   {agents, approvals}
//   POST   /v1/dsh/drafts               a draft for the person to tap
//   DELETE /v1/dsh/drafts/{effect_id}                     withdraw one
//   PUT    /v1/dsh/view                 Unfold's view for the DSH client
import { Refusal, parseSubscriptions } from './watch.js'

const HEARTBEAT_MS = 15_000
const SSE = { 'Content-Type': 'text/event-stream; charset=utf-8', 'Cache-Control': 'no-store' }
const frame = (event, data) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

// `watch` is the work view; `refresh` reads DSH's sessions list into it before a read that
// names Sessions.
export function dshRoutes({ watch, refresh = async () => {}, heartbeatMs = HEARTBEAT_MS }) {
  let stream // {res, conn_id, stop}

  const end = () => {
    if (!stream) return
    const { res, stop } = stream
    stream = undefined
    stop()
    res.end()
  }

  const events = async (body, res) => {
    const subscriptions = body?.subscriptions
    parseSubscriptions(subscriptions) // refused before the old stream is replaced
    await refresh()
    end()
    const { conn_id, backlog } = watch.connect(subscriptions)
    res.writeHead(200, SSE)
    res.write(frame('hello', { conn_id }))
    for (const control of backlog) res.write(frame('control', control))
    const offEvent = watch.on('event', event => res.write(frame('event', event)))
    const offControl = watch.on('control', control => res.write(frame('control', control)))
    const beat = setInterval(() => res.write(': hb\n\n'), heartbeatMs)
    beat.unref?.()
    const current = { res, conn_id }
    current.stop = () => {
      clearInterval(beat)
      offEvent()
      offControl()
      watch.disconnect(conn_id)
    }
    stream = current
    res.on('close', () => {
      if (stream === current) {
        stream = undefined
        current.stop()
      }
    })
  }

  return {
    // Serves a /v1/dsh/ path, or returns false; refusals are thrown as `Refusal`.
    async serve(req, res, path, readJson, sendJson) {
      if (path === '/v1/dsh/events' && req.method === 'POST') return events(await readJson(), res)
      if (path === '/v1/dsh/agents' && req.method === 'POST') {
        const body = await readJson()
        await refresh()
        return sendJson(res, 200, watch.agents(body?.subscriptions))
      }
      if (path === '/v1/dsh/drafts' && req.method === 'POST') {
        const body = await readJson()
        await refresh()
        return sendJson(res, 201, watch.register(body))
      }
      const draft = /^\/v1\/dsh\/drafts\/([A-Za-z0-9_-]{1,128})$/.exec(path)
      if (draft && req.method === 'DELETE') {
        watch.unregister(draft[1])
        return sendJson(res, 200, { ok: true })
      }
      if (path === '/v1/dsh/view' && req.method === 'PUT') {
        watch.setView(await readJson())
        return sendJson(res, 200, { ok: true })
      }
      throw new Refusal(404, 'not_found', `${req.method} ${path} is not served`)
    },
    close: end,
  }
}
