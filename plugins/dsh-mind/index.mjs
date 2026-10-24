// @rth/dsh-mind: the Mind stream, as a Cordis/dsh plugin. It shows and never commands. Every
// event the agent and host emit, as far as its sources carry them, becomes one plain sentence
// (src/sentences.mjs), grouped into episodes through explicit ids only (src/mind.mjs), with an
// audit record per row (ids, cause chain, source, integrity, catalog definition, redacted
// envelope) and gap markers wherever the evidence is incomplete.
//
// Sources, all read-only:
// - rthEvents (@rth/dsh-events): its event window and link state. This is the ONE observer
//   attachment to rth-agentd; Mind adds no connection of its own.
// - journals.host: rth-host's --log JSONL (default off), tailed read-only with byte offsets.
// - journals.agentd: the agentd journal in the rth-state volume (default off), the same way.
// Brain and Jev feed nothing yet; the snapshot lists them as missing sources.
//
// Suggestions are appended to <dataDir>/suggestions.jsonl (src/suggestions.mjs) and go nowhere.
// Routes are on DSH's connection.fetch only (Host/Origin fence, browser auth):
//   GET  <route>                 snapshot (rows, episodes, link, sources)
//   GET  <route>/stream          SSE: snapshot, then row, row-updated, episode, merged, evicted, link, heartbeat
//   GET  <route>/event?id=<id>   the audit record of one row
//   GET  <route>/suggestions     the suggestion log (optionally ?episode=)
//   POST <route>/suggestions     append one suggestion {key, actor, scope:{episode, event, title}, text}
import { join } from 'node:path'
import Schema from '@deepseek-ai/schemastery'
import { DEFAULTS as SSE, fetchStream, sseHub } from '@rth/dsh-sse'
import { DEFAULT_CATALOG, DEFAULT_PROTOCOL, loadDefinitions } from './src/definitions.mjs'
import { JsonlTail, followHostJournal } from './src/journal.mjs'
import { resolveHostJournal } from './src/host-binding.mjs'
import { MindStream } from './src/mind.mjs'
import { redact } from './src/redact.mjs'
import { SuggestionError, SuggestionLog } from './src/suggestions.mjs'

export const name = 'rth-dsh-mind'

export const Config = Schema.object({
  route: Schema.string().default('/api/rth-mind'),
  dataDir: Schema.string().description("the plugin's own data dir (suggestions.jsonl); defaults to <DSH_HOME>/rth-mind"),
  limit: Schema.natural().default(1500).description('rows kept'),
  heartbeatMs: Schema.natural().default(5000).description('Mind stream heartbeat while a reader is connected; readers treat 3 missed beats as a lost stream'),
  catalog: Schema.string().description('catalog/events.v3.json; defaults to the repo copy'),
  protocol: Schema.string().description('crates/rth-protocol/src/lib.rs, for variant doc comments; defaults to the repo copy'),
  journals: Schema.object({
    host: Schema.string().default('').description("rth-host's --log JSONL (e.g. ~/.local/share/rth/app/<label>/rth-host.jsonl); empty: not read (unless hostFromTokenFile)"),
    hostFromTokenFile: Schema.boolean().default(false).description("bind this app instance's host journal from the RTH_HOST_TOKEN_FILE path (<data>/host/<label>/token -> <data>/app/<label>/rth-host.jsonl; the token is never read); used only when journals.host is empty; an unsupported path leaves it off with the reason"),
    agentd: Schema.string().default('').description('the agentd journal (e.g. ~/.microsandbox/volumes/rth-state/journal.jsonl); empty: not read'),
    backfillBytes: Schema.natural().default(256 * 1024).description('history read from the end of each journal at load'),
  }),
  stream: Schema.object({
    maxPendingBytes: Schema.natural().default(SSE.maxPendingBytes),
    maxPendingEvents: Schema.natural().default(SSE.maxPendingEvents),
    drainTimeoutMs: Schema.natural().default(SSE.drainTimeoutMs),
  }),
})

const OBSERVER = 'rth-events observer feed'

/**
 * Mind's data dir (its suggestion log): config.dataDir, else $DSH_HOME/rth-mind. With neither it is
 * refused (no fallback to the working directory, which a Finder launch makes `/`).
 */
