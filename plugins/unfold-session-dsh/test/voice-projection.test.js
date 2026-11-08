// The DSH half of spoken turns: apply() against a small stand-in for the DSH host (one root
// agent whose turns run our agent/pre-step, a Session that records appends, the Session API's
// prompt). What lands in unfold-root, and that no spoken turn ever becomes a command.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { SessionBridge } from '../src/bridge.js'
import * as llm from '@deepseek-ai/dsh-llm'
import { apply } from '../index.js'
import { fakeDsh } from './fake-dsh.js'
const AUTO_VAD = 'provider:gemini-auto-vad@v1'
const SESSION = 'unfold-session-1'

/** Unfold's control endpoint: each submit is a new turn, named by the command id the chat sent. */
function fakeHost() {
  const calls = []
  return {
    calls,
    async submit(session, text) {
      const command_id = `app:b1:${calls.length + 1}`
      calls.push({ op: 'submit', session, text, command_id })
      return { status: 200, body: { accepted: true, turn_id: command_id } }
    },
  }
}

function harness(opts, deps = {}) {
  const host = fakeHost()
  const bridge = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: { stopMs: 200, gapWaitMs: 100, voiceGapWaitMs: 50, voiceIdleMs: 2000 } })
  const dsh = fakeDsh(opts)
  apply(dsh.ctx, {}, { env: { AUTOMAGIK_HOST: 'unfold' }, host, bridge, ...deps })
  bridge.link.session_id = SESSION
  bridge.onLink('attached')
  return { host, bridge, dsh }
}

function guest(producer = 'guest-p1') {
  let seq = 0
  return (kind, fields = {}) => ({ v: 2, id: `${producer}:${++seq}`, producer, seq, source: 'guest', kind, ...fields })
}

function spoken(g, n, reply, { status = 'completed', utterance } = {}) {
  const d = g('decision', { decision_id: `p1-d${n}`, policy: AUTO_VAD, selected: 'respond', ...(utterance ? { utterance_id: utterance } : {}) })
  const rid = `p1-v${n}`
  return [d, g('response_started', { cause: d.id, response_id: rid, request_id: 'p1-mic-1', ...(utterance ? { utterance_id: utterance } : {}) }), ...reply.map((text) => g('agent_delta', { cause: d.id, response_id: rid, text })), g('agent_done', { cause: d.id, response_id: rid, status })]
}

const heard = (g, u, text) => g('input_transcript', { text, utterance_id: u, utterance_seq: 0, final: true, association: 'unknown' })
const submits = (host) => host.calls.filter((c) => c.op === 'submit')

// Clean bubbles changed the expected TEXTS below: the words only, no labels; the
// reply's user bubble carries the words heard before it (adjacency) and none when nothing was heard.
test('unfold-root gets one user bubble (the words) and one reply for a spoken turn; no command is generated', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  bridge.ingest(heard(g, 'p1-u1', 'What is the capital of France?'))
  spoken(g, 1, ['The capital of ', 'France is Paris.']).forEach((e) => bridge.ingest(e))
  await dsh.idle()
  const b = dsh.bubbles()
  assert.deepEqual(b.map((x) => [x.role, x.text]), [
    ['user', 'What is the capital of France?'],
    ['assistant', 'The capital of France is Paris.'],
  ])
  assert.equal(b[1].model, 'agentd voice (p1-v1, decision p1-d1)')
  assert.equal(b[0].rpcId, 'unfold-voice-r-p1-v1', 'the words are the reply\'s own user bubble')
  assert.equal(host.calls.length, 0, 'no command_submitted, no re-ask, no cancel')
  assert.equal(dsh.events.filter((e) => e.type === 'step/start').length, dsh.events.filter((e) => e.type === 'step/end').length)
  dsh.dispose()
})

test('a backend that names the utterance: 1 user voice bubble (the transcript) then 1 reply, in one turn', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  spoken(g, 0, ['Ready.'], { utterance: 'p1-u0' }).forEach((e) => bridge.ingest(e))
  await dsh.idle()
  const before = dsh.bubbles().length
  bridge.ingest(heard(g, 'p1-u1', 'What is two plus two?'))
  spoken(g, 1, ['Four.'], { utterance: 'p1-u1' }).forEach((e) => bridge.ingest(e))
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().slice(before).map((x) => [x.role, x.text]), [['user', 'What is two plus two?'], ['assistant', 'Four.']])
  assert.equal(submits(host).length, 0)
  dsh.dispose()
})

