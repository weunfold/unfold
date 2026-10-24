// @rth/dsh-settings: "RTH settings" for DSH, read-only. Shows the effective runtime settings,
// prompt/config revision, backend/model, voice and device state and the model context, each
// with its source and how fresh it is. It opens no connection of its own: it reads three
// services other plugins already provide, and a missing one makes its values unknown.
//
// - rthSession (@rth/dsh-session): `status().owner` is the host owner's GET /v1/status. This
//   plugin never sees the host token; rth-session reads it from its 0600 file.
// - rthEvents (@rth/dsh-events): the observer's event window. Hello, the `gemini ready` log
//   line and context_restore are picked from it; no second observer attaches to rth-agentd.
// - omniTeam (the Bot): only `config.memoryReview` is picked from it.
//
// The host is read when someone asks (GET, a stream opening, an agentd fact arriving), and,
// only while a settings stream is open, renewed once its read is half a lease old. Each open
// stream also gets a heartbeat every `heartbeatMs`: it confirms the event-derived values are
// still current as of its `at`, and it is NOT a host sample (host values carry their own
// `expires_at`). The browser expires everything on its own clock (src/display.mjs), so a
// silent or lost stream can never keep a value looking live. Everything served passes through
// `redact`. There is no write route.
//
// One snapshot identity for GET and SSE: every view names its serving
// instance (`serving`, `serving_started_at`, fixed per load) and a content revision `rev` that
// rises whenever what the view says changes, whichever route produced it. A change is always
// pushed to open streams, so a heartbeat's `rev` is the revision it confirms: the browser
// accepts only views at or after the newest revision it holds, and lets a heartbeat confirm
// only the view with that same revision.
import { randomUUID } from 'node:crypto'
import Schema from '@deepseek-ai/schemastery'
import { DEFAULTS as SSE, fetchStream, sseHub } from '@rth/dsh-sse'
import { redact } from './src/redact.mjs'
import { Facts, HOST_LEASE_MS, STREAM_LEASE_MS, project } from './src/settings.mjs'
import { DEFAULT_CONTEXT_SOURCE, DEFAULT_GEMINI_SOURCE, readContextBounds, readPolicies } from './src/sources.mjs'

export const name = 'rth-dsh-settings'

export const Config = Schema.object({
  route: Schema.string().default('/api/rth-settings'),
  leaseMs: Schema.natural().default(HOST_LEASE_MS).description('a host status read older than this is stale'),
  hostReadTimeoutMs: Schema.natural().default(5_000),
  heartbeatMs: Schema.natural().default(5_000).description('stream heartbeat while a settings stream is open; the browser treats 3 missed beats as a lost stream'),
  sources: Schema.object({
    gemini: Schema.string().description('path to rth-agentd src/gemini.rs (the policy text); defaults to the repo copy'),
    context: Schema.string().description('path to rth-agentd src/context.rs (the fold bounds); defaults to the repo copy'),
  }),
  transport: Schema.union(['connection', 'webServer']).default('connection')
    .description("connection: DSH's /api channel (Host/Origin fence, browser auth); webServer: a bare route, for hosts without Connection"),
  stream: Schema.object({
    maxPendingBytes: Schema.natural().default(SSE.maxPendingBytes),
    maxPendingEvents: Schema.natural().default(SSE.maxPendingEvents),
    drainTimeoutMs: Schema.natural().default(SSE.drainTimeoutMs),
  }),
})

