// Delegation inside rth-dsh-session: the held rth-root task turn writes exactly one report bubble
// (no user bubble, no command), the floor is taken after step/start and before the append, the
// awaited flush result reaches the executor, the model-traffic counters are split, and only the
// admitted child session passes the pre-step.
import test from 'node:test'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'
import { RthBridge } from '../src/bridge.mjs'
import { apply, taskKeyOf } from '../index.mjs'
import { TranscriptJournal } from '../src/delegate/journal.mjs'

const llm = await import(pathToFileURL(createRequire(new URL('../../../shell/package.json', import.meta.url)).resolve('@deepseek-ai/dsh-llm')).href)
const sha = (s) => createHash('sha256').update(s).digest('hex')
const REPORT = '## Context\nThe report text.'

function fakeHost() {
  const calls = []
  return { calls, async submit(id, text) { calls.push({ id, text }); return { status: 200, body: {} } }, async cancel() { return { status: 200, body: {} } }, async status() { return { status: 200, body: {} } } }
}

function stubExecutor({ admitted = new Map(), gate = { ok: true }, prep = { text: REPORT, result_sha256: sha(REPORT) } } = {}) {
  const calls = []
  return {
    calls,
    stats: {},
    admittedChild: (sid) => admitted.get(sid) ?? null,
    beforeChildStep: (task) => { calls.push(['before', task]); return gate },
    prepareAppend: (id, seq) => { calls.push(['prepare', id, seq]); return prep },
    afterFlush: async (id, o) => { calls.push(['after', id, o.appendedSeq, o.flushError ? String(o.flushError.message) : null]) },
    onTaskState: async () => {},
  }
}

function fakeDsh({ flushThrows = false } = {}) {
  const handlers = {}
  const events = []
  let turn = 0
  let running = Promise.resolve()
  const session = {
    id: 'rth-root',
    header: {},
    get seq() { return events.length },
    append(type, data) { const ev = { type, data, seq: events.length }; events.push(ev); return ev },
    snapshotEvents: () => events,
  }
  let busy = false
  const nextTurn = []
  const agent = {
    id: 'rth-root',
    session,
    get status() { return busy ? 'running' : 'idle' },
    inbox: { nextTurn, nextStep: [], get hasPending() { return nextTurn.length > 0 }, remove: () => false },
    whenIdle: () => running,
    send(message, _t, wakeup) {
      nextTurn.push(message)
      if (!wakeup || busy) return
      busy = true
      const claimed = nextTurn.splice(0, 1)
      running = (async () => {
        try {
          const [pre] = handlers['agent/pre-step']
          assert.deepEqual(await pre({ agent, messages: claimed, turn: ++turn, signal: new AbortController().signal }, () => ({ kind: 'enter' })), { kind: 'reject' })
        } finally { busy = false }
      })()
    },
    followup(m) { agent.send(m, 'next-turn', true) },
  }
  const flushes = []
  const provided = {}
  const ctx = {
    effect: (fn) => fn(),
    on: (name, fn) => { (handlers[name] ??= []).push(fn) },
    inject: () => {},
    provide: (n, v) => { provided[n] = v },
    get: () => undefined,
    logger: { info: () => {}, warn: () => {} },
    events: { dispatch: () => [] },
    agents: { get: (id) => (id === 'rth-root' ? agent : undefined) },
    sessions: { flush: async (s) => { flushes.push(s.id); if (flushThrows) throw new Error('EIO'); return true } },
  }
  return { ctx, agent, events, handlers, flushes, provided, idle: async () => { for (let i = 0; i < 20; i++) { await new Promise((r) => setTimeout(r, 2)); await running } } }
}

function harness(opts = {}, ex = stubExecutor(), journal = new TranscriptJournal()) {
  const host = fakeHost()
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} } })
  const dsh = fakeDsh(opts)
  apply(dsh.ctx, {}, { host, bridge, executor: ex, journal })
  bridge.onLink('attached')
  return { host, bridge, dsh, ex }
}

const taskItem = (id) => llm.createUserMessage({ content: [{ type: 'text', text: '(relatório)' }], source: { kind: 'user', rpcId: `rth-task-${id}` } })

test('a report item is its own held turn: one report bubble, no user bubble, never a command', async () => {
  const { host, dsh, ex } = harness()
  dsh.agent.followup(taskItem('b1-t1'))
  await dsh.idle()
  const bubbles = dsh.events.filter((e) => e.type === 'user/message' || e.type === 'assistant/message')
  assert.equal(bubbles.length, 1)
  assert.equal(bubbles[0].type, 'assistant/message')
  assert.deepEqual(bubbles[0].data.message.source, { kind: 'model', provider: 'rth-task', model: 'rth-task-b1-t1' })
  assert.equal(bubbles[0].data.message.content[0].text, REPORT)
  assert.equal(host.calls.length, 0, 'report_never_becomes_a_command')
  // The floor is the seq of the append (taken after step/start, before the append).
  const prep = ex.calls.find((c) => c[0] === 'prepare')
  assert.equal(prep[2], bubbles[0].seq)
  assert.deepEqual(ex.calls.find((c) => c[0] === 'after'), ['after', 'b1-t1', bubbles[0].seq, null])
  assert.equal(dsh.events.filter((e) => e.type === 'step/start').length, dsh.events.filter((e) => e.type === 'step/end').length)
  assert.equal(dsh.flushes.length, 1, 'the flush is awaited after the append')
  assert.equal(taskKeyOf(taskItem('x')), 'rth-task-x')
})

