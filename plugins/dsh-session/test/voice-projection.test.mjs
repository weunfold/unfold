// The DSH half of spoken turns: apply() against a small stand-in for the DSH host (one root
// agent whose turns run our agent/pre-step, a Session that records appends, the Session API's
// prompt). What lands in rth-root, and that no spoken turn ever becomes a command.
import test from 'node:test'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'
import { RthBridge } from '../src/bridge.mjs'
import { apply } from '../index.mjs'

const llm = await import(pathToFileURL(createRequire(new URL('../../../shell/package.json', import.meta.url)).resolve('@deepseek-ai/dsh-llm')).href)
const AUTO_VAD = 'provider:gemini-auto-vad@v1'

function fakeHost() {
  const calls = []
  return {
    calls,
    async submit(command_id, text) {
      calls.push({ op: 'submit', command_id, text })
      return { status: 200, body: { command: { command_id, status: 'dispatched', generation: 1, response_id: `resp-${command_id}` } } }
    },
    async cancel() {
      calls.push({ op: 'cancel' })
      return { status: 200, body: { ok: true } }
    },
    async status() {
      return { status: 200, body: {} }
    },
  }
}

/** A stand-in DSH: one live rth-root agent; queued messages run one turn each, in order. */
function fakeDsh({ live = true, sessionApi = true, stallPrompt = false, mind = null, prior = [] } = {}) {
  const handlers = {}
  // `prior`: rth-root's durable log from an earlier DSH process (a restart keeps it).
  const events = prior.map((e, seq) => ({ ...e, seq }))
  const prompts = []
  let turn = 0
  let running = Promise.resolve()
  const session = {
    id: 'rth-root',
    header: {},
    append(type, data) {
      const ev = { type, data, seq: events.length }
      events.push(ev)
      return ev
    },
    snapshotEvents: () => events,
  }
  // As DSH's AgentLoop (dsh-agent-loop 0.2.1-rc.2): a wake while idle starts the driver; a
  // turn claims EVERY next-step message plus the FIRST next-turn one; because our pre-step
  // rejects, `turn()` returns false and the driver goes idle (`while (await this.turn())`),
  // even with work pending. A wake while running is dropped, so that work stays parked.
  let busy = false
  const nextTurn = []
  const nextStep = []
  const agent = {
    id: 'rth-root',
    session,
    get status() {
      return busy ? 'running' : 'idle'
    },
    inbox: {
      nextTurn,
      nextStep,
      get hasPending() {
        return nextTurn.length > 0 || nextStep.length > 0
      },
      remove(id) {
        for (const list of [nextStep, nextTurn]) {
          const i = list.findIndex((m) => m.id === id)
          if (i >= 0) {
            list.splice(i, 1)
            return true
          }
        }
        return false
      },
    },
    whenIdle: () => running,
    send(message, target, wakeup) {
      ;(target === 'next-step' ? nextStep : nextTurn).push(message)
      if (!wakeup || busy) return
      busy = true
      const claimed = [...nextStep.splice(0), ...nextTurn.splice(0, 1)]
      running = (async () => {
        try {
          const [pre] = handlers['agent/pre-step']
          const decision = await pre({ agent, messages: claimed, turn: ++turn, signal: new AbortController().signal }, () => ({ kind: 'enter' }))
          assert.deepEqual(decision, { kind: 'reject' }, 'every rth-root turn is rejected: no model step')
        } finally {
          busy = false
        }
      })()
    },
    followup(message) {
      agent.send(message, 'next-turn', true)
    },
    steer(message) {
      agent.send(message, 'next-step', true)
    },
  }
  const hasRequest = (id) => [...nextTurn, ...nextStep].some((m) => m.source.rpcId === id) || events.some((e) => e.type === 'user/message' && e.data.source.rpcId === id)
  const sessionController = {
    async prompt(req) {
      prompts.push(req.requestId)
      if (stallPrompt) return new Promise(() => {})
      if (hasRequest(req.requestId)) return { accepted: true } // DSH's own durable dedupe
      agent.followup(llm.createUserMessage({ content: req.content, source: { kind: 'user', rpcId: req.requestId } }))
      return { accepted: true }
    },
  }
  const disposers = []
  const mindDisposers = []
  let mindInject = null
  const provided = {}
  const routes = []
  const sctx = {
    effect: (fn) => { const d = fn(); if (d) disposers.push(d) },
    get: () => ({ stat: async () => true }),
    connection: { fetch: { register: (r) => { routes.push(r); return () => {} } } },
    ...(sessionApi ? { sessionController } : {}),
    workspaceRegistry: {},
  }
  const ctx = {
    effect: (fn) => { const d = fn(); if (d) disposers.push(d) },
    on: (name, fn) => { (handlers[name] ??= []).push(fn) },
    inject: (deps, cb) => {
      // The Mind attachment is its own scope: its effects are disposed apart from the plugin's
      // (detachMind), and a new Mind can attach later (attachMind), as Cordis does on replacement.
      if (deps.includes('rthMind')) {
        mindInject = cb
        return mind ? cb({ ...sctx, rthMind: mind, effect: (fn) => { const d = fn(); if (d) mindDisposers.push(d) } }) : undefined
      }
      if (!deps.includes('sessionController') || sessionApi) cb(sctx)
    },
    provide: (name, v) => { provided[name] = v },
    get: () => undefined,
    logger: { info: () => {}, warn: () => {} },
    events: { dispatch: () => [] },
    serial: async () => {},
    agents: { get: (id) => (live && id === 'rth-root' ? agent : undefined) },
    sessions: { flush: async () => {} },
  }
  return {
    ctx,
    provided,
    routes,
    agent,
    events,
    prompts,
    handlers,
    idle: async () => { for (let i = 0; i < 40; i++) { await new Promise((r) => setTimeout(r, 5)); await running } },
    type: (text, mode = 'queue') => {
      const m = llm.createUserMessage({ content: [{ type: 'text', text }], source: { kind: 'user', rpcId: `browser-${text}` } })
      if (mode === 'steer') agent.steer(m)
      else agent.followup(m)
      return m
    },
    bubbles: () => events.filter((e) => e.type === 'user/message' || e.type === 'assistant/message').map((e) => ({ role: e.type.split('/')[0], text: e.data.message ? e.data.message.content.map((c) => c.text).join('') : e.data.content.map((c) => c.text).join(''), rpcId: e.data.source?.rpcId, model: e.data.message?.source?.model })),
    dispose: () => { mindDisposers.splice(0).forEach((d) => d()); disposers.forEach((d) => typeof d === 'function' && d()) },
    detachMind: () => mindDisposers.splice(0).forEach((d) => d()),
    attachMind: (m) => mindInject({ ...sctx, rthMind: m, effect: (fn) => { const d = fn(); if (d) mindDisposers.push(d) } }),
  }
}

