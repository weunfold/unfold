// @rth/dsh-events: the realtime event pane's data side, as a Cordis/dsh plugin. It shows and
// never commands: it attaches to rth-agentd as an observer (agentd refuses its input), keeps a
// bounded window of plain-named rows, and pushes them over SSE. Closing the pane closes the
// stream only; ingestion and the agent go on. Rows come from the live observer feed, not
// from session-log paging: there is no session adapter yet.
//
// The presence rides the same feed: a pure reducer (src/presence.mjs) over the
// envelopes and link states this plugin already receives, plus the host owner's status for the
// microphone indicator, pushed as its own small SSE stream to the sidebar tab and the app tray.
import Schema from '@deepseek-ai/schemastery'
import { DEFAULTS as SSE, fetchStream, sseHub } from '@rth/dsh-sse'
import { DEFAULT_CATALOG, loadCatalog } from './src/catalog.mjs'
import { AgentdObserver } from './src/observer.mjs'
import { HOST_READ_MS, STREAM_HEARTBEAT_MS, initialPresence, nextDeadline, presence, view } from './src/presence.mjs'
import { fold, initialState, projectionDefinition } from './src/projection.mjs'
import { EventWindow } from './src/window.mjs'
import { DEFAULT_APPROVER, DEFAULT_LEDGER, readGrowth } from './src/cp.mjs'

export const name = 'rth-dsh-events'

export const Config = Schema.object({
  catalog: Schema.string().description('path to catalog/events.v3.json; defaults to the repo copy'),
  limit: Schema.natural().default(2000).description('rows kept in the window'),
  agentd: Schema.object({
    enabled: Schema.boolean().default(true),
    host: Schema.string().default('127.0.0.1'),
    port: Schema.natural().default(7700),
  }),
  route: Schema.string().default('/api/rth-events'),
  presence: Schema.object({
    route: Schema.string().default('/api/rth-presence'),
    host: Schema.boolean().default(true).description("read the host owner's status (via rthSession) for the microphone indicator and the request record"),
    heartbeatMs: Schema.natural().default(STREAM_HEARTBEAT_MS).description('presence stream heartbeat, so readers can lease an unchanged stream'),
    cpLedger: Schema.string().default('').description('the append-only cosmic points ledger; defaults to <repo>/cp/ledger/cp-ledger.jsonl; read-only'),
    cpApprover: Schema.string().default(DEFAULT_APPROVER).description('the approved_by id a cosmic points policy approval must name'),
  }),
  transport: Schema.union(['connection', 'webServer']).default('connection')
    .description("connection: DSH's /api channel (Host/Origin fence, browser auth); webServer: a bare route, for hosts without Connection"),
  stream: Schema.object({
    maxPendingBytes: Schema.natural().default(SSE.maxPendingBytes).description('bytes queued behind one slow client before it is dropped'),
    maxPendingEvents: Schema.natural().default(SSE.maxPendingEvents),
    drainTimeoutMs: Schema.natural().default(SSE.drainTimeoutMs).description('longest wait for one client to drain'),
  }),
})

