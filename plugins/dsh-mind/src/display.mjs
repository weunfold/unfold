// What the panel may claim as live, as time passes: a stream silent past its lease, or lost,
// is not live, and an agent heartbeat past its lease is stale, even if no new frame arrives to
// say so. Pure: the clock is an argument. Bundled into the browser client and tested here.
export const STREAM_BEATS = 3

/**
 * @param {{stream:string, lastStreamAt:number|null, heartbeatMs:number, offsetMs:number, link:object|null}} s
 *   offsetMs = server clock minus local clock, from the last snapshot or heartbeat
 * @param {number} now local ms
 */
export function freshness(s, now) {
  const leaseMs = s.heartbeatMs * STREAM_BEATS
  const out = { stream: null, agent: null, nextChange: null }
  const next = []
  if (s.stream !== 'open' || s.lastStreamAt == null) {
    out.stream = { state: 'lost', label: s.stream === 'opening' ? 'Mind stream: connecting' : 'Mind stream: lost, reconnecting (nothing below is live)' }
  } else if (now - s.lastStreamAt > leaseMs) {
    out.stream = { state: 'stale', label: `Mind stream: no word for ${Math.round((now - s.lastStreamAt) / 1000)} s (stale; nothing below is live)` }
  } else {
    out.stream = { state: 'live', label: 'Mind stream: live (pushed)' }
    next.push(s.lastStreamAt + leaseMs + 1)
  }
  const link = s.link
  if (out.stream.state !== 'live') out.agent = { state: 'unknown', label: 'agent: unknown (this panel is not receiving)' }
  else if (!link || link.state !== 'attached') out.agent = { state: 'down', label: `agent feed: ${link?.state ?? 'unknown'}${link?.detail != null ? ` (${link.detail})` : ''}: nothing new is observed` }
  else {
    const last = link.heartbeat?.at ?? link.since
    const serverNow = now + (s.offsetMs ?? 0)
    if (last == null) out.agent = { state: 'unknown', label: 'agent feed: attached, no heartbeat yet' }
    else if (serverNow - last > link.lease_ms) out.agent = { state: 'stale', label: `agent feed: attached but no heartbeat for ${Math.round((serverNow - last) / 1000)} s (stale)` }
    else {
      out.agent = { state: 'live', label: `agent feed: live (attach ${link.attaches}, heartbeat fresh)` }
      next.push(last + link.lease_ms + 1 - (s.offsetMs ?? 0))
    }
  }
  out.nextChange = next.length ? Math.min(...next) : null
  return out
}

/**
 * The audit panel's reads, bound to the selection. Each `select(id)` aborts
 * the previous read and only the latest selection may report; an answer is accepted only when it
 * is for exactly the selected row id (row ids carry the Mind lifetime, so a read crossing a
 * server restart is refused, not shown as the selected event).
 * @param {(id: string, signal: AbortSignal) => Promise<{ok: boolean, body?: any, error?: string}>} fetchDetail
 */
export function auditLoader(fetchDetail) {
  let token = 0
  let ctrl = null
  return {
    select(id, on) {
      const mine = ++token
      ctrl?.abort()
      const c = new AbortController() // this read's own controller: a later select aborts it
      ctrl = c
      on({ state: 'loading', id })
      let read
      try {
        read = Promise.resolve(fetchDetail(id, c.signal))
      } catch (e) {
        read = Promise.reject(e)
      }
      read
        .then((r) => {
          if (mine !== token) return // superseded: a later selection owns the panel
          if (!r?.ok) return on({ state: 'error', id, error: r?.error ?? 'not available' })
          const got = r.body?.row?.id
          if (got !== id) return on({ state: 'error', id, error: `the server answered for ${got ?? 'another row'}, not ${id} (Mind restarted, or the row is gone)` })
          on({ state: 'ready', id, data: r.body })
        }, (e) => {
          if (mine !== token || e?.name === 'AbortError') return
          on({ state: 'error', id, error: e?.message ?? String(e) })
        })
    },
    cancel() {
      token++
      ctrl?.abort()
      ctrl = null
    },
  }
}