function harness(opts, deps = {}) {
  const host = fakeHost()
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { stopMs: 200, reaskMs: [20, 40], reconcileAttempts: 2, voiceGapWaitMs: 50, voiceIdleMs: 2000 } })
  const dsh = fakeDsh(opts)
  apply(dsh.ctx, {}, { host, bridge, ...deps })
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

// Clean bubbles: the expected TEXTS below are the words only, no labels; the reply's user bubble
// carries the words heard before it (adjacency) and none when nothing was heard.
test('rth-root gets one user bubble (the words) and one reply for a spoken turn; no command is generated', async () => {
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
  assert.equal(b[1].model, 'rth-agentd voice (p1-v1, decision p1-d1)')
  assert.equal(b[0].rpcId, 'rth-voice-r-p1-v1', 'the words are the reply\'s own user bubble')
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
  assert.ok(submits(host).every((c) => c.command_id.startsWith('dsh-')))
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
  dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: '(voice) reply p1-v9' }], source: { kind: 'user', rpcId: 'rth-voice-r-p1-v9' } }))
  dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: '(heard; association unknown) earlier words' }], source: { kind: 'user', rpcId: 'rth-voice-h-u-x' } }))
  await dsh.idle()
  const b = dsh.bubbles()
  // The forgotten reply: no user bubble, one short mark; a queued note keeps the text it was
  // queued with (here one in an older format, labels included).
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

