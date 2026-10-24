// @rth/dsh-sse: server-sent events for the RTH DSH plugins. A stream is a finite baseline
// (a snapshot) followed by increments. res.write returning false is ordinary backpressure,
// not a dead reader: later events queue until 'drain'. A client is dropped only when a bound
// is exceeded: more than `maxPendingBytes` or `maxPendingEvents` queued behind the socket, or
// no 'drain' within `drainTimeoutMs`. A dropped client is disconnected; reconnecting starts
// from a fresh snapshot, which is the resnapshot. Host ingestion and other clients never wait.

export const DEFAULTS = { maxPendingBytes: 1 << 20, maxPendingEvents: 10_000, drainTimeoutMs: 5_000 }

export function sseHub(options = {}) {
  const { maxPendingBytes, maxPendingEvents, drainTimeoutMs } = { ...DEFAULTS, ...options }
  const clients = new Set()
  const drops = {} // reason -> clients dropped for it

  const drop = (c, reason) => {
    if (!clients.delete(c)) return
    drops[reason] = (drops[reason] ?? 0) + 1
    clearTimeout(c.timer)
    c.res.off('drain', c.onDrain)
    // A client still waiting for drain cannot take a goodbye frame; it is cut and resnapshots on reconnect.
    if (c.waiting) c.res.destroy()
    else c.res.end(frame('resnapshot', { reason }))
  }

  const wait = (c) => {
    c.waiting = true
    c.timer = setTimeout(() => drop(c, 'stalled'), drainTimeoutMs)
    c.res.once('drain', c.onDrain)
  }

  const deliver = (c, text) => {
    if (!c.waiting) {
      if (!c.res.write(text)) wait(c)
      return
    }
    c.queue.push(text)
    c.queued += Buffer.byteLength(text)
    if (c.queued > maxPendingBytes || c.queue.length > maxPendingEvents) drop(c, 'stalled')
  }

  return {
    clients,
    drops,
    /** Start one stream with its baseline; closing it removes only this client. */
    open(req, res, event, data, id) {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' })
      const c = { res, queue: [], queued: 0, waiting: false, timer: null }
      c.onDrain = () => {
        clearTimeout(c.timer)
        c.waiting = false
        while (c.queue.length && !c.waiting) {
          const text = c.queue.shift()
          c.queued -= Buffer.byteLength(text)
          if (!res.write(text)) wait(c)
        }
      }
      clients.add(c)
      res.once('close', () => {
        clearTimeout(c.timer)
        clients.delete(c)
      })
      deliver(c, frame(event, data, id))
      return c
    },
    /** One event to every client, serialized once. */
    send(event, data, id) {
      if (!clients.size) return
      const text = frame(event, data, id)
      for (const c of [...clients]) deliver(c, text)
    },
    /** End every stream and cancel every pending drain wait (plugin unload). */
    close(reason = 'unloaded') {
      for (const c of [...clients]) drop(c, reason)
    },
  }
}

/**
 * The same stream as a Fetch `Response`, for DSH's `connection.fetch` routes (the /api channel
 * behind Connection's Host/Origin fence and browser auth). The bridge pulls the body with
 * backpressure and aborts `request.signal` when the browser goes away, so the hub sees an
 * ordinary socket: a full queue is `write() === false`, the next pull is `drain`, the abort
 * or a cancel is `close`. The hub's bounds apply unchanged.
 *
 * Every terminal path settles the body, so a reader (the bridge's `for await`) never waits on
 * a stream nothing will write to again: an abort closes it, a hub drop errors it, an end closes
 * it after the goodbye frame, and a consumer cancel only detaches. Each path removes the client
 * from the hub (through `close`) and the abort listener.
 */
export function fetchStream(hub, request, event, data, id, { highWaterMark = 64 * 1024 } = {}) {
  const listeners = { drain: new Set(), close: new Set() }
  const fire = (name) => {
    for (const fn of [...listeners[name]]) fn()
  }
  let controller
  let done = false
  const onAbort = () => finish((c) => c.close())
  /** The one terminal step: settle the body with `settle`, detach, and tell the hub. */
  const finish = (settle) => {
    if (done) return
    done = true
    res.destroyed = true
    request.signal.removeEventListener('abort', onAbort)
    if (settle) {
      try {
        settle(controller)
      } catch {} // already settled by the consumer
    }
    fire('close')
  }
  const encoder = new TextEncoder()
  const body = new ReadableStream(
    {
      start(c) {
        controller = c
      },
      pull() {
        fire('drain')
      },
      cancel() {
        finish(null) // the consumer went away: nothing left to settle
      },
    },
    { highWaterMark, size: (chunk) => chunk.byteLength },
  )
  const res = {
    destroyed: false,
    writableEnded: false,
    writeHead() {},
    write(text) {
      if (done) return false
      controller.enqueue(encoder.encode(text))
      return controller.desiredSize > 0
    },
    end(text) {
      if (done) return
      res.writableEnded = true
      finish((c) => {
        if (text) c.enqueue(encoder.encode(text))
        c.close()
      })
    },
    destroy() {
      finish((c) => c.error(new Error('sse: client dropped')))
    },
    once(name, fn) {
      const once = () => {
        listeners[name].delete(once)
        fn()
      }
      once.fn = fn
      listeners[name].add(once)
    },
    on(name, fn) {
      listeners[name].add(fn)
    },
    off(name, fn) {
      for (const l of listeners[name]) if (l === fn || l.fn === fn) listeners[name].delete(l)
    },
  }
  if (request.signal.aborted) {
    finish((c) => c.close()) // never registered with the hub
  } else {
    request.signal.addEventListener('abort', onAbort, { once: true })
    hub.open(request, res, event, data, id)
  }
  return new Response(body, { status: 200, headers: SSE_HEADERS })
}

export const SSE_HEADERS = { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', 'x-accel-buffering': 'no' }

export const frame = (event, data, id) => `${id === undefined ? '' : `id: ${id}\n`}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