test('the same key and digest already in the live session is not appended twice', async () => {
  const { dsh, ex } = harness()
  dsh.agent.followup(taskItem('b1-t1'))
  await dsh.idle()
  dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: '(relatório)' }], source: { kind: 'user', rpcId: 'rth-task-b1-t1' } }))
  await dsh.idle()
  assert.equal(dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
  const afters = ex.calls.filter((c) => c[0] === 'after')
  assert.equal(afters.length, 2)
  assert.equal(afters[1][2], afters[0][2], 'the existing row is reported, not a new one')
})

test('a flush failure reaches the executor as flushError', async () => {
  const { dsh, ex } = harness({ flushThrows: true })
  dsh.agent.followup(taskItem('b1-t2'))
  await dsh.idle()
  assert.equal(ex.calls.find((c) => c[0] === 'after')[3], 'EIO')
})

test('model traffic is split: rth-root counts as a violation, the admitted child as task traffic', async () => {
  const ex = stubExecutor({ admitted: new Map([['rth-task-b1-t1', 'b1-t1']]) })
  const { dsh } = harness({}, ex)
  const [req] = dsh.handlers['agent/request']
  const next = () => 'n'
  req({ agent: { id: 'rth-root', session: { id: 'rth-root' } } }, next)
  req({ agent: { id: 'c', session: { id: 'rth-task-b1-t1' } } }, next)
  req({ agent: { id: 'z', session: { id: 'other' } } }, next)
  const c = dsh.provided.rthSession.counters
  assert.equal(c.root_model_requests, 1, 'a root model request is counted as a violation')
  assert.deepEqual(c.task_model_requests, { 'b1-t1': 1 })
  assert.equal(c.other_model_requests, 1)
  const pre = dsh.handlers['agent/pre-step'][0]
  // An unregistered subagent session is refused; the admitted child passes to next().
  const stranger = await pre({ agent: { id: 's', session: { id: 'rth-task-unknown', header: { origin: 'subagent' } } }, messages: [] }, () => ({ kind: 'enter' }))
  assert.deepEqual(stranger, { kind: 'reject' })
  assert.equal(c.refused_subagents, 1)
  const admitted = await pre({ agent: { id: 'c', session: { id: 'rth-task-b1-t1', header: { origin: 'subagent' } } }, messages: [] }, () => ({ kind: 'enter' }))
  assert.deepEqual(admitted, { kind: 'enter' })
  assert.deepEqual(ex.calls.filter((c) => c[0] === 'before'), [['before', 'b1-t1']])
})

test('the admitted child is refused a step when its budget gate says no', async () => {
  const ex = stubExecutor({ admitted: new Map([['rth-task-b1-t1', 'b1-t1']]), gate: { ok: false, reason: 'budget:task' } })
  const { dsh } = harness({}, ex)
  const pre = dsh.handlers['agent/pre-step'][0]
  assert.deepEqual(await pre({ agent: { id: 'c', session: { id: 'rth-task-b1-t1', header: { origin: 'subagent' } } }, messages: [] }, () => ({ kind: 'enter' })), { kind: 'reject' })
})

test('task_state records from agentd reach the executor through the bridge', async () => {
  const seen = []
  const ex = { ...stubExecutor(), onTaskState: async (r) => { seen.push(r.task_id) } }
  const { bridge } = harness({}, ex)
  bridge.ingest({ v: 2, id: 'g:1', producer: 'g', seq: 1, source: 'guest', kind: 'task_state', task_id: 'b1-t9', execution: 'output_sent' })
  await new Promise((r) => setTimeout(r, 5))
  assert.deepEqual(seen, ['b1-t9'])
})

test('observed agentd envelopes feed the journal by their own ids (not chat adjacency)', async () => {
  const journal = new TranscriptJournal()
  const { bridge } = harness({}, stubExecutor(), journal)
  bridge.ingest({ v: 2, id: 'g:1', producer: 'guest-p1', seq: 1, source: 'guest', kind: 'input_transcript', text: 'oi', utterance_id: 'p1-u1', utterance_seq: 0, final: true, association: 'unknown' })
  bridge.ingest({ v: 2, id: 'g:2', producer: 'guest-p1', seq: 2, source: 'guest', kind: 'response_started', response_id: 'p1-v1', request_id: 'p1-mic-1' })
  const f = journal.freeze()
  assert.deepEqual(f.items.map((i) => [i.kind, i.id, i.finality]), [['utterance', 'p1-u1', 'final'], ['reply', 'p1-v1', 'open']])
})

test('a nested task_state record (the rth-protocol shape) reaches the executor as the record', async () => {
  const seen = []
  const ex = { ...stubExecutor(), onTaskState: async (r) => { seen.push([r.task_id, r.execution]) } }
  const { bridge } = harness({}, ex)
  bridge.ingest({ v: 2, id: 'g:2', producer: 'g', seq: 2, source: 'guest', kind: 'task_state', task_id: 'b1-t3', record: { task_id: 'b1-t3', execution: 'output_sent' } })
  await new Promise((r) => setTimeout(r, 5))
  assert.deepEqual(seen, [['b1-t3', 'output_sent']])
})
