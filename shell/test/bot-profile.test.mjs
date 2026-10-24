// The Bot profile and its model attempts.
//
// No model is configured in DSH: rth-root is the voice session in rth-agentd. With its default
// config the vendored Bot's memory review asks for a model 2 s after every idle and fails each
// time ("ds-bot: memory review of rth-root failed: No model is available"). The profile turns the
// review off with the vendor's own switch (config.memoryReview: false, vendor memory.js); no
// vendor file is patched.
//
// This test loads the Bot plugin with the omni-ui config read from the profile, a context that has
// NO model, and the team seed. It counts every model-side call (provider/model listing, model
// resolution, generation) and every "No model" warning, across startup, repeated busy -> idle
// turns of rth-root, and well past the review delay.
//
//   node --test shell/test/            (from the repo root)
//
// The profile (shell/dsh-home-bot/profiles/web/cordis.patch.yml) is read, never written: DSH
// hot-reloads that file. "before" is the same profile with memoryReview removed (the vendor
// default), built in memory.
import assert from 'node:assert/strict'
import { cpSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import yaml from 'js-yaml'
import { apply } from '../vendor/omni-ui-dsh/index.js'
import { fakeContext } from '../vendor/omni-ui-dsh/test/fake-ctx.js'

const SHELL = join(dirname(fileURLToPath(import.meta.url)), '..')
const LIVE = join(SHELL, 'dsh-home-bot/profiles/web/cordis.patch.yml')
const SEED = join(SHELL, 'dsh-home-bot/omni-team-seed/state.json')

// `!!js` values are code DSH evaluates at load; here they only need to parse.
const JS = new yaml.Type('tag:yaml.org,2002:js', { kind: 'scalar', construct: src => ({ js: src }) })
const SCHEMA = yaml.DEFAULT_SCHEMA.extend([JS])

function omniConfig(file) {
  const doc = yaml.load(readFileSync(file, 'utf8'), { schema: SCHEMA })
  const entries = doc.flatMap(item => item.insert ?? [item])
  const omni = entries.find(entry => entry.id === 'omni-ui')
  assert.ok(omni, `${file} has an omni-ui entry`)
  return omni.config
}

// Starts the Bot with the profile's omni-ui config (home and importHome are paths the profile
// computes from env; here a temp home holding a copy of the team seed).
function start(file, t, { dropReviewSwitch = false } = {}) {
  const home = mkdtempSync(join(tmpdir(), 'rth-bot-profile-'))
  cpSync(SEED, join(home, 'state.json'))
  const { home: _h, importHome: _i, ...config } = omniConfig(file)
  if (dropReviewSwitch) delete config.memoryReview
  const ctx = fakeContext()
  const attempts = []
  const warnings = []
  const count = (name, value) => (...args) => {
    attempts.push(name)
    return typeof value === 'function' ? value(...args) : value
  }
  ctx.logger = { warn: (...a) => warnings.push(a.map(String).join(' ')), info() {} }
  ctx.llm = {
    listProviders: count('listProviders', () => []),
    listModels: count('listModels', async () => []),
    resolveModelInfo: count('resolveModelInfo', async () => undefined),
    stream: count('stream', async function* () { throw new Error('no model') }),
  }
  // rth-root's chat grows by one exchange per turn (`turns` appends).
  const chat = []
  ctx.sessionController = {
    resolveAgent: async () => ({ agent: { session: { id: 'rth-root', seq: (chat.at(-1)?.event.seq ?? 0) + 1 } } }),
    selectModel: async () => {},
    page: async ({ beforeSeq, maxMessages }) => {
      const all = chat.filter(r => beforeSeq === undefined || r.event.seq < beforeSeq)
      const records = maxMessages === undefined ? all : all.slice(-maxMessages)
      return { records, hasMore: records.length < all.length }
    },
  }
  ctx.sessions = { messageProjections: [] }
  ctx.workspaceRegistry = { archiveSession: async () => {} }
  let route
  const register = ctx.connection.fetch.register
  ctx.connection.fetch.register = (options) => {
    route = options
    return register(options)
  }
  const warnOrig = console.warn
  console.warn = (...a) => warnings.push(a.map(String).join(' '))
  t.after(async () => {
    console.warn = warnOrig
    await ctx.dispose()
    rmSync(home, { recursive: true, force: true })
  })
  apply(ctx, { home, ...config })
  const call = async (endpoint, payload) => {
    const response = await route.fetch(new Request('http://localhost/api/bot', { method: 'POST', body: JSON.stringify({ endpoint, payload }) }))
    return response.json()
  }
  return { ctx, call, attempts, warnings, config, chat }
}

const settle = () => new Promise(resolve => setImmediate(resolve))

// Busy -> idle turns of rth-root, each followed by more than the review delay.
// The agent carries the DSH session surface the Bot's idle hooks read: the root Session never
// routes to a model, so it has no request header.
async function turns(t, { ctx, chat }, n) {
  for (let i = 0; i < n; i++) {
    const seq = (chat.at(-1)?.event.seq ?? 0) + 1
    chat.push({ event: { seq, type: 'user/message', data: { source: { kind: 'user' }, content: [{ type: 'text', text: `oi, pergunta ${i}` }] } } })
    chat.push({ event: { seq: seq + 1, type: 'assistant/message', data: { message: { content: [{ type: 'text', text: `resposta ${i}` }] } } } })
    const session = { id: 'rth-root', seq: seq + 2, requestHeader: () => undefined, eventAt: () => undefined, deriveMessages: () => [] }
    const agent = { id: 'rth-root', session }
    ctx.emit('agent/status', { agent, status: 'busy' })
    ctx.emit('agent/status', { agent, status: 'idle' })
    t.mock.timers.tick(10_000) // REVIEW_DELAY_MS is 2 s
    for (let k = 0; k < 20; k++) await settle()
  }
}

const noModel = w => /No model is available/.test(w)

test('the profile turns the memory review off', () => {
  assert.equal(omniConfig(LIVE).memoryReview, false)
})

test('before: without the switch the profile asks for a model after every idle and fails', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const bot = start(LIVE, t, { dropReviewSwitch: true })
  const { call, attempts, warnings } = bot
  await call('state', {})
  for (let k = 0; k < 20; k++) await settle()
  const atStart = attempts.length
  await turns(t, bot, 5)
  const failed = warnings.filter(noModel).length
  console.log(`profile without the switch: ${atStart} model-side calls at start, ${attempts.length - atStart} after 5 idle turns; ${failed} "No model" review failures`)
  assert.equal(failed, 5, 'one failed review per idle turn')
  assert.equal(warnings.filter(w => /context check/.test(w)).length, 0, 'the idle context check needs a routed model and makes no attempt')
})

test('after: the profile makes no model attempt after startup, however many idles pass', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const bot = start(LIVE, t)
  const { call, attempts, warnings } = bot
  await call('state', {})
  for (let k = 0; k < 20; k++) await settle()
  const atStart = [...attempts]
  await turns(t, bot, 5)
  t.mock.timers.tick(10 * 60_000) // ten more minutes: no delayed or background retry
  for (let k = 0; k < 50; k++) await settle()
  const after = attempts.slice(atStart.length)
  const state = (await call('state', {})).value // (a UI state read lists providers; counted apart)
  console.log(`profile: start calls [${atStart.join(', ')}]; after 5 idle turns + 10 min: ${after.length} model-side calls [${after.join(', ')}]; memoryReviews ${state.memoryReviews}`)
  assert.equal(after.length, 0, 'no model-side call at all after startup')
  assert.deepEqual(warnings, [], 'no review failure, no context-check failure')
  assert.equal(state.memoryReviews, 0, 'nothing waiting, queued or running')
  assert.ok(!after.includes('stream') && !atStart.includes('stream'), 'no generation was attempted')
  assert.ok(!after.includes('resolveModelInfo'), 'no model was resolved after an idle')
})