test('interleaved typed and spoken turns: typed unchanged (one command, its own bubbles), spoken never submitted', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  dsh.type('hi')
  await new Promise((r) => setTimeout(r, 5))
  const cmd = submits(host)[0].command_id
  bridge.ingest(g('response_started', { response_id: `resp-${cmd}`, request_id: cmd }))
  bridge.ingest(g('agent_delta', { response_id: `resp-${cmd}`, text: "hi! I'm alive." }))
  // a spoken turn while the typed reply is still open: it queues behind it
  const v = spoken(g, 2, ['Spoken.'])
  v.forEach((e) => bridge.ingest(e))
  bridge.ingest(g('agent_done', { response_id: `resp-${cmd}`, status: 'completed' }))
  await dsh.idle()
  dsh.type('second')
  await new Promise((r) => setTimeout(r, 5))
  const cmd2 = submits(host)[1].command_id
  bridge.ingest(g('response_started', { response_id: `resp-${cmd2}`, request_id: cmd2 }))
  bridge.ingest(g('agent_delta', { response_id: `resp-${cmd2}`, text: 'ok' }))
  bridge.ingest(g('agent_done', { response_id: `resp-${cmd2}`, status: 'completed' }))
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => [x.role, x.text.slice(0, 20)]), [
    ['user', 'hi'],
    ['assistant', "hi! I'm alive."],
    ['assistant', 'Spoken.'], // nothing was heard for it: no user bubble
    ['user', 'second'],
    ['assistant', 'ok'],
  ])
  assert.deepEqual(submits(host).map((c) => c.text), ['hi', 'second'], 'only the typed turns are commands')
  assert.ok(submits(host).every((c) => c.session === SESSION))
  dsh.dispose()
})

test('no duplicates across replay and re-offer: the same reply and note are shown once', async () => {
  const { bridge, dsh } = harness()
  const g = guest()
  const evs = [heard(g, 'p1-u3', 'Once?'), ...spoken(g, 3, ['Once.'])]
  evs.forEach((e) => bridge.ingest(e))
  await dsh.idle()
  evs.forEach((e) => bridge.ingest(e))
  // a fresh adapter (DSH restarted) sees the same events: DSH's durable rpcId dedupe holds
  bridge.emit('voice-opened', { responseId: 'p1-v3', decisionId: 'p1-d3' })
  await dsh.idle()
  assert.equal(dsh.bubbles().length, 2)
  dsh.dispose()
})

test('a queued spoken turn the adapter no longer knows (restart): an honest unknown bubble, never a command', async () => {
  const { host, dsh } = harness()
  dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: '(voice) reply p1-v9' }], source: { kind: 'user', rpcId: 'unfold-voice-r-p1-v9' } }))
  dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: '(heard; association unknown) earlier words' }], source: { kind: 'user', rpcId: 'unfold-voice-h-u-x' } }))
  await dsh.idle()
  const b = dsh.bubbles()
  // The forgotten reply: no user bubble, one short mark; a queued note keeps the text it was
  // queued with (here one written by an earlier version, labels included).
  assert.equal(b.length, 2)
  assert.equal(b[0].text, '*outcome unknown*')
  assert.equal(b[1].text, '(heard; association unknown) earlier words')
  assert.equal(host.calls.length, 0)
  dsh.dispose()
})

test('an interrupted spoken reply is shown cut off; a gapped one possibly incomplete', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  spoken(g, 4, ['1, 2, 3,'], { status: 'cancelled' }).forEach((e) => bridge.ingest(e))
  const gapped = spoken(g, 5, ['One, ', 'two, ', 'three.'])
  gapped.slice(0, 3).forEach((e) => bridge.ingest(e))
  bridge.onLink('waiting')
  bridge.onLink('attached')
  bridge.ingest(gapped[4])
  bridge.ingest(gapped[5])
  await dsh.idle()
  await new Promise((r) => setTimeout(r, 100))
  await dsh.idle()
  const replies = dsh.bubbles().filter((x) => x.role === 'assistant').map((x) => x.text)
  assert.equal(replies[0], '1, 2, 3, *— interrupted*', 'a subtle mark, not a paragraph')
  assert.equal(replies[1], 'One, three. *— may be incomplete*')
  const asst = dsh.events.filter((e) => e.type === 'assistant/message')
  assert.ok(asst.every((e) => e.data.interrupted === true), 'neither is committed as a completed reply')
  assert.equal(submits(host).length, 0)
  dsh.dispose()
})