export function dataDirFor(config, env) {
  if (config.dataDir) return config.dataDir
  if (env.DSH_HOME) return join(env.DSH_HOME, 'rth-mind')
  throw new Error('rth-mind: DSH_HOME is not set and no dataDir is configured; refusing to write into the working directory')
}

export function apply(ctx, config) {
  config = Config(config ?? {})
  const definitions = loadDefinitions({ catalog: config.catalog || DEFAULT_CATALOG, protocol: config.protocol || DEFAULT_PROTOCOL })
  const mind = new MindStream({ limit: config.limit })
  const dataDir = dataDirFor(config, process.env)
  const suggestions = new SuggestionLog(dataDir)
  const hub = sseHub(config.stream)
  const hostJournal = resolveHostJournal(config.journals)
  const sources = {
    agentd_feed: { state: 'not loaded', detail: 'rth-events is not loaded: no live agent events' },
    host_journal: { state: hostJournal.path ? 'starting' : 'off', detail: hostJournal.detail },
    agentd_journal: { state: config.journals.agentd ? 'starting' : 'off', detail: config.journals.agentd ? config.journals.agentd : 'journals.agentd not configured: no file offsets for agent events, no history before Mind loaded' },
    brain: { state: 'no feed', detail: 'no Brain memory read/write events reach Unfold yet' },
    jev: { state: 'no feed', detail: 'no Jev events reach Unfold yet (Jev-derived rows would be training-ineligible)' },
  }

  // One timer, for the next time the heartbeat lease can run out with no input.
  let timer = null
  const arm = () => {
    clearTimeout(timer)
    const at = mind.nextDeadline()
    timer = at == null ? null : setTimeout(() => (mind.tick(), arm()), Math.max(0, at - Date.now()))
    timer?.unref?.()
  }

  const service = {
    mind,
    definitions,
    suggestions,
    streams: hub,
    sources,
    snapshot: () => ({ ...mind.snapshot(), sources, suggestions: { count: suggestions.records.length, keys: [...suggestions.byKey.keys()], file: 'suggestions.jsonl in the plugin data dir', damaged: suggestions.damaged }, heartbeat_ms: config.heartbeatMs, catalog_version: definitions.catalogVersion }),
    detail: (id) => mind.detail(id, definitions),
    suggest: (body) => suggestions.append(body, Date.now(), mind.scopeContext(body?.scope?.episode)),
  }
  ctx.effect(() => ctx.provide('rthMind', service), 'rth-mind: service')

  ctx.effect(() => {
    const off = mind.onChange((change, payload, rev) => {
      hub.send(change, redact(payload), rev) // the final boundary: every SSE payload is redacted
      if (change === 'link' || change === 'heartbeat') arm()
    })
    const beat = setInterval(() => hub.clients.size && hub.send('heartbeat-mind', { at: Date.now(), rev: mind.rev, link: mind.linkView() }, mind.rev), config.heartbeatMs)
    beat.unref?.()
    return () => {
      off()
      clearInterval(beat)
      clearTimeout(timer)
      hub.close('unloaded')
    }
  }, 'rth-mind: streams')

  // The single observer path: rth-events' window. Rows it already holds were seen before Mind.
  ctx.inject(['rthEvents'], (sctx) => {
    sctx.effect(() => {
      const events = sctx.rthEvents
      sources.agentd_feed = { state: 'bound', detail: 'rth-events window (its one read-only observer attachment to rth-agentd; host frames are not on it, and there is no replay)' }
      const held = events.window.rows ?? []
      if (held.length) {
        for (const row of held) mind.agentd(row, { via: OBSERVER, history: true, preload: true, note: 'retained in the rth-events window before Mind loaded' })
        // Written after them, so it marks the history/live boundary and is not the first row evicted.
        mind.marker('preload', `Mind loaded after rth-events: the ${held.length} row(s) it still held (below) were read at this moment. Their observed time is this load, not when they happened; anything it had already evicted is not shown, and sequence gaps among them are not judged.`)
      }
      const link = events.snapshot?.().link
      if (link) mind.setLink(link.state, link.detail)
      const off = events.window.onChange((change, payload) => {
        if (change === 'added') mind.agentd(payload, { via: OBSERVER })
        else if (change === 'updated') mind.agentdPatch(payload)
        else if (change === 'status' && payload?.wire) mind.agentd(payload, { via: OBSERVER })
        else if (change === 'link') mind.setLink(payload?.state ?? 'unknown', payload?.detail)
      })
      arm()
      return () => {
        off()
        sources.agentd_feed = { state: 'not loaded', detail: 'rth-events was unloaded: no live agent events' }
        mind.setLink('unloaded', 'rth-events was unloaded')
      }
    }, 'rth-mind: agentd events via rth-events')
  })

  for (const [lane, path] of [['host', hostJournal.path], ['agentd', config.journals.agentd]]) {
    if (!path) continue
    ctx.effect(() => {
      const key = `${lane}_journal`
      const onNote = (type, msg) => {
        if (type === 'missing') sources[key] = { state: 'missing', detail: msg }
        mind.marker(`journal_${type}`, `${lane === 'host' ? 'Host' : 'Agent'} journal: ${msg}`)
      }
      // The host lane goes through followHostJournal: a discontinuity reaches Mind before new records.
      const tail = (lane === 'host'
        ? followHostJournal(path, mind, { backfillBytes: config.journals.backfillBytes, onNote })
        : new JsonlTail(path, {
          backfillBytes: config.journals.backfillBytes,
          onRecord: (rec, at) => {
            if (rec && typeof rec === 'object' && rec.kind) mind.agentd(rec, { via: 'agentd journal file', process: rec.source === 'host' ? 'rth-host (a host frame the agent admitted and journaled)' : 'rth-agentd (guest)', ...at })
          },
          onNote,
        })).start()
      if (tail.state === 'following') sources[key] = { state: 'following', detail: `${lane === 'host' ? hostJournal.detail : path} (read-only; ${tail.records} records read at load)` }
      return () => tail.stop()
    }, `rth-mind: ${lane} journal`)
  }

  // connection.fetch only: a bare webServer route under /api would skip DSH's fence and auth.
  ctx.inject(['connection'], (sctx) => {
    sctx.effect(() => {
      const disposers = fetchRoutes(service, config.route, hub).map((route) => sctx.connection.fetch.register(route))
      return () => Promise.all(disposers.map((dispose) => dispose()))
    }, 'rth-mind: routes')
  })
}

