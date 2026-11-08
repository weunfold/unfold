// SPDX-License-Identifier: MIT
//
// @automagik/unfold-capabilities-dsh: "What I can do" for DSH. Answers how much Unfold can do
// now, and how well, using only bench scorecards. It reads the two paths its config names, the
// capability registry and the bench results; with no registry the map is empty and says so. A
// finished run pushes `POST <route>/refresh`; nothing polls. fs.watch is opt-in: on macOS
// (Node 24, Darwin 25) it delivered no events at all, so it cannot be the default trigger.
import { watch } from 'node:fs'
import Schema from '@deepseek-ai/schemastery'
import { computeMap, loadRegistry, loadRuns, unconfiguredMap } from './src/capmap.js'
import { DEFAULTS as SSE, fetchStream, sseHub } from './src/sse.js'

export const name = 'unfold-capabilities'

export const Config = Schema.object({
  registry: Schema.string().description('path to the capability registry (registry.json); without it the map is empty'),
  results: Schema.string().description('path to the bench results directory, one folder per run'),
  watch: Schema.boolean().default(false).description('also recompute on fs.watch events (best effort)'),
  route: Schema.string().default('/api/unfold-capabilities'),
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
  const registryPath = config.registry || null
  const resultsPath = config.results || null
  const listeners = new Set()
  const hub = sseHub(config.stream)
  let current = null

  const service = {
    streams: hub,
    map: () => current,
    refresh() {
      current = registryPath ? computeMap(loadRegistry(registryPath), loadRuns(resultsPath)) : unconfiguredMap()
      for (const listener of listeners) listener(current)
      return current
    },
    onChange(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
  }
  service.refresh()
  ctx.effect(() => ctx.provide('unfoldCapabilities', service), 'unfold-capabilities: service')
  ctx.effect(() => {
    const off = service.onChange((map) => hub.send('map', map))
    return () => {
      off()
      hub.close('unloaded')
    }
  }, 'unfold-capabilities: streams')

  if (config.watch && resultsPath) {
    ctx.effect(() => {
      let timer = null
      // A run writes several files; recompute once it has gone quiet.
      const watcher = watch(resultsPath, { recursive: true }, () => {
        clearTimeout(timer)
        timer = setTimeout(() => service.refresh(), 250)
      })
      return () => {
        clearTimeout(timer)
        watcher.close()
      }
    }, 'unfold-capabilities: watch bench results')
  }

  // A bare /api webServer route would bypass Connection's fence and auth.
  if (config.transport === 'connection') {
    ctx.inject(['connection'], (sctx) => {
      sctx.effect(() => {
        const disposers = fetchRoutes(service, config.route, hub).map((route) => sctx.connection.fetch.register(route))
        return () => Promise.all(disposers.map((dispose) => dispose()))
      }, 'unfold-capabilities: routes')
    })
  } else {
    ctx.inject(['webServer'], (sctx) => {
      sctx.effect(() => {
        const disposers = routes(service, config.route, hub).map((route) => sctx.webServer.register(route))
        return () => disposers.forEach((dispose) => dispose())
      }, 'unfold-capabilities: routes')
    })
  }
}

export function routes(service, base, hub = sseHub()) {
  return [
    {
      kind: 'exact',
      path: base,
      handler: async (_req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify(service.map()))
      },
    },
    {
      kind: 'exact',
      path: `${base}/refresh`,
      handler: async (req, res) => {
        if (req.method !== 'POST') {
          res.writeHead(405, { allow: 'POST' })
          return res.end()
        }
        const map = service.refresh()
        res.writeHead(200, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ latest_run: map.latest_run, runs: map.runs }))
      },
    },
    {
      kind: 'exact',
      path: `${base}/stream`,
      handler: async (req, res) => hub.open(req, res, 'map', service.map()),
    },
    { kind: 'exact', path: '/api/unfold-capabilities/config', handler: async (req, res) => {
      if (req.method !== 'GET') { res.writeHead(405, { allow: 'GET' }); return res.end() }
      res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' })
      res.end(JSON.stringify({ route: base }))
    } },
  ]
}

/** The same three routes as DSH `connection.fetch` routes: exact paths below /api, owner peers only. */
export function fetchRoutes(service, base, hub = sseHub()) {
  return [
    {
      path: base,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async () => Response.json(service.map()),
    },
    {
      path: `${base}/refresh`,
      methods: ['POST'],
      requestBody: 'buffered',
      fetch: async () => {
        const map = service.refresh()
        return Response.json({ latest_run: map.latest_run, runs: map.runs })
      },
    },
    {
      path: `${base}/stream`,
      methods: ['GET'],
      requestBody: 'buffered',
      fetch: async (request) => fetchStream(hub, request, 'map', service.map()),
    },
    { path: '/api/unfold-capabilities/config', methods: ['GET'], requestBody: 'buffered', fetch: async () => Response.json({ route: base }) },
  ]
}