test('a cold rth-root: spoken items wait (bounded) and are delivered when the agent appears', async () => {
  const host = fakeHost()
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh({ live: false, sessionApi: false })
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  const g = guest()
  spoken(g, 6, ['Later.']).forEach((e) => bridge.ingest(e))
  await dsh.idle()
  assert.equal(dsh.bubbles().length, 0)
  dsh.ctx.agents.get = (id) => (id === 'rth-root' ? dsh.agent : undefined)
  for (const fn of dsh.handlers['agent/created']) fn({ agent: dsh.agent })
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => x.role), ['assistant'], 'nothing was heard for it: the reply alone')
  assert.equal(host.calls.length, 0)
  dsh.dispose()
})

test('recorded journal (bench voice run) into rth-root: 4 utterances, each followed by its reply, zero commands', async () => {
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
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { holdMs: 30, retainMs: 60 } } })
  const dsh = fakeDsh()
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  const g = guest()
  bridge.ledger.adjacency = false // the id contract, adjacency off
  const part = (u, seq, text, final) => g('input_transcript', { text, utterance_id: u, utterance_seq: seq, ...(final ? { final: true } : {}), association: 'unknown' })
  // 1. a partial note is shown, then the rest and the end flag arrive
  bridge.ingest(part('p1-u30', 0, 'Name one ', false))
  await new Promise((r) => setTimeout(r, 50))
  bridge.ingest(part('p1-u30', 1, 'planet', true))
  // 2. the last part (with the end flag) first; the first part never arrives
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

// ── liveness: user work queued during an rth-root turn gets its next wake ──

/** rth-agentd's side of a typed command, as the observer sees it. */
function answerCommands(host, bridge, g, reply = (text) => `re: ${text}`) {
  const submit = host.submit
  host.submit = async (command_id, text) => {
    const r = await submit(command_id, text)
    if (host.calls.filter((c) => c.op === 'submit' && c.command_id === command_id).length === 1) {
      setTimeout(() => {
        const rid = `resp-${command_id}`
        bridge.ingest(g('response_started', { response_id: rid, request_id: command_id }))
        bridge.ingest(g('agent_delta', { response_id: rid, text: reply(text) }))
        bridge.ingest(g('agent_done', { response_id: rid, status: 'completed' }))
      }, 5)
    }
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
  assert.deepEqual(submits(host).map((c) => [c.command_id, c.text]), [[`dsh-${hi.id}`, 'hi']], 'exactly once, its original command id')
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
  assert.deepEqual(submits(host).map((c) => [c.command_id, c.text]), [[`dsh-${a.id}`, 'first'], [`dsh-${b.id}`, 'second']])
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
  assert.deepEqual(submits(host).map((c) => [c.command_id, c.text]), [[`dsh-${s1.id}`, 'steer one'], [`dsh-${s2.id}`, 'steer two']])
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
// For backends that cannot pair a transcript with its reply by id (Gemini: association "unknown",
// one utterance_id for the whole call). Such transcripts are grouped by ADJACENCY: a provider
// voice_activity_start begins a new utterance, the next reply that opens closes it and takes it as
// its user bubble, written when the reply STARTS. No labels in chat; the association qualifiers
// stay in Trajectory and the Mind audit. Id pairing is kept for a backend that can prove it.
const LABELS = /association|heard|update|spoken reply|\(voice|provider opened|not identified|\[cut off|\[possibly/
const recordedCall = () => readFileSync(new URL('fixtures/voice-call-shape.jsonl', import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))

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
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { adjacentHoldMs: 40 } } })
  const dsh = fakeDsh()
  apply(dsh.ctx, {}, { host, bridge })
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

test('the live listening view: a host onset (through Mind) starts it with its level, the words replace it, the reply clears it; served on <route>/live', async () => {
  const listeners = new Set()
  const envs = new Map()
  const mind = {
    mind: { onChange: (fn) => (listeners.add(fn), () => listeners.delete(fn)) },
    detail: (id) => (envs.has(id) ? { envelope: envs.get(id) } : null),
  }
  const row = (id, env) => { envs.set(id, env); for (const fn of listeners) fn('row', { id, lane: 'host', kind: env.kind, history: false }) }
  const host = fakeHost()
  // The onset shortcut needs fresh applied facts (an active call with its input open, the mic unmuted)
  host.status = async () => ({ status: 200, body: { host: { call: { state: 'active', input: { call_input: 'open', applied_closed: false } }, mic: { state: 'unmuted', applied: { muted: false } } } } })
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh({ mind })
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  const svc = dsh.provided.rthSession
  assert.ok(dsh.routes.some((r) => r.path === '/api/rth-session/live'), 'the live route is registered')
  row('m1', { kind: 'local_vad_onset', action: 'observed', audible: true, level_dbfs: -30 })
  assert.equal(svc.live.view().state, 'idle', 'an onset during playback does not start it')
  row('m2', { kind: 'local_vad_onset', action: 'nothing_audible', audible: false, level_dbfs: -16.04 })
  for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)) // the facts are read at the onset
  assert.deepEqual(svc.live.view().level, { dbfs: -16, basis: 'level at speech onset (rth-host)' })
  const g = guest()
  bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  bridge.ingest(g('input_transcript', { text: 'Synthetic words.', utterance_id: 'p1-u1', utterance_seq: 0, association: 'unknown' }))
  assert.deepEqual([svc.live.view().state, svc.live.view().text], ['heard', 'Synthetic words.'])
  spoken(g, 1, ['Ok.']).forEach((e) => bridge.ingest(e))
  assert.equal(svc.live.view().state, 'idle')
  await dsh.idle()
  assert.deepEqual(dsh.bubbles().map((x) => [x.role, x.text]), [['user', 'Synthetic words.'], ['assistant', 'Ok.']])
  const st = await svc.status()
  assert.equal(st.live.mind, 'bound')
  assert.equal(st.live.onsets, 2)
  svc.live.stop()
  dsh.dispose()
})

