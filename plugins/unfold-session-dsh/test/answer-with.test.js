// What Unfold answers with, read from the app only (src/answer-with.js), over the same channel
// as /submit: GET /answer-with for the facts, POST /answer-with for the person's pick.
import test from 'node:test'
import assert from 'node:assert/strict'
import { apply } from '../index.js'
import { AnswerWith, cleanFacts, menuOf } from '../src/answer-with.js'
import { HostClient } from '../src/host-client.js'
import { answerFacts, fakeControl, preload } from './fake-control.js'
import { fakeDsh } from './fake-dsh.js'
import { within } from './wait.js'

const ROOT = 'unfold-root'
const quiet = { start() {}, stop() {} }
const settle = (ms = 30) => new Promise((resolve) => setTimeout(resolve, ms))

async function endpoint(t) {
  const fake = await fakeControl(`aw-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  return fake
}

test('the client reads the facts and sends a pick with the token; refusals come back as codes', async (t) => {
  const fake = await endpoint(t)
  const host = new HostClient({ channel: () => fake.channel() })
  const read = await host.answerWith()
  assert.equal(read.status, 200)
  assert.deepEqual(read.body, answerFacts())
  assert.deepEqual((await host.chooseAnswer({ choice: 'granite' })).body, { rev: 2, after_reply: false })
  assert.deepEqual((await host.chooseAnswer({ effort: 'high' })).body, { rev: 3, after_reply: false })
  assert.equal((await host.chooseAnswer({ choice: 'nope' })).body.refused, 'not_offered')
  assert.equal((await host.chooseAnswer({ effort: 'medium' })).body.refused, 'not_offered')
  assert.equal((await host.chooseAnswer({ choice: 'openai-codex/gpt-5.5' })).body.refused, 'needs_ack')
  assert.equal((await host.chooseAnswer({ choice: 'openai-codex/gpt-5.5', ack: true })).status, 200)
  // Nothing to pick is refused here, before a connection.
  assert.equal((await host.chooseAnswer({})).body.refused, 'body')
  assert.deepEqual(fake.answerRows.map((row) => [row.method, row.status]), [['GET', 200], ['POST', 200], ['POST', 200], ['POST', 409], ['POST', 409], ['POST', 409], ['POST', 200]])
  assert.equal(fake.rows.length, 0, 'the submit journal has no answer-with row')
  const wrong = new HostClient({ channel: () => fake.channel({ token: 'x'.repeat(64) }) })
  assert.equal((await wrong.answerWith()).status, 401)
  assert.equal((await new HostClient({ channel: () => null }).answerWith()).body.error, 'no_channel')
})

test('the menu lists only the app\'s options, says where each goes, and shows levels only where the app has them', () => {
  const menu = menuOf(cleanFacts(answerFacts()))
  assert.deepEqual(menu.entries.map((e) => [e.id, e.label, e.group]), [
    ['granite', 'Granite 4.1 3B', 'Unfold'],
    ['deepseek', 'DeepSeek V4.1 Flash', 'Unfold'],
    ['openai-codex/gpt-5.5', 'GPT-5.5', 'Unfold Automagik'],
  ])
  assert.deepEqual(menu.entries.map((e) => e.privacy), [
    { kind: 'local', device: 'Mac' },
    { kind: 'direct', provider: 'DeepSeek', memories: true },
    { kind: 'relay', provider: 'ChatGPT', via: 'Unfold Automagik', device: 'Mac' },
  ])
  assert.deepEqual(menu.entries.map((e) => e.levels?.map((l) => l.name) ?? null), [null, ['Auto', 'Off', 'Low', 'High', 'Max'], null])
  assert.equal(menu.entries[1].current, true)
  assert.equal(menu.entries[1].answering, true)
  assert.equal(menu.entries[2].confirm, true)
  assert.equal(menu.level, 'auto')
  assert.equal(menu.status, undefined)
  // Not facts: no menu; an unreadable app: the reason, no entries.
  assert.equal(menuOf(cleanFacts({ options: 'x' })), undefined)
  assert.deepEqual(menuOf(null, { error: 'Unfold is not reachable from here right now' }), { entries: [], error: 'Unfold is not reachable from here right now' })
  // A fallback says who answers; a restart in progress says it is switching.
  const fallback = { ...answerFacts(), chosen: 'openai-codex/gpt-5.5', answering: 'granite', why: 'no_bridge' }
  assert.deepEqual(menuOf(cleanFacts(fallback)).status, { kind: 'fallback', answering: 'Granite 4.1 3B' })
  assert.deepEqual(menuOf(cleanFacts({ ...answerFacts(), chosen: 'granite', answering: null })).status, { kind: 'switching' })
})

test('a pick made during a reply says it switches after the reply, until the agent answers with it', async (t) => {
  const fake = await endpoint(t)
  fake.replying = true
  const aw = new AnswerWith({ host: new HostClient({ channel: () => fake.channel() }) })
  await aw.read()
  assert.deepEqual(await aw.pick({ id: 'granite' }), { afterReply: true })
  assert.deepEqual(aw.menu().status, { kind: 'after-reply' })
  assert.equal(aw.menu().entries.find((e) => e.current).id, 'granite')
  assert.equal(aw.menu().entries.find((e) => e.answering).id, 'deepseek')
  // The reply ended and the agent restarted on the pick.
  fake.facts.answering = 'granite'
  await aw.read()
  assert.equal(aw.menu().status, undefined)
  await assert.rejects(aw.pick({ id: 'openai-codex/gpt-5.5' }), /needs your confirmation/)
  await assert.rejects(aw.pick({ effort: 'medium' }), /does not offer/)
})

test('the plugin reads the pick at start and again when the agent link attaches', async (t) => {
  const fake = await endpoint(t)
  preload(fake.channel())
  t.after(() => preload(null))
  const dsh = fakeDsh({ root: ROOT })
  apply(dsh.ctx, { sessionId: ROOT }, { env: { AUTOMAGIK_HOST: 'unfold' }, observer: quiet })
  t.after(() => dsh.dispose())
  const service = dsh.provided.unfoldSession
  for (let i = 0; i < 100 && service.answerWith.menu() === undefined; i++) await settle(10)
  assert.equal(service.answerWith.menu().entries.length, 3)
  const reads = fake.answerRows.length
  fake.facts.chosen = 'granite'
  fake.facts.answering = 'granite'
  service.bridge.onLink('attached')
  for (let i = 0; i < 100 && fake.answerRows.length === reads; i++) await settle(10)
  await settle()
  assert.equal(service.answerWith.menu().entries.find((e) => e.current).id, 'granite')
  // Reading the pick sends nothing to /submit.
  assert.equal(fake.rows.length, 0)
})

test('every sentence the menu can show from here has its Chinese, and the pt-BR pack reads the same keys', async () => {
  const { ANSWER_WITH_ZH } = await import('../src/client/strings.js')
  const { refusedText } = await import('../src/host-client.js')
  const shown = [
    refusedText('not_offered'), refusedText('needs_ack'),
    'Unfold is not reachable from here right now', "Unfold's model list could not be read right now",
    'The pick may not have reached Unfold; open the menu again to see',
  ]
  for (const text of shown) assert.ok(Object.hasOwn(ANSWER_WITH_ZH, text), text)
  // The menu's read errors are these fixed sentences.
  const down = new AnswerWith({ host: { answerWith: async () => ({ status: 0, body: { error: 'unreachable' } }) } })
  await down.read()
  assert.equal(down.menu().error, 'Unfold is not reachable from here right now')
  const refused = new AnswerWith({ host: { answerWith: async () => ({ status: 401, body: { refused: 'token' } }) } })
  await refused.read()
  assert.equal(refused.menu().error, "Unfold's model list could not be read right now")
})