export function apply(ctx, config) {
  config = Config(config ?? {})
  const catalog = loadCatalog(config.catalog || DEFAULT_CATALOG)
  const window = new EventWindow(catalog, { limit: config.limit })
  const hub = sseHub(config.stream)
  let aggregates = initialState()
  let link = { state: 'idle', detail: null }
  const presenceHub = sseHub(config.stream)
  const pres = { state: initialPresence(Date.now()), view: null, text: '', rev: 0, timer: null }

  // Fold one input, push the view only when it changed, and arm one timer for the next time it
  // can change without input (a lease, the wince, sleep). No loop redraws an unchanged state.
  const feed = (input) => {
    const now = Date.now()
    pres.state = presence(pres.state, input, now)
    const v = view(pres.state, now)
    const text = JSON.stringify(v)
    if (text !== pres.text) {
      pres.view = v
      pres.text = text
      pres.rev += 1
      presenceHub.send('presence', v, pres.rev)
    }
    clearTimeout(pres.timer)
    const at = nextDeadline(pres.state, now)
    pres.timer = at === null ? null : setTimeout(() => feed({ type: 'tick' }), Math.max(0, at - now))
    pres.timer?.unref?.()
  }
  feed({ type: 'tick' })

  const service = {
    catalog,
    window,
    streams: hub,
    presenceStreams: presenceHub,
    ingest(env) {
      aggregates = fold(aggregates, env, catalog)
      feed({ type: 'event', d: catalog.describe(env) })
      return window.push(env)
    },
    setLink(state, detail) {
      link = { state, detail: detail ?? null }
      feed({ type: 'link', state, detail })
      window.emit('link', link)
    },
    /** Fold a host owner status (or null: unreadable) into the presence. */
    hostStatus(status) {
      feed({ type: 'host', status })
    },
    presence() {
      return { rev: pres.rev, view: pres.view }
    },
    snapshot() {
      return {
        rev: window.rev,
        rows: window.rows,
        aggregates,
        status: window.status,
        link,
        source: SOURCE,
        dropped: window.dropped,
        unknown: [...window.unknown],
        unknown_omitted: window.unknownOmitted,
      }
    },
  }
  ctx.effect(() => ctx.provide('rthEvents', service), 'rth-events: service')

  // One window listener feeds every stream; unloading ends the streams it opened.
  ctx.effect(() => {
    const off = window.onChange((change, payload, rev) => hub.send(change, payload, rev))
    // Liveness for the presence readers (the tray, the tab): they treat silence past their
    // lease as a stale source. Sent only while a reader is connected.
    const beat = setInterval(() => presenceHub.send('heartbeat', { at: Date.now(), rev: pres.rev }, pres.rev), config.presence.heartbeatMs)
    beat.unref?.()
    return () => {
      clearInterval(beat)
      off()
      hub.close('unloaded')
      presenceHub.close('unloaded')
      clearTimeout(pres.timer)
    }
  }, 'rth-events: streams')

  // The microphone indicator's only source: the owner's /v1/status (`host.mic`), read on a
  // fixed cadence through rth-session with a lease (src/presence.mjs LEASE.hostMs). It is
  // polled until the host pushes capture state; without rth-session it stays unknown.
  if (config.presence.host) {
    ctx.inject(['rthSession'], (sctx) => {
      sctx.effect(() => {
        let stopped = false
        const read = async () => {
          const st = await sctx.rthSession.status().catch(() => null)
          if (!stopped) service.hostStatus(st?.owner ?? null)
        }
        read()
        const timer = setInterval(read, HOST_READ_MS)
        return () => {
          stopped = true
          clearInterval(timer)
        }
      }, 'rth-events: presence host reads')
    })
  }

  if (config.agentd.enabled) {
    ctx.effect(() => {
      const observer = new AgentdObserver({
        host: config.agentd.host,
        port: config.agentd.port,
        onEnvelope: (env) => service.ingest(env),
        onState: (state, detail) => service.setLink(state, detail),
      }).start()
      return () => observer.stop()
    }, 'rth-events: agentd observer')
  }

  // Registered, not driven: nothing appends rth/event to a DSH Session until a session adapter exists.
  ctx.inject(['sessionProjections'], (sctx) => {
    sctx.sessionProjections.register(projectionDefinition({ catalog }))
  })

  // A bare webServer route under /api would shadow Connection's /api prefix and skip its
  // Host/Origin fence and browser auth (WebServer matches exact routes first), so DSH hosts
  // use connection.fetch. The webServer transport stays for hosts without Connection.
  if (config.transport === 'connection') {
    ctx.inject(['connection'], (sctx) => {
      sctx.effect(() => {
        const disposers = [...fetchRoutes(service, config.route, hub), ...presenceFetchRoutes(service, config.presence.route, presenceHub), growthFetchRoute(config.presence.route, config.presence.cpLedger || DEFAULT_LEDGER, config.presence.cpApprover)].map((route) => sctx.connection.fetch.register(route))
        return () => Promise.all(disposers.map((dispose) => dispose()))
      }, 'rth-events: routes')
    })
  } else {
    ctx.inject(['webServer'], (sctx) => {
      sctx.effect(() => {
        const disposers = [...routes(service, config.route, hub), ...presenceRoutes(service, config.presence.route, presenceHub)].map((route) => sctx.webServer.register(route))
        return () => disposers.forEach((dispose) => dispose())
      }, 'rth-events: routes')
    })
  }
}

/** What the live source can and cannot show; the pane states it instead of implying completeness. */
export const SOURCE = {
  kind: 'agentd-observer',
  host_frames: false, // agentd admits host input without broadcasting it to observers
  replay: false, // no durable cursor: a reconnect resumes live, it does not fill the gap
  lag_skips: true, // a lagging attachment is skipped past with a warning
}

/** `GET <route>` returns a snapshot; `GET <route>/stream` is a snapshot then every change, as SSE. */
export function routes(service, base, hub = sseHub()) {
  return [
    {
      kind: 'exact',
      path: base,
      handler: async (_req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify(service.snapshot()))
      },
    },
    {
      kind: 'exact',
      path: `${base}/stream`,
      handler: async (req, res) => {
        const snapshot = service.snapshot()
        hub.open(req, res, 'snapshot', snapshot, snapshot.rev)
      },
    },
  ]
}

/** The same two routes as DSH `connection.fetch` routes: exact paths below /api, owner peers only. */
export function fetchRoutes(service, base, hub = sseHub()) {
  return [
    {
      path: base,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async () => Response.json(service.snapshot()),
    },
    {
      path: `${base}/stream`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const snapshot = service.snapshot()
        return fetchStream(hub, request, 'snapshot', snapshot, snapshot.rev)
      },
    },
  ]
}

/** The presence: `GET <route>` is `{rev, view}`; `GET <route>/stream` is that, then each changed view. */
export function presenceRoutes(service, base, hub = sseHub()) {
  return [
    {
      kind: 'exact',
      path: base,
      handler: async (_req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify(service.presence()))
      },
    },
    {
      kind: 'exact',
      path: `${base}/stream`,
      handler: async (req, res) => {
        const p = service.presence()
        hub.open(req, res, 'presence', p.view, p.rev)
      },
    },
  ]
}

/** The presence routes as DSH `connection.fetch` routes (behind its Host/Origin fence and auth). */
/** `GET <presence route>/growth`: the cosmic points from the CP ledger (read on each request; read-only). */
export function growthFetchRoute(base, ledger, approver = DEFAULT_APPROVER) {
  return { path: `${base}/growth`, methods: ['GET'], requestBody: 'buffered', fetch: async () => Response.json(readGrowth(ledger, { approver })) }
}

export function presenceFetchRoutes(service, base, hub = sseHub()) {
  return [
    { path: base, methods: ['GET'], requestBody: 'buffered', fetch: async () => Response.json(service.presence()) },
    {
      path: `${base}/stream`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const p = service.presence()
        return fetchStream(hub, request, 'presence', p.view, p.rev)
      },
    },
  ]
}