test('a cold unfold-root: spoken items wait (bounded) and are delivered when the agent appears', async () => {
  const host = fakeHost()
  const bridge = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh({ live: false, sessionApi: false })
  apply(dsh.ctx, {}, { env: { AUTOMAGIK_HOST: 'unfold' }, host, bridge })
  bridge.onLink('attached')
  const g = guest()
  spoken(g, 6, ['Later.']).forEach((e) => bridge.ingest(e))
  await dsh.idle()
  assert.equal(dsh.bubbles().length, 0)
  dsh.ctx.agents.get = (id) => (id === 'unfold-root' ? dsh.agent : undefined)
  for (const fn of dsh.handlers['agent/created']) fn({ agent: dsh.agent })
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => x.role), ['assistant'], 'nothing was heard for it: the reply alone')
  assert.equal(host.calls.length, 0)
  dsh.dispose()
})

test('recorded journal (bench voice run) into unfold-root: 4 utterances, each followed by its reply, zero commands', async () => {
  const { host, bridge, dsh } = harness()
  const evs = readFileSync(new URL('fixtures/bench-voice-4-turns-barge-in.jsonl', import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))
  evs.forEach((e) => bridge.ingest(e))
  await dsh.idle()
  const b = dsh.bubbles()
  assert.deepEqual(b.map((x) => x.role), ['user', 'assistant', 'user', 'assistant', 'user', 'assistant', 'user', 'assistant'])
  assert.equal(b[0].text, 'What is the capital of France?')
  assert.match(b[5].text, / \*— interrupted\*$/)
  assert.match(b[6].text, /^Stop\. What is 2 \+ 2\?/, 'the barge-in words belong to the reply that answered them')
  assert.equal(b[7].text, 'Two plus two equals four.')
  assert.equal(host.calls.length, 0)
  dsh.dispose()
})

test('a stalled Session API never wedges delivery: the live agent takes the item, and later items follow', async () => {
  const { host, bridge, dsh } = harness({ stallPrompt: true }, { promptMs: 40 })
  const g = guest()
  spoken(g, 20, ['First.']).forEach((e) => bridge.ingest(e))
  spoken(g, 21, ['Second.']).forEach((e) => bridge.ingest(e))
  await new Promise((r) => setTimeout(r, 150))
  await dsh.idle()
  assert.equal(dsh.prompts.length, 2, 'both were offered to the Session API, which never answered')
  assert.deepEqual(dsh.bubbles().filter((x) => x.role === 'assistant').map((x) => x.text), ['First.', 'Second.'])
  assert.equal(submits(host).length, 0)
  dsh.dispose()
})

test('through the projection (id contract): a shown partial note gets an update note; a gap is shown', async () => {
  const host = fakeHost()
  const bridge = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { holdMs: 30, retainMs: 60 } } })
  const dsh = fakeDsh()
  apply(dsh.ctx, {}, { env: { AUTOMAGIK_HOST: 'unfold' }, host, bridge })
  bridge.onLink('attached')
  const g = guest()
  bridge.ledger.adjacency = false // the id contract , adjacency off
  const part = (u, seq, text, final) => g('input_transcript', { text, utterance_id: u, utterance_seq: seq, ...(final ? { final: true } : {}), association: 'unknown' })
  // repro 1: a partial note is shown, then the rest and the end flag arrive
  bridge.ingest(part('p1-u30', 0, 'Name one ', false))
  await new Promise((r) => setTimeout(r, 50))
  bridge.ingest(part('p1-u30', 1, 'planet', true))
  // repro 2: the last part (with the end flag) first; the first part never arrives
  bridge.ingest(part('p1-u31', 1, 'is it?', true))
  await new Promise((r) => setTimeout(r, 200))
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => x.text), [
    'Name one',
    'Name one planet',
    '[missing part] is it?',
  ])
  assert.equal(submits(host).length, 0)
  assert.ok(bridge.ledger.utterances.size <= 32 && bridge.ledger.seen.set.size <= 512 && bridge.ledger.forgotten.set.size <= 512)
  dsh.dispose()
})