// ── a local onset is not "listening" unless the input is admitted ──
// rth-host's VAD runs before its admission gate: `local_vad_onset { audible:false }` is logged even
// while muted. The onset shortcut is taken only on fresh applied facts read at that moment (call
// active with its input open, mic applied unmuted); anything else waits for the provider's signal.
function muteRig(statusBody, { status = 200 } = {}) {
  const listeners = new Set()
  const envs = new Map()
  const mind = { mind: { onChange: (fn) => (listeners.add(fn), () => listeners.delete(fn)) }, detail: (id) => ({ envelope: envs.get(id) }) }
  const host = fakeHost()
  host.status = async () => ({ status, body: statusBody })
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh({ mind })
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  let n = 0
  const row = (env) => { const id = `m${++n}`; envs.set(id, env); for (const fn of listeners) fn('row', { id, lane: 'host', kind: env.kind, history: false }) }
  const svc = dsh.provided.rthSession
  return { svc, row, host, bridge, dsh, settle: async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)) }, done: () => { svc.live.stop(); dsh.dispose() } }
}
const ONSET = { kind: 'local_vad_onset', action: 'nothing_audible', audible: false, level_dbfs: -16 }
const OPEN = { host: { call: { state: 'active', desired: 'on', input: { applied_closed: false, call_input: 'open' } }, mic: { state: 'unmuted', muted: false, applied: { muted: false } } } }

test('a local onset while the mic is applied-muted never claims listening', async () => {
  const r = muteRig({ host: { call: OPEN.host.call, mic: { state: 'muted', muted: true, applied: { muted: true } } } })
  r.row(ONSET)
  await r.settle()
  assert.notEqual(r.svc.live.view().state, 'listening', JSON.stringify(r.svc.live.view()))
  r.done()
})

test('muting, unmuting, closed input, a call that is off, unknown or unreadable facts: no onset shortcut', async () => {
  const cases = [
    { host: { call: OPEN.host.call, mic: { state: 'muting', muted: false, applied: { muted: false } } } },
    { host: { call: OPEN.host.call, mic: { state: 'unmuting', muted: true, applied: { muted: true } } } },
    { host: { call: { ...OPEN.host.call, input: { applied_closed: true, call_input: 'closed' } }, mic: OPEN.host.mic } },
    { host: { call: { state: 'off', desired: 'off' }, mic: OPEN.host.mic } },
    { host: { mic: OPEN.host.mic } },
    {},
  ]
  for (const body of cases) {
    const r = muteRig(body)
    r.row(ONSET)
    await r.settle()
    assert.equal(r.svc.live.view().state, 'idle', JSON.stringify(body))
    r.done()
  }
  const u = muteRig(OPEN, { status: 503 })
  u.row(ONSET)
  await u.settle()
  assert.equal(u.svc.live.view().state, 'idle', 'owner unreadable')
  u.done()
})

