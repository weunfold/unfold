// The committed client.js, loaded whole as DSH loads it: a classic script that hands a factory to
// the module loader. The factory runs against stand-ins for the platform modules, and the plugin's
// apply() against a recording client context, so a name the bundle uses but never defines fails
// here rather than in someone's DSH.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const PACKAGE = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8')).name

// Any property, call or construction of a platform module gives another stand-in.
function standIn() {
  return new Proxy(function () {}, {
    get: (_, key) => (key === Symbol.toPrimitive ? () => '' : key === Symbol.iterator ? [][Symbol.iterator] : standIn()),
    apply: () => standIn(),
    construct: () => standIn(),
  })
}

function load({ bots = null } = {}) {
  const registrations = []
  const sources = []
  const context = vm.createContext({
    window: { __ModuleLoader__: { load: (registration) => registrations.push(registration) } },
    EventSource: class { constructor(url) { sources.push(url) } addEventListener() {} close() {} },
    URL, console, setTimeout, clearTimeout,
    fetch: async (url) => ({ ok: true, json: async () => url.pathname.endsWith("/unfold-session/config") ? { sessionId: "custom-root" } : { route: "/api/custom-mind" } }),
  })
  vm.runInContext(readFileSync(join(ROOT, 'client.js'), 'utf8'), context, { filename: 'client.js' })
  const requested = []
  const require = (spec) => {
    requested.push(spec)
    if (spec === '@automagik/bots-dsh/client') {
      if (bots === null) throw new Error(`Cannot find module ${spec}`)
      return bots
    }
    return standIn()
  }
  assert.equal(registrations.length, 1)
  const [{ id, factory }] = registrations
  const exports = factory(require)
  context.document = { baseURI: "https://dsh.example/" }
  return { id, exports, requested, sources }
}

function clientContext() {
  const calls = []
  const ctx = {
    effect: (run, label) => { calls.push(['effect', label]); run() },
    locale: { register: (ns, dicts) => calls.push(['locale', ns, dicts]), bind: () => (key) => key },
    sidebarRightTabs: { register: (tab) => calls.push(['tab', tab]) },
    sidebarRight: { openTabs: { getSnapshot: () => [] }, closeIn() {}, openTabIn() {} },
    slots: { inject: (name, run) => { calls.push(['slot', name]); run() }, register: (entry) => calls.push(['register', entry]) },
  }
  return { ctx, calls }
}

test('the bundle registers under the package name and its factory defines every name it uses', () => {
  const { id, exports, requested } = load()
  assert.equal(id, PACKAGE)
  assert.equal(typeof exports.apply, 'function')
  assert.deepEqual([...exports.inject], ['slots', 'uiConversation', 'sidebarRightTabs', 'sidebarRight', 'locale'])
  assert.ok(!requested.includes('@automagik/bots-dsh/client'), 'the Bot team is looked up only when the plugin applies')
})

test('apply registers its own view, tab and dictionary, and opens no stream on its own', async () => {
  const { exports, sources } = load()
  const { ctx, calls } = clientContext()
  await exports.apply(ctx)
  const [, ns, dicts] = calls.find(([kind]) => kind === 'locale')
  assert.equal(ns, 'unfold-trajectory')
  assert.equal(dicts.en['unfold.view'], 'Unfold')
  assert.equal(typeof dicts.zh['unfold.view'], 'string')
  assert.equal(calls.find(([kind]) => kind === 'tab')[1].kind, 'unfold.trajectory')
  const view = calls.find(([kind, entry]) => kind === 'register' && entry.name === 'conversation.view')[1]
  assert.equal(view.id, 'unfold-trajectory')
  assert.equal(view.label(), 'unfold.view')
  const custom = view.inject('custom-root').hooks.unfold
  assert.notEqual(custom.getSnapshot().notUnfold, true, 'configured session reads the trajectory')
  assert.equal(view.inject('unfold-root').hooks.unfold.getSnapshot().notUnfold, true, 'default session does not receive another session feed')
  assert.deepEqual(sources, [], 'the Mind stream opens only when a view subscribes')
  const stop = custom.subscribe(() => {})
  assert.equal(sources[0].pathname, '/api/custom-mind/stream', 'stream uses discovered Mind route')
  stop()
})

test('with the Bot team, the Bot header gets a Trajectory button; without it, nothing fails', async () => {
  const buttons = []
  const bots = { omni: { headerButtons: { register: (button) => { buttons.push(button); return () => {} } } } }
  const withBots = load({ bots })
  await withBots.exports.apply(clientContext().ctx)
  assert.equal(buttons.length, 1)
  assert.equal(buttons[0].id, 'unfold-trajectory')
  assert.equal(typeof buttons[0].Component, 'function')
  const alone = load()
  await assert.doesNotReject(() => alone.exports.apply(clientContext().ctx))
})