test('items queued while a turn runs are not left parked (DSH runs one turn per wake on reject)', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  // a burst: 3 notes and 3 replies arrive while earlier turns are still running
  for (let n = 40; n < 43; n++) {
    bridge.ingest(heard(g, `p1-u${n}`, `question ${n}`))
    spoken(g, n, [`answer ${n}`]).forEach((e) => bridge.ingest(e))
  }
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().filter((x) => x.role === 'assistant').map((x) => x.text), ['answer 40', 'answer 41', 'answer 42'])
  assert.deepEqual(dsh.bubbles().filter((x) => x.role === 'user').map((x) => x.text), ['question 40', 'question 41', 'question 42'])
  assert.equal(dsh.agent.inbox.hasPending, false, 'nothing left parked')
  assert.equal(submits(host).length, 0)
  dsh.dispose()
})

// ── liveness: user work queued during an unfold-root turn gets its next wake ──

/** agentd's side of a typed command, as the observer sees it. */
function answerCommands(host, bridge, g, reply = (text) => `re: ${text}`) {
  const submit = host.submit
  host.submit = async (session, text) => {
    const r = await submit(session, text)
    const command_id = r.body.turn_id
    setTimeout(() => {
      const rid = `resp-${command_id}`
      bridge.ingest(g('response_started', { response_id: rid, request_id: command_id }))
      bridge.ingest(g('agent_delta', { response_id: rid, text: reply(text) }))
      bridge.ingest(g('agent_done', { response_id: rid, status: 'completed' }))
    }, 5)
    return r
  }
}

/** A spoken reply that streams slowly: opened now, ended by `end()`. */
function slowSpoken(bridge, g, n) {
  const [d, rs, d1, done] = spoken(g, n, ['Counting slowly: one, '])
  ;[d, rs, d1].forEach((e) => bridge.ingest(e))
  return {
    more: (text) => bridge.ingest(g('agent_delta', { cause: d.id, response_id: `p1-v${n}`, text })),
    end: () => bridge.ingest(done),
  }
}

test('liveness: a typed "hi" queued while a spoken reply streams is submitted once, with its own id, with no further input', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  answerCommands(host, bridge, g)
  const v = slowSpoken(bridge, g, 60)
  await new Promise((r) => setTimeout(r, 20)) // the spoken turn is running in DSH
  assert.equal(dsh.agent.status, 'running')
  const hi = dsh.type('hi')
  v.more('two, ')
  await new Promise((r) => setTimeout(r, 20))
  v.end() // no further input, no wake from the test
  await dsh.idle()
  assert.deepEqual(submits(host).map((c) => c.text), ['hi'], 'exactly once')
  assert.deepEqual(dsh.provided.unfoldSession.turns.filter((t) => t.key?.startsWith('dsh-')).map((t) => [t.key, t.commandId]), [[`dsh-${hi.id}`, 'app:b1:1']], 'its own message, its own command id')
  assert.deepEqual(dsh.bubbles().slice(-2).map((x) => x.text), ['hi', 're: hi'])
  assert.equal(dsh.agent.inbox.hasPending, false)
  dsh.dispose()
})

test('liveness: two typed messages queued during an active turn: neither dropped, neither folded, in order', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  answerCommands(host, bridge, g)
  const v = slowSpoken(bridge, g, 61)
  await new Promise((r) => setTimeout(r, 20))
  const a = dsh.type('first')
  const b = dsh.type('second')
  v.end()
  await dsh.idle()
  assert.deepEqual(submits(host).map((c) => c.text), ['first', 'second'])
  assert.deepEqual(dsh.provided.unfoldSession.turns.filter((t) => t.key?.startsWith('dsh-')).map((t) => t.key), [`dsh-${a.id}`, `dsh-${b.id}`])
  assert.deepEqual(dsh.bubbles().filter((x) => !x.text.startsWith('(voice')).map((x) => x.text).slice(-4), ['first', 're: first', 'second', 're: second'])
  dsh.dispose()
})

