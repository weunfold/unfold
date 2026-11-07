// The bridge's client routes on DSH's own /api channel (src/host/client-routes.js). DSH admits
// each request with this page's own browser session; the client holds no key. Paths are
// document-relative, so the Web UI keeps working under a mounted base path.
export const ROUTE = 'api/unfold-bridge'

// A refused tap carries the bridge's error code; the copy for it is `error.<code>`. Every tap
// has a JSON body, DELETE included: the bridge takes nothing else.
async function post(path, body, { remove = false } = {}) {
  let response
  try {
    response = await fetch(`${ROUTE}/${path}`, {
      method: remove ? 'DELETE' : 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    })
  } catch {
    throw Object.assign(new Error('network'), { code: 'network' })
  }
  if (response.ok) return
  const answer = await response.json().catch(() => null)
  // DSH's own admission answers 401 without the bridge's error body.
  const code = answer?.error?.code ?? (response.status === 401 ? 'unauthorized' : `http_${response.status}`)
  throw Object.assign(new Error(code), { code })
}

const query = (name, value) => `${name}=${encodeURIComponent(value)}`

// A draft's Send carries its token and nothing else: the bridge sends the text it holds for
// that draft, never text from this page.
export const sendDraft = draft => post(`drafts/send?${query('effect_id', draft.effect_id)}`, { token: draft.token })
export const cancelDraft = draft => post(`drafts/cancel?${query('effect_id', draft.effect_id)}`, { token: draft.token })
export const watchSession = sessionId => post('subscriptions', { session_id: sessionId })
export const dropSubscription = id => post(`subscriptions?${query('id', id)}`, {}, { remove: true })
export const undoChange = changeId => post(`undo?${query('change_id', changeId)}`, {})

export const RETRY_MS = { first: 2_000, most: 30_000 }
const CLOSED = 2

// The view stream: a full snapshot on open and on every change. The bridge ends a stream it
// had to drop with `resnapshot`, and EventSource reconnects by itself to a fresh snapshot. It
// gives up for good on a non-200 answer, as before the host half's routes are registered (a
// plugin reload or update), so a closed source is opened again after a growing pause.
// `onSnapshot(null)` marks the stream down until it is back.
export function openView(onSnapshot, { EventSourceImpl = globalThis.EventSource, later = setTimeout, cancel = clearTimeout } = {}) {
  if (typeof EventSourceImpl !== 'function') return () => {}
  let source = null
  let timer = null
  let pause = RETRY_MS.first
  let closed = false
  const open = () => {
    timer = null
    const current = source = new EventSourceImpl(`${ROUTE}/view`)
    current.addEventListener('view', event => {
      pause = RETRY_MS.first
      try { onSnapshot(JSON.parse(event.data)) } catch { /* a torn frame waits for the next one */ }
    })
    current.addEventListener('error', () => {
      onSnapshot(null)
      if (closed || current !== source || current.readyState !== CLOSED || timer !== null) return
      current.close()
      timer = later(open, pause)
      pause = Math.min(pause * 2, RETRY_MS.most)
    })
  }
  open()
  return () => {
    closed = true
    if (timer !== null) cancel(timer)
    source.close()
  }
}