test('control: an admitted, unmuted onset is listening with its level; a later applied mute clears it', async () => {
  const r = muteRig(OPEN)
  r.row(ONSET)
  await r.settle()
  assert.deepEqual([r.svc.live.view().state, r.svc.live.view().level?.dbfs], ['listening', -16])
  r.row({ kind: 'mute_applied', muted: true, generation: 4 })
  await r.settle()
  assert.equal(r.svc.live.view().state, 'idle', 'the mute ends the indication')
  // the provider's own speech signal still shows listening, with no local level
  const g = guest()
  r.bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  assert.deepEqual([r.svc.live.view().state, r.svc.live.view().level], ['listening', null])
  r.row({ kind: 'call_devices_released', why: 'end' })
  await r.settle()
  assert.equal(r.svc.live.view().state, 'idle', 'devices released ends it too')
  r.done()
})

// ── onset races: an owner read that resolves after a newer
// boundary (applied mute, call off, devices released, a reply or a note) is dropped: a stale "open"
// never publishes listening. Each onset read is fenced by an epoch that every such boundary, and
// unload, advances; the token is checked immediately before publishing.
function deferredRig() {
  const listeners = new Set()
  const envs = new Map()
  const reads = []
  const mind = { mind: { onChange: (fn) => (listeners.add(fn), () => listeners.delete(fn)) }, detail: (id) => ({ envelope: envs.get(id) }) }
  const host = fakeHost()
  host.status = () => new Promise((resolve) => reads.push(resolve))
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const dsh = fakeDsh({ mind })
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  let n = 0
  const row = (env) => { const id = `d${++n}`; envs.set(id, env); for (const fn of listeners) fn('row', { id, lane: 'host', kind: env.kind, history: false }) }
  const svc = dsh.provided.rthSession
  return { row, reads, bridge, dsh, svc, settle: async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)) } }
}
const OPEN_READ = { status: 200, body: OPEN }

for (const boundary of ['mute', 'sleep', 'released', 'reply', 'note']) {
  test(`onset race: an onset read answered after ${boundary} publishes no listening; a fresh onset after it recovers`, async () => {
    const r = deferredRig()
    try {
      r.row(ONSET)
      assert.equal(r.reads.length, 1)
      const g = guest()
      if (boundary === 'mute') r.row({ kind: 'mute_applied', muted: true, generation: 4 })
      if (boundary === 'sleep') r.row({ kind: 'call', state: 'off' })
      if (boundary === 'released') r.row({ kind: 'call_devices_released', why: 'end' })
      if (boundary === 'reply') spoken(g, 1, ['Ok.']).slice(0, 2).forEach((e) => r.bridge.ingest(e))
      if (boundary === 'note') r.bridge.emit('voice-heard', { key: 'x', text: 'Words shown on their own.', final: true })
      assert.equal(r.svc.live.view().state, 'idle')
      r.reads[0](OPEN_READ) // the read sampled OPEN before the boundary; its answer arrives only now
      await r.settle()
      assert.equal(r.svc.live.view().state, 'idle', 'the stale answer publishes nothing')
      // recovery control: new evidence after the boundary (a fresh onset with a fresh open read)
      r.row(ONSET)
      r.reads[1](OPEN_READ)
      await r.settle()
      assert.equal(r.svc.live.view().state, 'listening')
    } finally {
      r.svc.live.stop()
      r.dsh.dispose()
    }
  })
}

test('onset race: a read answered after unload publishes nothing; a current read still admits (control)', async () => {
  const r = deferredRig()
  r.row(ONSET)
  r.dsh.dispose() // plugin unload
  r.reads[0](OPEN_READ)
  await r.settle()
  assert.equal(r.svc.live.view().state, 'idle')
  r.svc.live.stop()
  const c = deferredRig()
  c.row(ONSET)
  c.reads[0](OPEN_READ)
  await c.settle()
  assert.equal(c.svc.live.view().state, 'listening', 'control: no boundary, the read admits the onset')
  c.svc.live.stop()
  c.dsh.dispose()
})