test('liveness: messages claimed together (steering + queued) are separate units: no folding, voice never a command', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  answerCommands(host, bridge, g)
  const v = slowSpoken(bridge, g, 62)
  await new Promise((r) => setTimeout(r, 20))
  const s1 = dsh.type('steer one', 'steer')
  const s2 = dsh.type('steer two', 'steer')
  // a spoken reply opens and is queued behind them, and ends at once
  spoken(g, 63, ['Spoken.']).forEach((e) => bridge.ingest(e))
  v.end()
  await dsh.idle()
  assert.deepEqual(submits(host).map((c) => c.text), ['steer one', 'steer two'])
  assert.deepEqual(dsh.provided.unfoldSession.turns.filter((t) => t.key?.startsWith('dsh-')).map((t) => t.key), [`dsh-${s1.id}`, `dsh-${s2.id}`])
  const b = dsh.bubbles().map((x) => x.text)
  assert.ok(b.includes('Spoken.'))
  assert.ok(b.indexOf('re: steer one') < b.indexOf('steer two'), 'each typed unit gets its own reply before the next')
  assert.equal(dsh.agent.inbox.hasPending, false)
  dsh.dispose()
})

test('liveness: replaying the frames after the typed turn completed submits nothing again', async () => {
  const { host, bridge, dsh } = harness()
  const g = guest()
  answerCommands(host, bridge, g)
  const frames = spoken(g, 64, ['Once.'])
  frames.forEach((e) => bridge.ingest(e))
  await new Promise((r) => setTimeout(r, 5))
  dsh.type('hi')
  await dsh.idle()
  frames.forEach((e) => bridge.ingest(e))
  await dsh.idle()
  assert.equal(submits(host).length, 1)
  assert.equal(dsh.bubbles().filter((x) => x.text === 'Once.').length, 1)
  dsh.dispose()
})

