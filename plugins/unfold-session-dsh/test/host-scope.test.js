import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { apply } from '../index.js'
import { fakeDsh } from './fake-dsh.js'
import { within } from './wait.js'

const discovery = (dsh) => dsh.routes.find((r) => r.path === '/api/unfold-session/config')

test('ordinary hosts keep their root agents and browser UI when Session is installed', async () => {
  const dsh = fakeDsh()
  let starts = 0
  const before = [...dsh.events]
  apply(dsh.ctx, {}, { env: {}, observer: { start() { starts++ }, stop() {} } })
  assert.deepEqual(Object.keys(dsh.handlers), [], 'no agent hooks can reject another Bot or root')
  assert.deepEqual(Object.keys(dsh.provided), [], 'no takeover service is installed')
  assert.deepEqual(dsh.events, before, 'root session events are untouched')
  assert.equal(starts, 0, 'no observer is started')
  assert.deepEqual(dsh.routes.map((r) => r.path), ['/api/unfold-session/config'])
  const config = await (await discovery(dsh).fetch()).json()
  assert.equal(config.enabled, false)
  const source = readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  const start = source.indexOf('export async function apply(ctx) {')
  const end = source.indexOf('  ctx.effect(', start)
  const scope = { fetch: async () => Response.json(config) }
  vm.createContext(scope)
  const result = await vm.runInContext(`let ROOT_SESSION, STATUS, LIVE; ${source.slice(start, end).replace('export ', '')} throw new Error('disabled client reached UI registration') }; apply({})`, scope)
  assert.equal(result, undefined, 'disabled browser applies no style, slots or navigation')
  dsh.dispose()
})

for (const env of [{ AUTOMAGIK_HOST: ' Unfold ' }, { AUTOMAGIK_PRELOAD: 'unfold' }]) {
  test(`Unfold takeover is enabled by ${Object.keys(env)[0]}`, async () => {
    const dsh = fakeDsh()
    apply(dsh.ctx, {}, { env, observer: { start() {}, stop() {} } })
    const config = await (await discovery(dsh).fetch()).json()
    assert.equal(config.enabled, true)
    assert.ok(dsh.handlers['agent/pre-step']?.length, 'own instance still owns agent turns')
    assert.equal(dsh.provided.unfoldSession.sessionId, 'unfold-root')
    await dsh.provided.unfoldSession.ready
    dsh.dispose()
  })
}

test('own-instance browser bundle registers header buttons through the current Bot module', async () => {
  let mod
  const buttons = []
  const requested = []
  const scope = {
    window: { __ModuleLoader__: { load: ({ factory }) => { mod = factory((id) => {
      requested.push(id)
      if (id === '@automagik/bots-dsh/client') return { omni: { headerButtons: { register: (button) => buttons.push(button) } } }
      if (id === 'react' || id === 'react/jsx-runtime') return {}
      throw new Error(`missing module ${id}`)
    }) } } },
    fetch: async () => Response.json({ enabled: true, sessionId: 'custom-root', route: '/api/custom' }),
    document: { createElement: () => ({ dataset: {}, remove() {} }), head: { append() {} } },
  }
  vm.runInNewContext(readFileSync(new URL('../client.js', import.meta.url), 'utf8'), scope)
  const ctx = {
    effect: (fn) => fn(), inject() {},
    slots: { inject: (_name, run) => run(), register() {} },
    sidebarRightTabs: { register() {} },
    sidebarRight: { openTabs: { getSnapshot: () => [] } },
  }
  await mod.apply(ctx)
  assert.ok(requested.includes('@automagik/bots-dsh/client'))
  assert.equal(buttons.length, 1)
  assert.equal(buttons[0].id, 'unfold-tabs')
  assert.equal(typeof buttons[0].Component, 'function')
  assert.ok(JSON.parse(readFileSync(new URL('../package.json', import.meta.url))).dsh.client.external.includes('@automagik/bots-dsh/client'))
})

test('disabled browser bundle applies no style, slots, header buttons or navigation', async () => {
  let mod
  const scope = {
    window: { __ModuleLoader__: { load: ({ factory }) => { mod = factory(() => ({})) } } },
    fetch: async () => Response.json({ enabled: false, sessionId: 'unfold-root', route: '/api/unfold-session' }),
  }
  vm.runInNewContext(readFileSync(new URL('../client.js', import.meta.url), 'utf8'), scope)
  await mod.apply(new Proxy({}, { get: (_target, key) => { throw new Error(`disabled browser touched ${String(key)}`) } }))
})

test('Bot binding waits until the configured root Session has been created', async () => {
  const dsh = fakeDsh()
  const inject = dsh.ctx.inject
  let finishCreate
  const creation = new Promise((resolve) => { finishCreate = resolve })
  const created = []
  let seenCreate
  const createStarted = new Promise((resolve) => { seenCreate = resolve })
  dsh.ctx.inject = (names, callback) => inject(names, (ctx) => callback({
    ...ctx,
    get: () => ({ stat: async () => false }),
    workspaceRegistry: { create: async () => ({ id: 'workspace' }) },
    sessionController: { ...ctx.sessionController, create: async (options) => { created.push(options); seenCreate(); await creation } },
  }))
  const { mkdtempSync, rmSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const dir = mkdtempSync(join(tmpdir(), 'unfold-root-ready-'))
  try {
    apply(dsh.ctx, { sessionId: 'custom-root', workspaceDir: dir }, { env: { AUTOMAGIK_HOST: 'unfold' }, observer: { start() {}, stop() {} } })
    const service = dsh.provided.unfoldSession
    let ready = false
    service.ready.then(() => { ready = true })
    await within(createStarted, 2000, 'configured root creation starts')
    assert.equal(ready, false, 'service is not ready while root creation waits')
    assert.equal(service.sessionId, 'custom-root')
    assert.equal(created[0].sessionId, service.sessionId)
    finishCreate()
    await service.ready
    assert.equal(ready, true)
  } finally {
    dsh.dispose()
    rmSync(dir, { recursive: true, force: true })
  }
})