export function apply(ctx, config) {
  config = Config(config ?? {})
  const hub = sseHub(config.stream)
  const facts = new Facts()
  const policies = readPolicies(config.sources.gemini || DEFAULT_GEMINI_SOURCE)
  const bounds = readContextBounds(config.sources.context || DEFAULT_CONTEXT_SOURCE)
  let session = null
  let events = null
  let eventsLink = null
  let bot = null
  let host = null // null until rth-session is loaded
  let reading = null
  let rev = 0
  let lastContent = null
  const serving = { serving: randomUUID(), serving_started_at: Date.now() }
  const streamLeaseMs = config.heartbeatMs ? config.heartbeatMs * 3 : STREAM_LEASE_MS

  const readHost = async () => {
    if (!session) return
    // One read at a time per session: concurrent asks share it. A read started for a session
    // that was replaced or unloaded meanwhile is dropped, so an old owner never revalidates.
    if (reading && reading.session === session) return reading.promise
    const s = session
    const entry = { session: s, promise: null }
    reading = entry
    entry.promise = (async () => {
      let timer
      try {
        const st = await Promise.race([
          s.status(),
          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`no host status in ${config.hostReadTimeoutMs} ms`)), config.hostReadTimeoutMs) }),
        ])
        if (session !== s) return
        const owner = st?.owner
        if (owner && typeof owner === 'object' && !('unavailable' in owner)) host = { body: owner, read_at: Date.now(), ok: true, error: null }
        else host = { ...(host ?? { body: null, read_at: null }), ok: false, error: String(owner?.unavailable ?? 'rth-session returned no owner status') }
      } catch (e) {
        if (session !== s) return
        host = { ...(host ?? { body: null, read_at: null }), ok: false, error: String(e?.message ?? e) }
      } finally {
        clearTimeout(timer)
        if (reading === entry) reading = null
      }
    })()
    return entry.promise
  }

  const service = {
    streams: hub,
    facts,
    /** The view from what was last read; no read happens here. A changed view is pushed. */
    snapshot(now = Date.now()) {
      const view = redact(project({
      host: session || host ? host ?? { body: null, read_at: null, ok: false, error: 'no read yet' } : null,
      facts: events ? facts : null,
      eventsLink,
      bot,
      policies,
      bounds,
      now,
      leaseMs: config.leaseMs,
      streamLeaseMs,
    }))
      const { at, ...content } = view
      const text = JSON.stringify(content)
      const bumped = text !== lastContent
      if (bumped) {
        lastContent = text
        rev++
      }
      const out = { ...serving, rev, ...view }
      if (bumped) hub.send('settings', out, rev)
      return out
    },
    /** Read the host once, then the view. */
    async refresh() {
      await readHost()
      return service.snapshot()
    },
  }

  const changed = () => {
    if (!hub.clients.size) return
    void service.refresh() // pushes when the content changed
  }

  // Liveness, bounded: runs only while a stream is open. A beat renews the host first when its
  // read is half a lease old (a real, bounded read that pushes a new view); otherwise it sends a
  // heartbeat, which renews only event-derived values in the browser.
  let beat = null
  const tick = async () => {
    if (!hub.clients.size) {
      clearInterval(beat)
      beat = null
      return
    }
    const age = host?.read_at == null ? Infinity : Date.now() - host.read_at
    if (session && (!host?.ok || age >= config.leaseMs / 2)) await service.refresh() // pushes the renewed view
    // Confirms the current revision: every change was pushed before this beat.
    service.snapshot()
    hub.send('heartbeat', { at: Date.now(), ...serving, rev }, rev)
  }
  service.rev = () => rev
  service.opened = () => {
    if (beat || !config.heartbeatMs) return
    beat = setInterval(() => void tick(), config.heartbeatMs)
    beat.unref?.()
  }

  ctx.effect(() => ctx.provide('rthSettings', service), 'rth-settings: service')
  ctx.effect(() => () => {
    clearInterval(beat)
    beat = null
    hub.close('unloaded')
  }, 'rth-settings: streams')

  ctx.inject(['rthSession'], (sctx) => {
    sctx.effect(() => {
      session = sctx.rthSession
      host = null
      changed()
      return () => {
        session = null
        host = null
        changed()
      }
    }, 'rth-settings: host status via rth-session')
  })

  ctx.inject(['rthEvents'], (sctx) => {
    sctx.effect(() => {
      events = sctx.rthEvents
      facts.reset()
      for (const row of events.window.rows) facts.ingest(row, null)
      eventsLink = events.snapshot?.().link ?? null
      // Not attached now: whatever the window holds was seen before a gap.
      if (eventsLink?.state !== 'attached') facts.gap()
      const off = events.window.onChange((change, payload) => {
        if (change === 'added' && facts.ingest(payload, Date.now())) changed()
        else if (change === 'link') {
          if (payload?.state !== 'attached') facts.gap()
          eventsLink = payload
          changed()
        }
      })
      return () => {
        off()
        events = null
        eventsLink = null
        facts.reset()
      }
    }, 'rth-settings: agentd facts from rth-events')
  })

  ctx.inject(['omniTeam'], (sctx) => {
    sctx.effect(() => {
      // One boolean, never the Bot's whole config.
      bot = { memoryReview: sctx.omniTeam?.config?.memoryReview }
      return () => {
        bot = null
      }
    }, 'rth-settings: Bot memoryReview')
  })

  if (config.transport === 'connection') {
    ctx.inject(['connection'], (sctx) => {
      sctx.effect(() => {
        const disposers = fetchRoutes(service, config.route, hub).map((route) => sctx.connection.fetch.register(route))
        return () => Promise.all(disposers.map((dispose) => dispose()))
      }, 'rth-settings: routes')
    })
  } else {
    ctx.inject(['webServer'], (sctx) => {
      sctx.effect(() => {
        const disposers = routes(service, config.route, hub).map((route) => sctx.webServer.register(route))
        return () => disposers.forEach((dispose) => dispose())
      }, 'rth-settings: routes')
    })
  }
}

/** `GET <route>` reads the host and returns the view; `GET <route>/stream` is the view, then each change. */
export function routes(service, base, hub = sseHub()) {
  return [
    {
      kind: 'exact',
      path: base,
      handler: async (req, res) => {
        if (req.method !== 'GET') {
          res.writeHead(405, { allow: 'GET' })
          return res.end()
        }
        const view = await service.refresh()
        res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' })
        res.end(JSON.stringify(view))
      },
    },
    {
      kind: 'exact',
      path: `${base}/stream`,
      handler: async (req, res) => {
        hub.open(req, res, 'settings', await service.refresh(), service.rev?.())
        service.opened?.()
      },
    },
  ]
}

/** The same two routes as DSH `connection.fetch` routes: GET only, owner peers only. */
export function fetchRoutes(service, base, hub = sseHub()) {
  return [
    {
      path: base,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async () => Response.json(await service.refresh(), { headers: { 'cache-control': 'no-store' } }),
    },
    {
      path: `${base}/stream`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const res = fetchStream(hub, request, 'settings', await service.refresh(), service.rev?.())
        service.opened?.()
        return res
      },
    },
  ]
}