// ── clean bubbles ──
// A contract change against id pairing for backends that cannot pair a transcript with its reply
// by id (Gemini: association "unknown", one utterance_id for the whole call). Such transcripts
// are grouped by ADJACENCY: a provider voice_activity_start begins a new utterance, the next
// reply that opens closes it and takes it as its user bubble, written when the reply STARTS. No
// labels in chat; the association qualifiers stay in Trajectory and the Mind audit. Id pairing
// is unchanged for a backend that can prove it.
const LABELS = /association|heard|update|spoken reply|\(voice|provider opened|not identified|\[cut off|\[possibly/
const recordedCall = () => readFileSync(new URL('fixtures/recorded-call-shape.jsonl', import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))

test('a recorded call\'s shape (20 utterances, one utterance_id, no final flag): 20 clean user bubbles, each followed by its reply', async () => {
  const { host, bridge, dsh } = harness()
  // Paced as it happened: each reply's DSH turn runs before the next reply opens (in the call,
  // replies were seconds apart); the order of every envelope is the call's own.
  for (const x of recordedCall()) {
    if (x.lane !== 'agentd') continue
    bridge.ingest(x.env)
    if (x.env.kind === 'agent_done') await dsh.idle()
  }
  await dsh.idle()
  await new Promise((r) => setTimeout(r, 50))
  await dsh.idle()
  const b = dsh.bubbles()
  const want = []
  for (let n = 1; n <= 20; n++) want.push(['user', `Synthetic utterance ${n}.`], ['assistant', `Synthetic reply ${n}, second part.`])
  assert.deepEqual(b.map((x) => [x.role, x.text.replace(/\s*\*?—?\s*interrupted\*?$/, '')]), want)
  assert.deepEqual(b.filter((x) => LABELS.test(x.text)).map((x) => x.text), [], 'no labels in chat')
  assert.equal(b.filter((x) => x.role === 'assistant' && /interrupted/.test(x.text)).length, 3, 'the three barge-ins carry a subtle mark')
  assert.equal(host.calls.length, 0, 'still a projection: no command')
  dsh.dispose()
})

test('the user bubble is written when its reply STARTS, before the reply ends', async () => {
  const { bridge, dsh } = harness()
  const g = guest()
  bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  bridge.ingest(g('input_transcript', { text: 'Synthetic question.', utterance_id: 'p1-u1', utterance_seq: 0, association: 'unknown' }))
  const d = g('decision', { decision_id: 'p1-d1', policy: AUTO_VAD, selected: 'respond' })
  bridge.ingest(d)
  bridge.ingest(g('response_started', { cause: d.id, response_id: 'p1-v1', request_id: 'p1-mic' }))
  bridge.ingest(g('agent_delta', { cause: d.id, response_id: 'p1-v1', text: 'Still speaking' }))
  await new Promise((r) => setTimeout(r, 30))
  const users = dsh.events.filter((e) => e.type === 'user/message')
  assert.deepEqual(users.map((e) => e.data.content.map((c) => c.text).join('')), ['Synthetic question.'], 'shown while the reply is still streaming')
  bridge.ingest(g('agent_done', { cause: d.id, response_id: 'p1-v1', status: 'completed' }))
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => [x.role, x.text]), [['user', 'Synthetic question.'], ['assistant', 'Still speaking']])
  dsh.dispose()
})

test('adjacency edges: an unanswered utterance is its own plain bubble; one transcribed after its reply opened follows that reply', async () => {
  const host = fakeHost()
  const bridge = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { adjacentHoldMs: 40 } } })
  const dsh = fakeDsh()
  apply(dsh.ctx, {}, { env: { AUTOMAGIK_HOST: 'unfold' }, host, bridge })
  bridge.onLink('attached')
  const g = guest()
  // 1. heard, no reply: shown on its own after the hold, plain
  bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  bridge.ingest(g('input_transcript', { text: 'Synthetic aside.', utterance_id: 'p1-u1', utterance_seq: 0, association: 'unknown' }))
  await new Promise((r) => setTimeout(r, 80))
  await dsh.idle()
  // 2. a reply opens with nothing heard yet; its transcript arrives after it opened
  const d = g('decision', { decision_id: 'p1-d2', policy: AUTO_VAD, selected: 'respond' })
  bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  bridge.ingest(d)
  bridge.ingest(g('response_started', { cause: d.id, response_id: 'p1-v2', request_id: 'p1-mic' }))
  bridge.ingest(g('input_transcript', { text: 'Synthetic late words.', utterance_id: 'p1-u1', utterance_seq: 1, association: 'unknown' }))
  bridge.ingest(g('agent_delta', { cause: d.id, response_id: 'p1-v2', text: 'A reply.' }))
  bridge.ingest(g('agent_done', { cause: d.id, response_id: 'p1-v2', status: 'completed' }))
  await new Promise((r) => setTimeout(r, 80))
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => [x.role, x.text]), [['user', 'Synthetic aside.'], ['assistant', 'A reply.'], ['user', 'Synthetic late words.']])
  assert.deepEqual(dsh.bubbles().filter((x) => LABELS.test(x.text)), [])
  dsh.dispose()
})

test('the live listening view: the provider\'s speech start starts it, the words replace it, the reply clears it; served on <route>/live', async () => {
  const host = fakeHost()
  const bridge = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh()
  apply(dsh.ctx, {}, { env: { AUTOMAGIK_HOST: 'unfold' }, host, bridge })
  bridge.onLink('attached')
  const svc = dsh.provided.unfoldSession
  assert.ok(dsh.routes.some((r) => r.path === '/api/unfold-session/live'), 'the live route is registered')
  assert.equal(svc.live.view().state, 'idle')
  const g = guest()
  bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  assert.equal(svc.live.view().state, 'listening')
  bridge.ingest(g('input_transcript', { text: 'Synthetic words.', utterance_id: 'p1-u1', utterance_seq: 0, association: 'unknown' }))
  assert.deepEqual([svc.live.view().state, svc.live.view().text], ['heard', 'Synthetic words.'])
  spoken(g, 1, ['Ok.']).forEach((e) => bridge.ingest(e))
  assert.equal(svc.live.view().state, 'idle')
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => [x.role, x.text]), [['user', 'Synthetic words.'], ['assistant', 'Ok.']])
  assert.equal(submits(host).length, 0)
  svc.live.stop()
  dsh.dispose()
})
