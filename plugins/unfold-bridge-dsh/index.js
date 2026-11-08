// Your DSH's models for Unfold. This host plugin serves the model routes of the DSH it runs in
// on 127.0.0.1, as the OpenAI chat-completions subset Unfold's agent speaks (src/host/bridge.js),
// behind a key made at each start. Unfold finds the port and the key in this plugin's state
// file (src/host/endpoint-file.js). In Unfold's own DSH, Unfold names that file's folder and the
// port (UNFOLD_BRIDGE_STATE_DIR, UNFOLD_BRIDGE_PORT); without them it is as in any DSH. Sign-ins
// and their tokens stay in DSH: the bridge sends each request through DSH's llm service and
// returns text, usage and stable failure codes.
//
// Once DSH's session services are up, the plugin also keeps a work view for Unfold
// (src/host/watch.js): Unfold reads a subscription-filtered event stream and registers drafts
// over the bridge's key (src/host/dsh-routes.js), and the DSH client shows them and sends a
// draft only on the person's tap, over DSH's own admitted /api channel (src/host/client-routes.js).
// The plugin answers no approval.
import { createAssistantMessage } from '@deepseek-ai/dsh-llm'
import { randomBytes, randomUUID } from 'node:crypto'
import { basename } from 'node:path'
import { startBridge, takePort } from './src/host/bridge.js'
import { missingKeys } from './src/host/ready.js'
import { clientRoutes, promptWith } from './src/host/client-routes.js'
import { dshRoutes } from './src/host/dsh-routes.js'
import { endpointFile, removeEndpoint, takeStateDir, writeEndpoint } from './src/host/endpoint-file.js'
import { createWatch } from './src/host/watch.js'

export const name = 'unfold-bridge'
export const inject = ['llm']
// The work view starts once these are up; the model routes never wait for them.
export const VIEW_INJECT = ['sessionController', 'sessions', 'sessionTitle', 'workspaceRegistry', 'connection']
const LIST_TIMEOUT_MS = 10_000

// What Unfold named, read when the plugin first applies: the variables leave the env then, so a
// restart of the plugin in the same process keeps them here. They live on the process, not the
// module, because hot reload or a live update makes a fresh module instance that finds the env
// empty; without them it would write the shared file on a random port. Any code in the DSH
// process can reach this symbol, as it can DSH's launch snapshot: it is no boundary either.
const named = (globalThis[Symbol.for('unfold-bridge.named')] ??= { stateDir: null, port: 0 })
function takeNamed() {
  const stateDir = takeStateDir()
  if (stateDir !== undefined) named.stateDir = stateDir
  const port = takePort()
  if (port !== undefined) named.port = port
  return named
}

export function apply(ctx) {
  let routes
  const { stateDir, port } = takeNamed()
  ctx.effect(() => {
    const key = randomBytes(32).toString('base64url')
    const file = endpointFile(process.env, undefined, stateDir)
    let stopped = false
    const running = startBridge({
      llm: ctx.llm,
      key,
      port,
      assistant: createAssistantMessage,
      missingKeys: providers => missingKeys(ctx, providers),
      log: line => ctx.logger?.info?.('unfold-bridge: %s', line),
      dsh: () => routes,
    }).then(async bridge => {
      if (stopped) return bridge.close().then(() => undefined)
      try {
        writeEndpoint(file, { port: bridge.port, key, pid: process.pid, started: Date.now() })
      } catch (error) {
        // a listener nobody can find serves no one: close it
        await bridge.close()
        throw error
      }
      ctx.logger?.info?.('unfold-bridge: serving on 127.0.0.1:%d', bridge.port)
      return bridge
    }).catch(error => {
      // A refused start leaves no endpoint file, and DSH goes on without the bridge.
      ctx.logger?.warn?.('unfold-bridge: not serving: %s', error?.message ?? error)
    })
    return async () => {
      stopped = true
      const bridge = await running
      removeEndpoint(file, key)
      await bridge?.close()
    }
  }, 'unfold-bridge: the loopback endpoint')

  ctx.inject?.(VIEW_INJECT, view => {
    const log = line => view.logger?.info?.('unfold-bridge: %s', line)
    // A Session's workspace label: the registered workspace that holds it, else its folder's name.
    const workspace = (sessionId, cwd) => {
      const all = view.workspaceRegistry.list?.() ?? []
      const found = all.find(w => w.sessionIds?.includes?.(sessionId)) ?? (cwd ? all.find(w => w.path === cwd) : undefined)
      return found?.title ?? (cwd ? basename(cwd) : '')
    }
    const watch = createWatch({
      names: { title: (session, sessionId) => { const live = session ?? view.sessions.get(sessionId); return live ? view.sessionTitle.get(live)?.title : undefined }, workspace },
      newId: randomUUID,
    })
    const refresh = async () => {
      try {
        watch.listed((await view.sessionController.list({}, AbortSignal.timeout(LIST_TIMEOUT_MS))).items)
      } catch (error) {
        log(`the sessions list failed: ${error?.name ?? 'error'}`)
      }
    }
    view.on('session/event', (session, event) => watch.sessionEvent(session, event))
    view.on('agent/status', ({ agent, status }) => watch.agentStatus(agent?.id, status, agent?.session))
    view.on('agent/created', ({ agent }) => watch.agentCreated(agent?.id, agent?.session))
    // A waterfall: the plugin only looks, and DSH's own handling goes on.
    view.on('agent/request-error', (payload, next) => {
      watch.requestError(payload?.agent?.id, payload?.failure?.code, payload?.agent?.session)
      return next()
    })
    view.effect(() => clientRoutes({
      watch,
      register: route => view.connection.fetch.register(route),
      prompt: promptWith(view.sessionController),
      log,
    }), 'unfold-bridge: the DSH client routes')
    view.effect(() => {
      const served = dshRoutes({ watch, refresh })
      routes = served
      void refresh()
      return () => {
        if (routes === served) routes = undefined
        served.close()
      }
    }, 'unfold-bridge: the work view')
  })
}