// ── Mind attachment lifetime: disposing ONLY the Mind attachment, or
// replacing it, invalidates the reads it started; the plugin stays usable and a new attachment's
// fresh onset is admitted. The plugin's own unload flag is not used for this.
function mindOf() {
  const listeners = new Set()
  const envs = new Map()
  let n = 0
  return {
    svc: { mind: { onChange: (fn) => (listeners.add(fn), () => listeners.delete(fn)) }, detail: (id) => ({ envelope: envs.get(id) }) },
    row: (env) => { const id = `a${++n}`; envs.set(id, env); for (const fn of listeners) fn('row', { id, lane: 'host', kind: env.kind, history: false }) },
    listeners,
  }
}
function attachRig() {
  const reads = []
  const host = fakeHost()
  host.status = () => new Promise((resolve) => reads.push(resolve))
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000 } })
  const m1 = mindOf()
  const dsh = fakeDsh({ mind: m1.svc })
  apply(dsh.ctx, {}, { host, bridge })
  bridge.onLink('attached')
  const svc = dsh.provided.rthSession
  // status() also reads the (deferred) owner: answer that read so the status route returns
  const status = async () => { const p = svc.status(); await new Promise((r) => setImmediate(r)); reads.at(-1)({ status: 200, body: {} }); return p }
  return { reads, bridge, dsh, m1, svc, status, settle: async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r)) } }
}

test('onset attachment: a read from a detached Mind attachment publishes nothing; the plugin stays usable', async () => {
  const r = attachRig()
  try {
    r.m1.row(ONSET)
    assert.equal(r.reads.length, 1)
    r.dsh.detachMind() // only the attachment's own disposer; rth-session stays loaded
    assert.equal(r.m1.listeners.size, 0)
    r.reads[0](OPEN_READ)
    await r.settle()
    assert.equal(r.svc.live.view().state, 'idle', 'a read belonging to a detached attachment must not publish')
    const g = guest()
    r.bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
    assert.equal(r.svc.live.view().state, 'listening', 'the plugin is still live: provider speech shows listening')
    assert.equal((await r.status()).live.mind, 'not loaded')
  } finally {
    r.svc.live.stop()
    r.dsh.dispose()
  }
})

test('onset attachment: replaced while an old read is pending: the old answer is rejected, a fresh onset from the replacement is admitted', async () => {
  const r = attachRig()
  try {
    r.m1.row(ONSET)
    r.dsh.detachMind()
    const m2 = mindOf()
    r.dsh.attachMind(m2.svc) // the replacement Mind attaches before the old read answers
    r.reads[0](OPEN_READ)
    await r.settle()
    assert.equal(r.svc.live.view().state, 'idle', 'the detached attachment\'s answer is rejected')
    m2.row(ONSET)
    r.reads[1](OPEN_READ)
    await r.settle()
    assert.deepEqual([r.svc.live.view().state, r.svc.live.view().level?.dbfs], ['listening', -16], 'the replacement\'s fresh onset')
    assert.equal((await r.status()).live.mind, 'bound')
    assert.ok((await r.status()).live.onsets_superseded >= 1)
  } finally {
    r.svc.live.stop()
    r.dsh.dispose()
  }
})

// Heard-note request ids must not restart at `a1` in every DSH process: DSH treats a request id
// already in the durable log as accepted, so an unbound utterance after a restart would be silently
// swallowed. A per-ledger epoch keeps them apart.
test('after a DSH restart, an unbound utterance with a reused segment number is still shown', async () => {
  const run = async (prior, text) => {
    const host = fakeHost()
    const bridge = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { adjacentHoldMs: 40 } } })
    const dsh = fakeDsh({ prior })
    apply(dsh.ctx, {}, { host, bridge })
    bridge.onLink('attached')
    const g = guest()
    bridge.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
    bridge.ingest(g('input_transcript', { text, utterance_id: 'p1-u1', utterance_seq: 0, association: 'unknown' }))
    await new Promise((r) => setTimeout(r, 80))
    await dsh.idle()
    dsh.dispose()
    return dsh
  }
  const first = await run([], 'Synthetic first run.')
  const second = await run(first.events, 'Synthetic second run.')
  assert.deepEqual(second.bubbles().map((x) => [x.role, x.text]), [['user', 'Synthetic first run.'], ['user', 'Synthetic second run.']])
  const ids = second.events.filter((e) => e.type === 'user/message').map((e) => e.data.source.rpcId)
  assert.equal(new Set(ids).size, 2, `distinct note ids across the restart: ${ids}`)
  assert.ok(ids.every((id) => /^rth-voice-h-[0-9a-f]{8}-a1$/.test(id)), ids.join(' '))
})