const json = (v, status = 200) => Response.json(redact(v), { status })

/** DSH `connection.fetch` routes: exact paths below /api, behind its fence and auth. */
export function fetchRoutes(service, base, hub) {
  return [
    { path: base, methods: ['GET'], requestBody: 'buffered', fetch: async () => json(service.snapshot()) },
    {
      path: `${base}/stream`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const s = service.snapshot()
        return fetchStream(hub, request, 'snapshot', redact(s), s.rev)
      },
    },
    {
      path: `${base}/event`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => {
        const id = new URL(request.url).searchParams.get('id')
        const d = id && service.detail(id)
        return d ? json(d) : json({ error: `no retained row ${id ?? '(no id)'}: evicted, or never observed` }, 404)
      },
    },
    {
      path: `${base}/suggestions`,
      methods: ['GET', 'POST'],
      requestBody: 'buffered',
      fetch: async (request) => {
        if (request.method === 'GET') {
          const episode = new URL(request.url).searchParams.get('episode') || undefined
          // A proposal is listed under the episode it was saved for, or the one that episode merged into.
          const episodes = episode ? service.mind.episodeIds(episode) : undefined
          return json({ suggestions: service.suggestions.list({ episodes }), damaged: service.suggestions.damaged, mind_lifetime: service.mind.life })
        }
        let body
        try {
          body = await request.json()
        } catch {
          return json({ error: 'body must be JSON' }, 400)
        }
        try {
          const out = service.suggest(body) // { record, duplicate, reconciled? }
          return json(out, out.duplicate ? 200 : 201)
        } catch (e) {
          if (e instanceof SuggestionError) return json({ error: e.message }, e.status)
          return json({ error: 'could not store the suggestion' }, 500)
        }
      },
    },
  ]
}
