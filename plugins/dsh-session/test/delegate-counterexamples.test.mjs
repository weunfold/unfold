// Delegation counterexamples, each with the companion invariants it must keep: a delayed or lost
// answer, a false flush, a restart mid-publication, a repeated envelope, a missing launch id. The
// executor runs against agentd's REAL ledger rules through the test-only `rth-tasks-stdio` seam;
// the plugin runs through its own `apply()` over a fake DSH. Zero spend: no provider, no DSH
// child, no network.
import nodeTest from 'node:test'
import { tasksStdio } from '../../../test/rth-checkout.mjs'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { createHash } from 'node:crypto'
import { createRequire } from 'node:module'
import { existsSync, mkdtempSync, unlinkSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { DelegateExecutor, RUN_MS } from '../src/delegate/executor.mjs'
import { PendingStore } from '../src/delegate/pending.mjs'
import { SpendLedger } from '../src/delegate/budget.mjs'
import { buildSnapshot } from '../src/delegate/snapshot.mjs'
import { TranscriptJournal } from '../src/delegate/journal.mjs'
import { RthBridge } from '../src/bridge.mjs'
import { apply, delegationSnapshot } from '../index.mjs'

const llm = await import(pathToFileURL(createRequire(new URL('../../../shell/package.json', import.meta.url)).resolve('@deepseek-ai/dsh-llm')).href)
const STDIO = tasksStdio()
// The rth-tasks ledger these tests drive is rth's binary: $RTH_TASKS_STDIO, else the rth
// checkout's debug build. Without it, a test that
// starts the ledger is SKIPPED with the reason (ledger() throws NoStdio, the wrapper turns exactly
// that into t.skip); every other test, and every other error, is unchanged.
class NoStdio extends Error {}
const test = (name, ...rest) => {
  const fn = rest.pop()
  return nodeTest(name, ...rest, async (t) => {
    try { return await fn(t) } catch (e) { if (e instanceof NoStdio) return t.skip(e.message); throw e }
  })
}
const sha = (s) => createHash('sha256').update(s).digest('hex')
const PRICES = { model: 'fake-mini', inputPerMTok: 0.4, outputPerMTok: 1.6, reasoningPerMTok: 1.6, source: 'test fixture, not a real price', pinnedAt: '2026-10-23' }
const REPORT = `## Context\n${'The owner tested RTH and found the chat garbled. '.repeat(8)}\n## Requests\n- a report for the coding agent\nSUMMARY: The chat projection merged utterances and the persona claimed tools it lacked.`
const LINEAGE = 'rth-root@2026-10-22T20:30Z'
const tick = (ms = 15) => new Promise((r) => setTimeout(r, ms))
const inc = (launch, pi) => ({ launch_id: launch, pid: 4242, plugin_instance: pi })
/** The task's run, once it has started (the claim's answer comes from another process). */
async function ran(ex, id) {
  await until(() => !!ex.tasks.get(id)?.done) // a boolean: `until` awaits what it is given
  await ex.tasks.get(id).done
}
/** Wait for a condition on real time (the ledger is another process), bounded. */
async function until(cond, ms = 4000) {
  const end = Date.now() + ms
  while (!(await cond())) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${cond}`)
    await tick(5)
  }
}

async function ledger(t, logPath = join(mkdtempSync(join(tmpdir(), 'rth-d-')), 'tasks.jsonl'), boot = 'b1') {
  if (!existsSync(STDIO)) throw new NoStdio(`needs the rth-tasks-stdio binary (cargo build -p rth-tasks --bin rth-tasks-stdio in an rth checkout; RTH_TASKS_STDIO or RTH_CHECKOUT): ${STDIO}`)
  const p = spawn(STDIO, [logPath, 'inst', boot], { stdio: ['pipe', 'pipe', 'inherit'] })
  const rl = createInterface({ input: p.stdout })
  const waiting = []
  rl.on('line', (l) => waiting.shift()?.(JSON.parse(l)))
  const next = () => new Promise((r) => waiting.push(r))
  const ready = await next()
  t.after(() => p.kill())
  const op = async (o) => { const r = next(); p.stdin.write(`${JSON.stringify(o)}\n`); return r }
  return { op, ready, logPath }
}

function clock() {
  let now = 1000
  const timers = []
  return {
    now: () => now,
    advance(ms) { now += ms; for (const t of timers.filter((x) => !x.done && x.at <= now)) { t.done = true; t.f() } },
    setTimer: (f, ms) => { const t = { f, at: now + ms, done: false }; timers.push(t); return t },
    clearTimer: (t) => { if (t) t.done = true },
  }
}

/** A scripted child that counts its starts. */
function child(counter, script = {}) {
  return ({ hooks }) => {
    counter.spawned++
    let settle
    const result = new Promise((r) => { settle = r })
    ;(async () => {
      const gate = hooks.beforeStep()
      if (!gate.ok) return settle({ turnEnd: 'blocked', stopReason: null, text: '' })
      await null
      hooks.afterStep({ inputTokens: 9000, outputTokens: 600 }, 40_000)
      if (script.hang) return
      settle({ turnEnd: 'completed', stopReason: 'stop', interrupted: false, text: REPORT, toolAttempts: 0 })
    })()
    return { result, dispose: () => settle({ turnEnd: 'aborted', stopReason: null, text: '' }) }
  }
}

/**
 * One executor over the real ledger. `answer(update, record)` may replace or hold the owner's
 * HTTP answer to an update (default: `{status: 200, body: {record}}`).
 */
async function rig(t, { me = inc('L1', 'p1'), L, dir = mkdtempSync(join(tmpdir(), 'rth-dx-')), rows = [], script = {}, answer } = {}) {
  L ??= await ledger(t)
  const c = clock()
  const counter = { spawned: 0 }
  const offered = []
  const updates = []
  let ex
  ex = new DelegateExecutor({
    incarnation: me,
    recipient: { provider: 'openai', model: 'fake-mini' },
    pending: new PendingStore(join(dir, 'pending')),
    spend: new SpendLedger(join(dir, 'spend.jsonl'), { prices: PRICES, now: () => Date.UTC(2026, 9, 23, 12) }),
    clock: c.now,
    setTimer: c.setTimer,
    clearTimer: c.clearTimer,
    postTask: async (taskId, update) => {
      updates.push(update)
      const r = await L.op({ op: 'update', update, now: c.now() })
      const http = { status: 200, body: { record: r.record } }
      return answer ? answer(update, r.record, http) : http
    },
    postExecutor: async (s) => L.op({ op: 'executor_status', ...s, now: c.now() }),
    snapshotSource: () => {
      const j = new TranscriptJournal()
      j.ingest({ producer: 'guest-p1', seq: 1, kind: 'input_transcript', utterance_id: 'p1-u1', utterance_seq: 0, text: 'manda um relatório pro meu agente', final: true })
      return j.freeze()
    },
    offerTaskItem: (i) => offered.push(i),
    spawnChild: child(counter, script),
    readBack: async () => (rows === 'error' ? { state: 'error' } : { state: 'ok', rows, lineage: LINEAGE }),
  })
  await ex.heartbeat(true)
  const call = async (id = 'c1') => {
    const a = await L.op({ op: 'tool_call', name: 'delegate', call_key: { instance_id: 'inst', provider_generation: 1, call_id: id }, args: { purpose: 'report', task: 'relatório pro meu agente de código', audience: 'coding_agent' }, now: c.now() })
    const s = await L.op({ op: 'output_sent', task_id: a.task_id, now: c.now() })
    void ex.onTaskState(s.record)
    await tick(5)
    return a
  }
  const get = async (id) => (await L.op({ op: 'get', task_id: id })).record
  return { L, c, ex, counter, offered, updates, rows, call, get, dir }
}

/** Hold the HTTP answer to the first update of `kind` until `release()`. */
function holdFirst(kind) {
  const h = { entered: null, release: null }
  h.reached = new Promise((r) => { h.entered = r })
  let held = false
  h.answer = (update, _rec, http) => {
    if (held || update.kind.kind !== kind) return http
    held = true
    h.entered()
    return new Promise((r) => { h.release = () => r(http) })
  }
  return h
}

/** Lose the first answer to an update of `kind` (the owner's 504), after the ledger committed it. */
function loseFirst(kind) {
  let lost = false
  return (update, _rec, http) => {
    if (lost || update.kind.kind !== kind) return http
    lost = true
    return { status: 504, body: { error: 'no committed answer in time; resend the identical update', retry: true } }
  }
}

// ── a delayed running acknowledgement must not start a child after settlement ──────

test('a held running answer, then the local deadline: settled, and no child starts on release', async (t) => {
  const h = holdFirst('running')
  const r = await rig(t, { answer: h.answer })
  const a = await r.call()
  await h.reached
  r.c.advance(RUN_MS + 1)
  await tick()
  const before = await r.get(a.task_id)
  assert.equal(before.execution, 'settled_failed')
  assert.equal(before.reason, 'deadline')
  h.release()
  await ran(r.ex, a.task_id)
  await tick()
  assert.equal(r.counter.spawned, 0, 'zero child starts after settlement')
  assert.equal((await r.get(a.task_id)).execution, 'settled_failed')
  // The stale running receipt does not restore running authority.
  assert.equal(r.ex.tasks.get(a.task_id).state.execution, 'settled_failed')
  assert.equal(r.ex.admittedChild(before.child_id), null)
})

test('a held running answer, then a committed UI cancel: cancelled, and no child starts', async (t) => {
  const h = holdFirst('running')
  const r = await rig(t, { answer: h.answer })
  const a = await r.call()
  await h.reached
  const rec = await r.ex.cancelIntent(a.task_id)
  assert.equal(rec.execution, 'cancel_requested')
  await r.ex.onTaskState(rec)
  await tick()
  assert.equal((await r.get(a.task_id)).execution, 'cancelled', 'no activation exists and none can start: exact teardown')
  h.release()
  await ran(r.ex, a.task_id)
  await tick()
  assert.equal(r.counter.spawned, 0)
  assert.equal((await r.get(a.task_id)).execution, 'cancelled')
})

test('a held running answer, then plugin disposal: no child starts on release', async (t) => {
  const h = holdFirst('running')
  const r = await rig(t, { answer: h.answer })
  const a = await r.call()
  await h.reached
  r.ex.dispose()
  h.release()
  await ran(r.ex, a.task_id)
  await tick()
  assert.equal(r.counter.spawned, 0)
  assert.equal(r.ex.admittedChild(r.ex.tasks.get(a.task_id).childId), null)
})

test('a held claim answer, then agentd\'s own deadline: settled; the claim answer starts nothing', async (t) => {
  const h = holdFirst('claim')
  const r = await rig(t, { answer: h.answer })
  const a = await r.call()
  await h.reached
  // One transition per tick: claimed → start_uncertain (no running) → cancel_requested (deadline).
  for (let i = 0; i < 2; i++) {
    const ticked = await r.L.op({ op: 'tick', now: r.c.now() + RUN_MS + 1 })
    for (const rec of ticked.records) await r.ex.onTaskState(rec)
  }
  // agentd's own deadline cancel: no activation exists and none can start, so the executor's
  // exact teardown settles it as failed{deadline}, freeing the slot.
  await until(async () => (await r.get(a.task_id)).execution === 'settled_failed')
  assert.equal((await r.get(a.task_id)).reason, 'deadline')
  h.release()
  await tick()
  assert.equal(r.counter.spawned, 0)
  assert.equal(r.ex.stats.runs, 0)
})

// ── lost answers: identical retries and the committed stream, never a second start ─

test('a lost claim answer is recovered from the committed stream record: one run, delivered', async (t) => {
  const r = await rig(t, { answer: loseFirst('claim') })
  const a = await r.call()
  const committed = await r.get(a.task_id)
  assert.equal(committed.execution, 'claimed')
  await r.ex.onTaskState(committed)
  await tick()
  const tk = r.ex.tasks.get(a.task_id)
  assert.ok(tk.done, 'the run started')
  await tk.done
  r.c.advance(30_000) // any pending retry of the claim fires now; it must not start a second run
  await tick()
  assert.equal(r.ex.stats.runs, 1)
  assert.equal(r.counter.spawned, 1)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
})

test('a lost claim answer with no stream record: the identical claim is resent and answered once', async (t) => {
  const r = await rig(t, { answer: loseFirst('claim') })
  const a = await r.call()
  await until(() => r.ex.stats.resends === 1)
  r.c.advance(300)
  await tick()
  await ran(r.ex, a.task_id)
  const claims = r.updates.filter((u) => u.kind.kind === 'claim')
  assert.equal(claims.length, 2)
  assert.deepEqual(claims[1], claims[0], 'the same identity, sequence and payload')
  assert.equal(r.counter.spawned, 1)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
})

test('every claim answer lost through the resends: unknown, then the committed stream record starts it once', async (t) => {
  const r = await rig(t, { answer: (u, _rec, http) => (u.kind.kind === 'claim' ? { status: 504, body: { retry: true } } : http) })
  const a = await r.call()
  for (let i = 0; i < 6; i++) { await until(() => r.ex.stats.resends === Math.min(i + 1, 5) || r.ex.stats.unknown_answers === 1); r.c.advance(10_000) }
  await until(() => r.ex.stats.unknown_answers === 1)
  assert.equal(r.ex.stats.runs, 0, 'an unknown claim starts nothing')
  await r.ex.onTaskState(await r.get(a.task_id))
  await r.ex.onTaskState(await r.get(a.task_id)) // a duplicate stream record starts nothing more
  await ran(r.ex, a.task_id)
  assert.equal(r.ex.stats.runs, 1)
  assert.equal(r.counter.spawned, 1)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
})

test('a lost running answer: resent identically; the child starts exactly once', async (t) => {
  const r = await rig(t, { answer: loseFirst('running') })
  const a = await r.call()
  await until(() => r.ex.stats.resends === 1)
  r.c.advance(300)
  await tick()
  await ran(r.ex, a.task_id)
  assert.equal(r.counter.spawned, 1)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
})

test('a lost publish answer: resent; one pending file, one offered task turn', async (t) => {
  const r = await rig(t, { answer: loseFirst('publish_request') })
  const a = await r.call()
  await until(() => r.ex.stats.resends === 1)
  r.c.advance(300)
  await tick()
  await ran(r.ex, a.task_id)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'ok')
  assert.equal(r.offered.length, 1)
})

test('a lost delivered answer: the pending file stays until the resent delivered is answered', async (t) => {
  const r = await rig(t, { answer: loseFirst('delivered') })
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const prep = r.ex.prepareAppend(a.task_id, 10)
  r.rows.push({ seq: 10, digest: sha(prep.text) })
  const out = r.ex.afterFlush(a.task_id, { appendedSeq: 10, appendedNow: true, flushed: true })
  await until(() => r.ex.stats.resends === 1)
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'ok', 'kept while delivery is not established')
  r.c.advance(300)
  assert.equal((await out).outcome, 'delivered')
  const delivered = r.updates.filter((u) => u.kind.kind === 'delivered')
  assert.equal(delivered.length, 2)
  assert.deepEqual(delivered[1], delivered[0], 'the same pub_epoch, pub_seq and receipt')
  assert.equal((await r.get(a.task_id)).publication, 'delivered')
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'missing')
})

// ── the plugin over a fake DSH (index.mjs's production wiring) ──────────────────────────

function fakeHost() {
  return { calls: [], async submit() { return { status: 200, body: {} } }, async cancel() { return { status: 200, body: {} } }, async status() { return { status: 200, body: {} } } }
}

function fakeDsh({ flush = async () => true } = {}) {
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
  const drain = () => {
    if (busy || !nextTurn.length) return
    busy = true
    const claimed = nextTurn.splice(0)
    running = (async () => {
      try {
        const [pre] = handlers['agent/pre-step']
        await pre({ agent, messages: claimed, turn: ++turn, signal: new AbortController().signal }, () => ({ kind: 'enter' }))
      } finally { busy = false; drain() }
    })()
  }
  const agent = {
    id: 'rth-root',
    session,
    get status() { return busy ? 'running' : 'idle' },
    inbox: { nextTurn, nextStep: [], get hasPending() { return nextTurn.length > 0 }, remove: () => false },
    whenIdle: () => running,
    send(message, _t, wakeup) { nextTurn.push(message); if (wakeup) drain() },
    followup(m) { agent.send(m, 'next-turn', true) },
  }
  const flushes = []
  const effects = []
  const provided = {}
  const ctx = {
    effect: (fn) => { const d = fn(); effects.push(d) },
    on: (name, fn) => { (handlers[name] ??= []).push(fn) },
    inject: () => {},
    provide: (n, v) => { provided[n] = v },
    get: () => undefined,
    logger: { info: () => {}, warn: () => {} },
    events: { dispatch: () => [] },
    agents: { get: (id) => (id === 'rth-root' ? agent : undefined) },
    sessions: { flush: async (s) => { flushes.push(s.id); return flush(s) } },
  }
  // What a fresh read handle would show of this session.
  const readBack = async () => ({ state: 'ok', lineage: LINEAGE, rows: events.filter((e) => e.type === 'assistant/message' && e.data.message.source?.provider === 'rth-task').map((e) => ({ seq: e.seq, digest: sha(e.data.message.content.map((b) => b.text).join('')), key: e.data.message.source.model })) })
  const idle = async () => { for (let i = 0; i < 30; i++) { await tick(2); await running } }
  return { ctx, agent, events, handlers, flushes, provided, readBack, idle, dispose: () => { for (const d of effects) d?.() } }
}

function plugin(ex, dsh = fakeDsh(), journal = new TranscriptJournal()) {
  const host = fakeHost()
  const bridge = new RthBridge({ host, observer: { start() {}, stop() {} } })
  apply(dsh.ctx, {}, { host, bridge, executor: ex, journal })
  return { host, bridge, dsh, journal }
}

const taskItem = (id) => llm.createUserMessage({ content: [{ type: 'text', text: '(relatório)' }], source: { kind: 'user', rpcId: `rth-task-${id}` } })
const reportRow = (id, text = REPORT) => ({ type: 'assistant/message', data: { message: llm.createAssistantMessage({ content: [{ type: 'text', text }], source: { provider: 'rth-task', model: `rth-task-${id}` } }) } })

/** An owner executor over a scripted host answer (a false flush). */
function ownerFixture({ seqFloor = 0, receipt = null } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'rth-ff-'))
  const me = inc('L', 'p')
  const pending = new PendingStore(dir)
  const id = 'b1-t1'
  pending.write({ task_id: id, text: REPORT, result_sha256: sha(REPORT), seq_floor: seqFloor, durable_receipt: receipt, owner: me, pub_epoch: 1 })
  const updates = []
  const PUB = { delivered: 'delivered', visible_unconfirmed: 'visible_unconfirmed', delivery_failed: 'delivery_failed' }
  return { id, pending, updates, me, make: (readBack) => new DelegateExecutor({ incarnation: me, pending, readBack, postTask: async (tid, u) => { updates.push(u.kind.kind); return { status: 200, body: { record: { task_id: tid, rev: updates.length + 10, outcome: 'applied', publication: PUB[u.kind.kind] ?? 'committed' } } } } }) }
}

// ── a false or no-op flush never manufactures a durable receipt ───────────────────

test('an existing visible row, no receipt, flush false: visible_unconfirmed, never delivered; the pending file stays', async () => {
  const f = ownerFixture()
  const dsh = fakeDsh({ flush: async () => false })
  dsh.events.push({ ...reportRow(f.id), seq: 0 })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.deepEqual(f.updates, ['visible_unconfirmed'])
  assert.equal(f.pending.read(f.id).state, 'ok')
  assert.equal(f.pending.read(f.id).rec.durable_receipt, null, 'no receipt is written for bytes this turn did not append')
  assert.equal(ex.stats.delivered, 0)
})

test('an existing visible row, no receipt, a no-op flush that returns true: still visible_unconfirmed', async () => {
  const f = ownerFixture()
  const dsh = fakeDsh({ flush: async () => true })
  dsh.events.push({ ...reportRow(f.id), seq: 0 })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.deepEqual(f.updates, ['visible_unconfirmed'])
  assert.equal(f.pending.read(f.id).state, 'ok')
})

test('an existing row covered by a valid durable receipt is delivered on that receipt', async () => {
  const f = ownerFixture({ receipt: { task_id: 'b1-t1', key: 'rth-task-b1-t1', seq: 0, result_sha256: sha(REPORT), session_lineage: LINEAGE } })
  const dsh = fakeDsh({ flush: async () => false })
  dsh.events.push({ ...reportRow(f.id), seq: 0 })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.deepEqual(f.updates, ['delivered'])
  assert.equal(f.pending.read(f.id).state, 'missing')
})

test('this turn\'s own append with a flush that returns false (no durability listener): not delivered', async () => {
  const f = ownerFixture({ seqFloor: null })
  const dsh = fakeDsh({ flush: async () => false })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.equal(dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
  assert.deepEqual(f.updates, ['visible_unconfirmed'])
  assert.equal(f.pending.read(f.id).state, 'ok')
})

test('this turn\'s own append, a thrown flush: delivery_failed{flush_failed}; the pending file stays', async () => {
  const f = ownerFixture({ seqFloor: null })
  const dsh = fakeDsh({ flush: async () => { throw new Error('EIO') } })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.deepEqual(f.updates, ['delivery_failed'])
  assert.equal(f.pending.read(f.id).state, 'ok')
})

test('this turn\'s own append, flush true, a fresh read-back matching its seq: delivered', async () => {
  const f = ownerFixture({ seqFloor: null })
  const dsh = fakeDsh({ flush: async () => true })
  const ex = f.make(dsh.readBack)
  plugin(ex, dsh)
  dsh.agent.followup(taskItem(f.id))
  await dsh.idle()
  assert.deepEqual(f.updates, ['delivered'])
  assert.equal(f.pending.read(f.id).state, 'missing')
})

// ── recovery runs from the plugin's own bootstrap and reconnect, not a test helper ─

/** A committed publication owned by L1 whose launcher exited; a successor plugin as L2. */
async function restart(t, opts = {}) {
  const { setup, rows = 'dsh', flush } = opts
  const L = await ledger(t)
  const dir = mkdtempSync(join(tmpdir(), 'rth-rs-'))
  const r = await rig(t, { L, dir })
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const dsh = fakeDsh({ flush })
  await setup?.({ ...r, id: a.task_id, dsh })
  await L.op({ op: 'register_launch', launch_id: 'L1', secret_sha256: sha('s1') })
  const exit = await L.op({ op: 'exit_receipt', launch_id: 'L1', launch_proof: 's1', code: 0 })
  await L.op({ op: 'register_launch', launch_id: 'L2', secret_sha256: sha('s2') })
  const c = clock()
  const updates = []
  let readBack = rows === 'dsh' ? dsh.readBack : rows
  const e2 = new DelegateExecutor({
    incarnation: inc('L2', 'p1'),
    pending: new PendingStore(join(dir, 'pending')),
    clock: c.now, setTimer: c.setTimer, clearTimer: c.clearTimer,
    postTask: async (_id, update) => { updates.push(update.kind.kind); const x = await L.op({ op: 'update', update, now: c.now() }); return { status: 200, body: { record: x.record } } },
    postExecutor: async (s) => L.op({ op: 'executor_status', ...s, now: c.now() }),
    readBack: (o) => readBack(o),
  })
  if (opts.skipBootstrap) return { ...r, id: a.task_id, e2, dsh, updates, exit, setReadBack: (f) => { readBack = f } }
  const p = plugin(e2, dsh)
  await e2.reconciling // the bootstrap pass apply() started
  await dsh.idle()
  return { ...r, id: a.task_id, e2, dsh, p, updates, exit, setReadBack: (f) => { readBack = f } }
}

test('restart before the floor: the successor\'s bootstrap takes over and appends once; delivered', async (t) => {
  const s = await restart(t)
  const rec = await s.get(s.id)
  assert.equal(rec.publication, 'delivered')
  assert.equal(rec.pub_epoch, 2)
  assert.deepEqual(rec.pub_owner, inc('L2', 'p1'))
  assert.equal(s.dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
  assert.equal(s.e2.d.pending.read(s.id).state, 'missing')
})

test('restart after the floor, the row absent: the successor appends once; delivered', async (t) => {
  const s = await restart(t, { setup: (r) => { r.ex.prepareAppend(r.id, 7) } })
  assert.equal((await s.get(s.id)).publication, 'delivered')
  assert.equal(s.dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
})

test('restart after the append, before the receipt: visible_unconfirmed; no second copy', async (t) => {
  const s = await restart(t, { setup: (r) => { r.ex.prepareAppend(r.id, 0); r.dsh.events.push({ ...reportRow(r.id, r.ex.d.pending.read(r.id).rec.text), seq: 0 }) } })
  assert.equal((await s.get(s.id)).publication, 'visible_unconfirmed')
  assert.equal(s.dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
  assert.equal(s.e2.d.pending.read(s.id).state, 'ok', 'the recovery copy stays')
})

test('restart after the durable receipt, before delivered: delivered on the old receipt', async (t) => {
  const s = await restart(t, {
    setup: (r) => {
      r.ex.prepareAppend(r.id, 0)
      const p = r.ex.d.pending.read(r.id).rec
      r.dsh.events.push({ ...reportRow(r.id, p.text), seq: 0 })
      r.ex.d.pending.write({ ...p, durable_receipt: { task_id: r.id, key: `rth-task-${r.id}`, seq: 0, result_sha256: p.result_sha256, session_lineage: LINEAGE } })
    },
  })
  assert.equal((await s.get(s.id)).publication, 'delivered')
  assert.equal(s.dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
})

test('restart after delivered: nothing to reconcile, nothing posted', async (t) => {
  const s = await restart(t, {
    setup: async (r) => {
      const prep = r.ex.prepareAppend(r.id, 0)
      r.dsh.events.push({ ...reportRow(r.id, prep.text), seq: 0 })
      r.rows.push({ seq: 0, digest: sha(prep.text) })
      assert.equal((await r.ex.afterFlush(r.id, { appendedSeq: 0, appendedNow: true, flushed: true })).outcome, 'delivered')
    },
  })
  assert.deepEqual(s.updates, [])
  assert.equal((await s.get(s.id)).publication, 'delivered')
})

test('an unreadable store at bootstrap stays unknown; the next observer attach reconciles', async (t) => {
  const s = await restart(t, { rows: async () => ({ state: 'error' }) })
  assert.equal((await s.get(s.id)).pub_epoch, 2, 'taken over')
  assert.equal((await s.get(s.id)).publication, 'committed', 'a failed read is not absence')
  assert.equal(s.e2.d.pending.read(s.id).state, 'ok')
  s.setReadBack(s.dsh.readBack)
  s.p.bridge.onLink('attached')
  await s.e2.reconciling
  await s.dsh.idle()
  assert.equal((await s.get(s.id)).publication, 'delivered')
})

test('a committed publication whose pending file was never written: delivery_failed{result_lost_before_commit} from the stream', async (t) => {
  const s = await restart(t, { setup: (r) => { unlinkSync(join(r.dir, 'pending', `${r.id}.json`)) } })
  assert.equal((await s.get(s.id)).publication, 'unknown', 'no file: bootstrap has nothing to enumerate')
  // The exit receipt's committed record, as the observer stream carries it.
  for (const rec of s.exit.records) s.p.bridge.ingest({ v: 2, id: 'g:9', producer: 'g', seq: 9, source: 'guest', kind: 'task_state', task_id: rec.task_id, record: rec })
  await until(async () => (await s.get(s.id)).publication !== 'unknown')
  const rec = await s.get(s.id)
  assert.equal(rec.publication, 'delivery_failed')
  assert.equal(rec.reason, 'result_lost_before_commit')
  assert.equal(s.dsh.events.filter((e) => e.type === 'assistant/message').length, 0)
})

test('every running answer lost through the resends: the committed stream record answers it; one child', async (t) => {
  const r = await rig(t, { answer: (u, _rec, http) => (u.kind.kind === 'running' ? { status: 504, body: { retry: true } } : http) })
  const a = await r.call()
  await until(() => r.ex.stats.resends >= 1)
  await r.ex.onTaskState(await r.get(a.task_id)) // the committed running record, as the observer carries it
  for (let i = 0; i < 6; i++) { r.c.advance(10_000); await tick(5) } // any resends left run out
  await ran(r.ex, a.task_id)
  assert.equal(r.counter.spawned, 1)
  assert.ok(r.ex.stats.stream_answers >= 1)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
})

test('every delivered answer lost through the resends: the stream record answers it, then the pending file goes', async (t) => {
  const r = await rig(t, { answer: (u, _rec, http) => (u.kind.kind === 'delivered' ? { status: 504, body: { retry: true } } : http) })
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const prep = r.ex.prepareAppend(a.task_id, 10)
  r.rows.push({ seq: 10, digest: sha(prep.text) })
  const out = r.ex.afterFlush(a.task_id, { appendedSeq: 10, appendedNow: true, flushed: true })
  await until(() => r.ex.stats.resends >= 1)
  await r.ex.onTaskState(await r.get(a.task_id))
  for (let i = 0; i < 6; i++) { r.c.advance(10_000); await tick(5) }
  assert.equal((await out).outcome, 'delivered')
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'missing')
})

test('the owner dies after the publish commit, before any pending write: the successor recovers it from bootstrap', async (t) => {
  // The publish_request commits in agentd, and its answer never reaches the owner: the owner
  // process is gone at that instant (its launcher's exit receipt follows).
  const L = await ledger(t)
  const dir = mkdtempSync(join(tmpdir(), 'rth-cc-'))
  const r = await rig(t, { L, dir, answer: (u, _rec, http) => (u.kind.kind === 'publish_request' ? new Promise(() => {}) : http) })
  const a = await r.call()
  await until(async () => (await r.get(a.task_id)).publication === 'committed')
  r.ex.dispose()
  await L.op({ op: 'register_launch', launch_id: 'L1', secret_sha256: sha('s1') })
  await L.op({ op: 'exit_receipt', launch_id: 'L1', launch_proof: 's1', code: 0 })
  assert.equal((await r.get(a.task_id)).publication, 'unknown')
  // No observer saw that record: the successor has only its bootstrap.
  const c = clock()
  const e2 = new DelegateExecutor({
    incarnation: inc('L2', 'p1'),
    pending: new PendingStore(join(dir, 'pending')),
    clock: c.now, setTimer: c.setTimer, clearTimer: c.clearTimer,
    postTask: async (_id, update) => ({ status: 200, body: { record: (await L.op({ op: 'update', update, now: c.now() })).record } }),
    postExecutor: async (s) => L.op({ op: 'executor_status', ...s, now: c.now() }),
    readBack: (o) => dsh.readBack(o),
  })
  const dsh = fakeDsh()
  plugin(e2, dsh)
  await e2.reconciling
  await dsh.idle()
  const rec = await r.get(a.task_id)
  assert.equal(rec.publication, 'delivered', 'recovered, not left unknown')
  assert.equal(rec.pub_epoch, 2)
  assert.equal(dsh.events.filter((e) => e.type === 'assistant/message').length, 1)
  assert.equal(e2.d.pending.read(a.task_id).state, 'missing')
})

test('a recovery intent whose publish can never commit (abandoned first) is removed by the successor', async (t) => {
  const L = await ledger(t)
  const dir = mkdtempSync(join(tmpdir(), 'rth-ci-'))
  // The publish_request is lost before agentd sees it, and the owner dies.
  const r = await rig(t, { L, dir, answer: (u, rec, http) => http })
  const post = r.ex.d.postTask
  r.ex.d.postTask = async (id, u) => (u.kind.kind === 'publish_request' ? new Promise(() => {}) : post(id, u))
  const a = await r.call()
  await until(() => r.ex.d.pending.read(a.task_id).state === 'ok')
  r.ex.dispose()
  await L.op({ op: 'register_launch', launch_id: 'L1', secret_sha256: sha('s1') })
  await L.op({ op: 'exit_receipt', launch_id: 'L1', launch_proof: 's1', code: 0 })
  // The owner abandons it before any publish committed: agentd will admit no publish_request.
  // (Without that, an executed-but-unpublished task of a dead launch keeps its intent: only the
  // assigned incarnation may publish, and the successor cannot tell a reload from a dead process.)
  await L.op({ op: 'update', update: { task_id: a.task_id, executor_id: 'rth-dsh-session', incarnation: inc('L1', 'p1'), request_id: 'op-abandon-1', kind: { kind: 'abandon' } }, now: 0 })
  const rec0 = await r.get(a.task_id)
  assert.equal(rec0.publication, 'none')
  assert.equal(rec0.abandoned, true)
  const c = clock()
  const e2 = new DelegateExecutor({
    incarnation: inc('L2', 'p1'), pending: new PendingStore(join(dir, 'pending')), clock: c.now, setTimer: c.setTimer, clearTimer: c.clearTimer,
    postTask: async (_id, update) => ({ status: 200, body: { record: (await L.op({ op: 'update', update, now: c.now() })).record } }),
    postExecutor: async (s) => L.op({ op: 'executor_status', ...s, now: c.now() }),
    readBack: async () => ({ state: 'ok', rows: [], lineage: LINEAGE }),
  })
  plugin(e2, fakeDsh())
  const out = await e2.reconciling
  assert.match(out[a.task_id], /^stale_intent/)
  assert.equal(e2.d.pending.read(a.task_id).state, 'missing')
  assert.equal((await r.get(a.task_id)).publication, 'none')
})

test('same incarnation, every delivered answer lost and no stream record: the next attach confirms it from agentd and removes the file', async (t) => {
  let lose = true
  const r = await rig(t, { answer: (u, _rec, http) => (lose && u.kind.kind === 'delivered' ? { status: 504, body: { retry: true } } : http) })
  const p = plugin(r.ex)
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const prep = r.ex.prepareAppend(a.task_id, 10)
  r.rows.push({ seq: 10, digest: sha(prep.text) })
  const out = r.ex.afterFlush(a.task_id, { appendedSeq: 10, appendedNow: true, flushed: true })
  for (let i = 0; i < 6; i++) { await until(() => r.ex.stats.resends >= Math.min(i + 1, 5) || r.ex.stats.unknown_answers >= 1); r.c.advance(10_000) }
  assert.equal((await out).outcome, 'delivered_unknown')
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'ok', 'kept: delivery is not established')
  assert.equal((await r.get(a.task_id)).publication, 'delivered', 'agentd committed it; only its answers were lost')
  lose = false
  p.bridge.onLink('attached')
  await r.ex.reconciling
  assert.equal((await r.get(a.task_id)).publication, 'delivered')
  assert.equal(r.ex.d.pending.read(a.task_id).state, 'missing')
})

test('same incarnation, every publish answer lost: the next attach learns the commit from agentd and offers the turn once', async (t) => {
  let lose = true
  const r = await rig(t, { answer: (u, _rec, http) => (lose && u.kind.kind === 'publish_request' ? { status: 504, body: { retry: true } } : http) })
  const p = plugin(r.ex)
  const a = await r.call()
  for (let i = 0; i < 6; i++) { await until(() => r.ex.stats.resends >= Math.min(i + 1, 5) || r.ex.stats.unknown_answers >= 1); r.c.advance(10_000) }
  await ran(r.ex, a.task_id)
  assert.equal((await r.get(a.task_id)).publication, 'committed')
  assert.equal(r.ex.d.pending.read(a.task_id).rec.pub_epoch, null, 'only the intent: the owner never heard the commit')
  assert.equal(r.offered.length, 0)
  lose = false
  p.bridge.onLink('attached')
  await r.ex.reconciling
  assert.equal(r.ex.d.pending.read(a.task_id).rec.pub_epoch, 1)
  assert.equal(r.offered.length, 1, 'the task turn is offered once')
  p.bridge.onLink('attached')
  await r.ex.reconciling
  assert.equal(r.offered.length, 1, 'a second attach offers nothing more')
})

// ── takeover across successive restarts ──────

/** A further successor (its own pending dir view of the same files), wired through the plugin. */
async function successorOf(t, s, launch, { readBack, postTask } = {}) {
  const c = clock()
  const updates = []
  const ex = new DelegateExecutor({
    incarnation: inc(launch, 'p1'),
    pending: new PendingStore(join(s.dir, 'pending')),
    clock: c.now, setTimer: c.setTimer, clearTimer: c.clearTimer,
    postTask: postTask ?? (async (_id, update) => { updates.push(update); return { status: 200, body: { record: (await s.L.op({ op: 'update', update, now: c.now() })).record } } }),
    postExecutor: async (st) => s.L.op({ op: 'executor_status', ...st, now: c.now() }),
    readBack: readBack ?? (async () => ({ state: 'ok', rows: [], lineage: LINEAGE })),
  })
  const offered = []
  ex.offerTaskItem = (i) => offered.push(i)
  return { ex, c, offered, updates }
}

test('L2 takes over, its read-back fails, L2 exits; L3 learns the current owner from agentd and recovers', async (t) => {
  const s = await restart(t, { rows: async () => ({ state: 'error' }) })
  const afterTakeover = await s.get(s.id)
  assert.equal(afterTakeover.pub_epoch, 2)
  assert.equal(afterTakeover.pub_owner.launch_id, 'L2')
  s.e2.dispose()
  await s.L.op({ op: 'exit_receipt', launch_id: 'L2', launch_proof: 's2', code: 0 })
  const n = await successorOf(t, s, 'L3')
  const first = await n.ex.reconcile()
  const second = await n.ex.reconcile()
  const rec = await s.get(s.id)
  assert.equal(first[s.id], 'reappend')
  assert.equal(second[s.id], undefined, 'a second pass finds its own file: nothing more')
  assert.equal(rec.publication, 'committed')
  assert.equal(rec.pub_epoch, 3)
  assert.deepEqual(rec.pub_owner, inc('L3', 'p1'))
  assert.equal(n.offered.length, 1, 'one append offered')
  const p = n.ex.d.pending.read(s.id).rec
  assert.deepEqual([p.owner, p.pub_epoch], [inc('L3', 'p1'), 3])
  assert.equal(p.result_sha256, rec.result_sha256, 'the same report')
})

test('L2\'s takeover commits but every answer is lost, L2 dies; L3 recovers from agentd\'s record', async (t) => {
  const s = await restart(t, { rows: async () => ({ state: 'ok', rows: [], lineage: LINEAGE }), skipBootstrap: true })
  // L2 as its own process: its takeover commits in agentd, and no answer ever reaches it.
  const l2 = await successorOf(t, s, 'L2', { postTask: async (_id, update) => { const x = await s.L.op({ op: 'update', update, now: 0 }); return update.kind.kind === 'publication_takeover' ? { status: 504, body: { retry: true } } : { status: 200, body: { record: x.record } } } })
  const pass = l2.ex.reconcile()
  for (let i = 0; i < 6; i++) { await until(() => l2.ex.stats.resends >= Math.min(i + 1, 5) || l2.ex.stats.unknown_answers >= 1); l2.c.advance(10_000) }
  assert.match((await pass)[s.id], /takeover_refused/)
  assert.equal((await s.get(s.id)).pub_owner.launch_id, 'L2', 'the takeover committed')
  assert.equal(l2.ex.d.pending.read(s.id).rec.owner.launch_id, 'L1', 'the file never heard')
  l2.ex.dispose()
  await s.L.op({ op: 'exit_receipt', launch_id: 'L2', launch_proof: 's2', code: 0 })
  const n = await successorOf(t, s, 'L3')
  assert.equal((await n.ex.reconcile())[s.id], 'reappend')
  const rec = await s.get(s.id)
  assert.deepEqual([rec.pub_owner.launch_id, rec.pub_epoch], ['L3', 3])
  assert.equal(n.offered.length, 1)
})

test('L2\'s takeover commits but its pending write fails; its next attach adopts its own committed takeover', async (t) => {
  const s = await restart(t, { rows: async () => ({ state: 'ok', rows: [], lineage: LINEAGE }), skipBootstrap: true })
  const l2 = await successorOf(t, s, 'L2')
  l2.ex.d.pending.faults.crashBeforeRename = true // the owner's local write after the commit fails
  await assert.rejects(l2.ex.reconcile())
  assert.equal((await s.get(s.id)).pub_owner.launch_id, 'L2')
  assert.equal(l2.ex.d.pending.read(s.id).rec.owner.launch_id, 'L1')
  assert.equal((await l2.ex.reconcile())[s.id], 'reappend', 'its own committed takeover, learned from agentd')
  assert.equal(l2.offered.length, 1)
  assert.equal((await s.get(s.id)).pub_epoch, 2, 'no second takeover by the same owner')
})

test('control: without the intermediate launch\'s exit receipt, the next takeover stays refused (the exit fence holds)', async (t) => {
  const s = await restart(t, { rows: async () => ({ state: 'error' }) })
  s.e2.dispose() // L2 is gone, but no launcher receipt was ever accepted for it
  const n = await successorOf(t, s, 'L3')
  const first = await n.ex.reconcile()
  const second = await n.ex.reconcile()
  assert.match(first[s.id], /takeover_refused/)
  assert.match(second[s.id], /takeover_refused/)
  const rec = await s.get(s.id)
  assert.deepEqual([rec.pub_owner.launch_id, rec.pub_epoch], ['L2', 2])
  assert.equal(n.offered.length, 0, 'no append without the exit proof')
  assert.equal(n.ex.d.pending.read(s.id).state, 'ok', 'the report stays recoverable')
})

// ── the Tasks service (`rthDelegation`, the Bot's projection) ─────────

test('Tasks service: rthDelegation.snapshot follows the ledger records, under one producer lifetime; a delivered row carries its report', async (t) => {
  const r = await rig(t)
  const dsh = fakeDsh()
  const p = plugin(r.ex, dsh)
  p.bridge.ingest({ v: 2, id: 'g:1', producer: 'g', seq: 1, source: 'guest', kind: 'hello', boot_id: 'boot-A' })
  const svc = dsh.provided.rthDelegation
  assert.ok(svc?.snapshot, 'the service is provided when delegation is on')
  const a = await r.call()
  await ran(r.ex, a.task_id)
  let snap = svc.snapshot()
  assert.match(snap.lifetime, /^boot-A\|L1\|p1$/)
  let row = snap.tasks.find((x) => x.task_id === a.task_id)
  const rec = await r.get(a.task_id)
  assert.deepEqual([row.rev, row.execution, row.publication, row.lifetime, row.kind, row.child_session_id], [rec.rev, 'execution_done', 'committed', snap.lifetime, 'report', rec.child_id])
  assert.equal(row.title, 'relatório pro meu agente de código')
  assert.equal(row.report_text, null, 'no text before delivery')
  assert.equal(row.accepted_at, null, 'the ledger has no timestamps; none are invented')
  const prep = r.ex.prepareAppend(a.task_id, 10)
  r.rows.push({ seq: 10, digest: sha(prep.text) })
  assert.equal((await r.ex.afterFlush(a.task_id, { appendedSeq: 10, appendedNow: true, flushed: true })).outcome, 'delivered')
  row = svc.snapshot().tasks.find((x) => x.task_id === a.task_id)
  assert.equal(row.publication, 'delivered')
  assert.equal(row.report_text, prep.text)
  // A new agentd boot is a new lifetime; the row keeps the one it was observed under.
  p.bridge.ingest({ v: 2, id: 'h:1', producer: 'h', seq: 1, source: 'guest', kind: 'hello', boot_id: 'boot-B' })
  snap = svc.snapshot()
  assert.match(snap.lifetime, /^boot-B\|/)
  assert.match(snap.tasks[0].lifetime, /^boot-A\|/)
})

test('Tasks service: live_seq renews only while this executor holds the live activation; the reconcile beat asks for the inventory', async (t) => {
  const beats = []
  const r = await rig(t, { script: { hang: true } })
  const post = r.ex.d.postExecutor
  r.ex.d.postExecutor = async (st) => { beats.push(st); return post(st) }
  const dsh = fakeDsh()
  plugin(r.ex, dsh)
  await r.ex.reconciling
  assert.equal(beats.at(-1).inventory, true, 'bootstrap asks agentd for its records')
  const a = await r.call()
  await until(() => r.counter.spawned === 1)
  const live = () => dsh.provided.rthDelegation.snapshot().tasks.find((x) => x.task_id === a.task_id).live_seq
  await r.ex.heartbeat(true)
  const l1 = live()
  await r.ex.heartbeat(true)
  assert.ok(live() > l1 && l1 > 0, 'each beat renews a running task')
  assert.equal(beats.at(-1).inventory, undefined, 'a plain beat asks for nothing')
  r.c.advance(RUN_MS + 1) // the deadline: torn down, settled
  await until(async () => (await r.get(a.task_id)).execution === 'settled_failed')
  const frozen = live()
  await r.ex.heartbeat(true)
  assert.equal(live(), frozen, 'no renewal once the activation is gone')
})

// Display authority belongs to the lifetime that observed a record, captured when the request was
// issued or the stream record arrived. The Bot's task projection: shell/vendor/omni-ui-dsh.
const { Projection } = await import('../../../shell/vendor/omni-ui-dsh/src/rth-tasks.js')
function lifetimeRig() {
  const pending = []
  let lifetime = 'boot-A|launch-1|plugin-1'
  const ex = new DelegateExecutor({ incarnation: { launch_id: 'launch-1', pid: 1, plugin_instance: 'plugin-1' }, postTask: () => new Promise((r) => pending.push(r)) })
  ex.lifetimeOf = () => lifetime
  const rec = { task_id: 'boot-A-t1', rev: 1, outcome: 'applied', execution: 'claimed', publication: 'none', args: { task: 'fictional report' } }
  const t = ex.task(rec)
  return { ex, t, rec, pending, setLifetime: (l) => { lifetime = l }, snap: () => delegationSnapshot(ex, null) }
}

test('a held boot-A answer released after boot B never becomes B\'s observation: the card stays unknown with no lease', async () => {
  const r = lifetimeRig()
  const p = new Projection()
  p.applySnapshot(r.snap(), 0)
  const request = r.ex.post(r.t, { kind: 'running' })
  r.setLifetime('boot-B|launch-1|plugin-1')
  p.applySnapshot(r.snap(), 10)
  r.pending.shift()({ status: 200, body: { record: { ...r.rec, rev: 2, execution: 'running' } } })
  await request
  assert.equal(r.t.state.rev, 2, 'the late answer still settles the operation it answers')
  p.applySnapshot(r.snap(), 20)
  const [row] = p.view(20)
  assert.equal(row.card, 'unknown', 'never revived as Writing the report')
  assert.equal(row.lease_remaining_ms, 0)
  assert.match(r.snap().tasks[0].lifetime, /^boot-A\|/, 'the row keeps the lifetime that observed it')
  r.ex.dispose()
})

test('under B, B\'s inventory shows rev 1; A\'s late rev-2 answer advances the operation, never B\'s display', async () => {
  const r = lifetimeRig()
  const request = r.ex.post(r.t, { kind: 'running' })
  r.setLifetime('boot-B|launch-1|plugin-1')
  await r.ex.onTaskState({ ...r.rec, rev: 1 }) // B's own observation of the record
  r.pending.shift()({ status: 200, body: { record: { ...r.rec, rev: 2, execution: 'running' } } })
  await request
  assert.equal(r.t.state.rev, 2, 'the operation it answered is settled')
  const row = r.snap().tasks[0]
  assert.deepEqual([row.rev, row.execution, row.lifetime], [1, 'claimed', 'boot-B|launch-1|plugin-1'], 'B shows only what B observed')
  r.ex.dispose()
})

test('control: an answer to a request issued under B is B\'s observation; a same-lifetime delayed answer is current too', async () => {
  const r = lifetimeRig()
  r.setLifetime('boot-B|launch-1|plugin-1')
  const p = new Projection()
  const request = r.ex.post(r.t, { kind: 'running' })
  r.pending.shift()({ status: 200, body: { record: { ...r.rec, rev: 2, execution: 'running' } } })
  await request
  p.applySnapshot(r.snap(), 0)
  const [row] = p.view(0)
  assert.equal(row.card, 'working')
  assert.ok(row.lease_remaining_ms > 0)
  assert.match(r.snap().tasks[0].lifetime, /^boot-B\|/)
  // Delayed, but within one lifetime: current.
  const later = r.ex.post(r.t, { kind: 'running' })
  r.pending.shift()({ status: 200, body: { record: { ...r.rec, rev: 3, execution: 'running' } } })
  await later
  assert.equal(r.snap().tasks[0].rev, 3)
  r.ex.dispose()
})

test('a stream record is the lifetime it arrived under; the current inventory recovers the card after B', async () => {
  const r = lifetimeRig()
  r.setLifetime('boot-B|launch-1|plugin-1')
  const p = new Projection()
  // agentd's inventory for B arrives on B's observer link: current.
  await r.ex.onTaskState({ ...r.rec, rev: 4, execution: 'running' })
  p.applySnapshot(r.snap(), 0)
  assert.equal(p.view(0)[0].card, 'working')
  assert.match(r.snap().tasks[0].lifetime, /^boot-B\|/)
  // An older revision, whatever lifetime, never replaces the display.
  await r.ex.onTaskState({ ...r.rec, rev: 3, execution: 'claimed' })
  assert.equal(r.snap().tasks[0].rev, 4)
  r.ex.dispose()
})

test('Tasks service: after a restart (cold rth-root) a delivered row\'s text is read once from the persisted session, digest-checked', async (t) => {
  const r = await rig(t)
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const prep = r.ex.prepareAppend(a.task_id, 7)
  r.rows.push({ seq: 7, digest: sha(prep.text) })
  assert.equal((await r.ex.afterFlush(a.task_id, { appendedSeq: 7, appendedNow: true, flushed: true })).outcome, 'delivered')
  const rec = await r.get(a.task_id)
  // A successor: no in-process result, no live rth-root; only the persisted session has the row.
  const e2 = new DelegateExecutor({ incarnation: inc('L2', 'p1'), pending: r.ex.d.pending, postTask: async () => ({ status: 200, body: {} }), postExecutor: async () => ({}) })
  const reads = []
  const dsh = fakeDsh()
  dsh.ctx.agents.get = () => undefined
  dsh.ctx.get = (n) => (n === 'sessionPersistence' ? { open: async (id, mode) => { reads.push([id, mode]); return { read: async (floor) => ({ events: [{ seq: 7, type: 'assistant/message', data: { message: llm.createAssistantMessage({ content: [{ type: 'text', text: prep.text }], source: { provider: 'rth-task', model: `rth-task-${a.task_id}` } }) } }].filter((e) => e.seq >= floor) }), close: async () => {} } } } : undefined)
  plugin(e2, dsh)
  await e2.onTaskState(rec)
  const first = dsh.provided.rthDelegation.snapshot().tasks[0]
  assert.equal(first.report_text, null, 'not yet read')
  await tick(10)
  const second = dsh.provided.rthDelegation.snapshot().tasks[0]
  assert.equal(second.report_text, prep.text)
  dsh.provided.rthDelegation.snapshot()
  assert.deepEqual(reads, [['rth-root', 'read']], 'one read, then the cache')
  // A persisted row whose bytes are not the committed report is never shown.
  const dsh3 = fakeDsh()
  dsh3.ctx.agents.get = () => undefined
  dsh3.ctx.get = (n) => (n === 'sessionPersistence' ? { open: async () => ({ read: async () => ({ events: [{ seq: 7, type: 'assistant/message', data: { message: llm.createAssistantMessage({ content: [{ type: 'text', text: 'other bytes' }], source: { provider: 'rth-task', model: `rth-task-${a.task_id}` } }) } }] }), close: async () => {} }) } : undefined)
  const e3 = new DelegateExecutor({ incarnation: inc('L3', 'p1'), pending: r.ex.d.pending, postTask: async () => ({ status: 200, body: {} }), postExecutor: async () => ({}) })
  plugin(e3, dsh3)
  await e3.onTaskState(rec)
  dsh3.provided.rthDelegation.snapshot()
  await tick(10)
  assert.equal(dsh3.provided.rthDelegation.snapshot().tasks[0].report_text, null, 'a digest mismatch shows nothing')
})

test('Tasks service: a persisted read that fails right after a relaunch is retried within seconds, not after 30 s', async (t) => {
  const r = await rig(t)
  const a = await r.call()
  await ran(r.ex, a.task_id)
  const prep = r.ex.prepareAppend(a.task_id, 7)
  r.rows.push({ seq: 7, digest: sha(prep.text) })
  assert.equal((await r.ex.afterFlush(a.task_id, { appendedSeq: 7, appendedNow: true, flushed: true })).outcome, 'delivered')
  const rec = await r.get(a.task_id)
  let opens = 0
  const dsh = fakeDsh()
  dsh.ctx.agents.get = () => undefined
  dsh.ctx.get = (n) => (n === 'sessionPersistence' ? { open: async () => { if (++opens === 1) throw new Error('session busy while it resumes'); return { read: async () => ({ events: [{ seq: 7, type: 'assistant/message', data: { message: llm.createAssistantMessage({ content: [{ type: 'text', text: prep.text }], source: { provider: 'rth-task', model: `rth-task-${a.task_id}` } }) } }] }), close: async () => {} } } } : undefined)
  const e2 = new DelegateExecutor({ incarnation: inc('L2', 'p1'), pending: r.ex.d.pending, postTask: async () => ({ status: 200, body: {} }), postExecutor: async () => ({}) })
  plugin(e2, dsh)
  await e2.onTaskState(rec)
  const snap = () => dsh.provided.rthDelegation.snapshot().tasks[0]
  snap()
  await tick(10)
  assert.match(snap().report_text_reason, /read failed: session busy/, 'the reason is reported')
  await new Promise((res) => setTimeout(res, 2_100))
  snap()
  await tick(10)
  assert.equal(snap().report_text, prep.text, 'recovered within seconds of the failed read')
})

test('Tasks service: with delegation off there is no rthDelegation service', () => {
  const dsh = fakeDsh()
  const host = fakeHost()
  apply(dsh.ctx, {}, { host, bridge: new RthBridge({ host, observer: { start() {}, stop() {} } }), journal: new TranscriptJournal() })
  assert.equal(dsh.provided.rthDelegation, undefined)
})

// ── repeated reply envelopes never alter the report transcript ─────────────────────

const env = (producer, seq, kind, rest) => ({ v: 2, id: `${producer}:${seq}`, producer, seq, source: 'guest', kind, ...rest })
const snapOf = (journal) => buildSnapshot(journal.freeze(), { taskId: 't', recipient: { provider: 'fake', model: 'fake' } })

function feed(bridge, envs) { for (const e of envs) bridge.ingest(e) }

test('a reply delta delivered twice through the bridge: the report input is byte-identical to one delivery', async () => {
  const reply = [env('guest-B', 1, 'response_started', { response_id: 'r1' }), env('guest-B', 2, 'agent_delta', { response_id: 'r1', text: 'Hello ' }), env('guest-B', 3, 'agent_done', { response_id: 'r1', status: 'completed' })]
  const once = plugin(stubEx())
  feed(once.bridge, reply)
  const twice = plugin(stubEx())
  feed(twice.bridge, [reply[0], reply[1], reply[1], reply[2], reply[2]])
  twice.journal.startedAt = once.journal.startedAt // the same observer start; only the deliveries differ
  const a = snapOf(once.journal)
  const b = snapOf(twice.journal)
  assert.equal(b.items[0].text, 'Hello')
  assert.equal(b.items[0].revision, 1)
  assert.equal(b.sha256, a.sha256)
})

test('repeated utterance parts and a replayed older envelope never mutate the journal', async () => {
  const p = plugin(stubEx())
  const u0 = env('guest-B', 1, 'input_transcript', { utterance_id: 'u1', utterance_seq: 0, text: 'manda ', final: false })
  const u1 = env('guest-B', 2, 'input_transcript', { utterance_id: 'u1', utterance_seq: 1, text: 'o relatório', final: true })
  feed(p.bridge, [u0, u1, u0, u1, u0])
  const s = snapOf(p.journal)
  assert.deepEqual(s.items.map((i) => [i.id, i.text, i.revision]), [['u1', 'manda o relatório', 2]])
})

test('a conflicting repeat (same producer and seq, other bytes) is omitted and listed, never chosen', async () => {
  const p = plugin(stubEx())
  feed(p.bridge, [env('guest-B', 1, 'response_started', { response_id: 'r1' }), env('guest-B', 2, 'agent_delta', { response_id: 'r1', text: 'Hello ' }), env('guest-B', 2, 'agent_delta', { response_id: 'r1', text: 'Bye ' }), env('guest-B', 3, 'agent_done', { response_id: 'r1', status: 'completed' })])
  const s = snapOf(p.journal)
  assert.equal(s.items.length, 0)
  assert.deepEqual(s.omissions.conflicted, [{ kind: 'reply', producer: 'guest-B', id: 'r1' }])
})

test('a repeat that changes the target reply invalidates BOTH replies', async () => {
  const p = plugin(stubEx())
  feed(p.bridge, [
    env('guest-B', 1, 'response_started', { response_id: 'a' }),
    env('guest-B', 2, 'agent_delta', { response_id: 'a', text: 'selected content ' }),
    env('guest-B', 3, 'response_started', { response_id: 'b' }),
    env('guest-B', 2, 'agent_delta', { response_id: 'b', text: 'other content ' }),
    env('guest-B', 4, 'agent_done', { response_id: 'a', status: 'completed' }),
    env('guest-B', 5, 'agent_done', { response_id: 'b', status: 'completed' }),
  ])
  const s = snapOf(p.journal)
  assert.deepEqual(s.items, [], 'neither side of the disputed identity reaches the worker')
  assert.deepEqual(s.omissions.conflicted.map((o) => o.id).sort(), ['a', 'b'])
})

test('a repeat that changes the kind invalidates the original item, and a target that appears later', async () => {
  const p = plugin(stubEx())
  feed(p.bridge, [
    env('guest-B', 1, 'response_started', { response_id: 'a' }),
    env('guest-B', 2, 'agent_delta', { response_id: 'a', text: 'selected content ' }),
    // seq 2 again, now an utterance part of an utterance not seen yet
    env('guest-B', 2, 'input_transcript', { utterance_id: 'u9', utterance_seq: 0, text: 'said', final: true }),
    env('guest-B', 3, 'agent_done', { response_id: 'a', status: 'completed' }),
    env('guest-B', 4, 'input_transcript', { utterance_id: 'u9', utterance_seq: 1, text: ' later', final: true }),
    env('guest-B', 5, 'input_transcript', { utterance_id: 'u1', utterance_seq: 0, text: 'unrelated', final: true }),
  ])
  const s = snapOf(p.journal)
  assert.deepEqual(s.items.map((i) => i.id), ['u1'], 'only the undisputed utterance remains')
  assert.deepEqual(s.omissions.conflicted.map((o) => `${o.kind}:${o.id}`).sort(), ['reply:a', 'utterance:u9'])
})

test('a repeat whose identity claims a non-transcript kind still invalidates the original item', async () => {
  const p = plugin(stubEx())
  feed(p.bridge, [
    env('guest-B', 1, 'response_started', { response_id: 'a' }),
    env('guest-B', 2, 'agent_delta', { response_id: 'a', text: 'selected content ' }),
    env('guest-B', 2, 'heartbeat', { uptime_ms: 5 }),
    env('guest-B', 3, 'agent_done', { response_id: 'a', status: 'completed' }),
  ])
  const s = snapOf(p.journal)
  assert.deepEqual(s.items, [])
  assert.deepEqual(s.omissions.conflicted.map((o) => o.id), ['a'])
})

test('a new producer lifetime restarts its seqs and is not a duplicate; order and no-pairing hold', async () => {
  const p = plugin(stubEx())
  feed(p.bridge, [
    env('guest-A', 1, 'input_transcript', { utterance_id: 'a-u1', utterance_seq: 0, text: 'oi', final: true }),
    env('guest-A', 2, 'response_started', { response_id: 'a-r1' }),
    env('guest-A', 3, 'agent_done', { response_id: 'a-r1', status: 'cancelled' }),
    env('guest-B', 1, 'input_transcript', { utterance_id: 'b-u1', utterance_seq: 0, text: 'de novo', final: true }),
  ])
  const s = snapOf(p.journal)
  assert.deepEqual(s.items.map((i) => [i.producer, i.id]), [['guest-A', 'a-u1'], ['guest-B', 'b-u1']])
  assert.ok(s.items.every((i) => !('answers' in i) && !('reply_to' in i)))
})

function stubEx() {
  return { stats: {}, admittedChild: () => null, prepareAppend: () => null, afterFlush: async () => ({}), onTaskState: async () => {} }
}

// ── without a registered launch id the live executor does not exist ──────────────

test('no RTH_DSH_LAUNCH_ID (registration refused, timed out or lost): no executor, no lease, no admission', async () => {
  const { makeLiveExecutor } = await import('../src/delegate/dsh-adapters.mjs')
  const saved = process.env.RTH_DSH_LAUNCH_ID
  const dir = mkdtempSync(join(tmpdir(), 'rth-ds5-'))
  const calls = []
  const host = { request: async (m, p) => { calls.push(p); return { status: 202, body: {} } } }
  const effects = []
  const ctx = { on: () => {}, effect: (fn) => effects.push(fn()), get: () => undefined, agents: { get: () => undefined } }
  const config = { delegate: { provider: 'openai', model: 'fake-mini', prices: PRICES, pendingDir: join(dir, 'p'), spendLedger: join(dir, 's.jsonl') } }
  const logs = []
  try {
    delete process.env.RTH_DSH_LAUNCH_ID
    assert.equal(makeLiveExecutor(ctx, config, host, { ROOT: 'rth-root', log: (m) => logs.push(m), offerTask: () => {}, journal: new TranscriptJournal() }), null)
    assert.match(logs.at(-1), /no registered launch id/)
    process.env.RTH_DSH_LAUNCH_ID = 'unlaunched; rm -rf'
    assert.equal(makeLiveExecutor(ctx, config, host, { ROOT: 'rth-root', log: (m) => logs.push(m), offerTask: () => {}, journal: new TranscriptJournal() }), null, 'a malformed id is not a registration')
    await tick()
    assert.deepEqual(calls, [], 'no executor_status lease was ever posted')
    assert.equal(effects.length, 0)
    process.env.RTH_DSH_LAUNCH_ID = 'b7f3c2d4-launch'
    const ex = makeLiveExecutor(ctx, config, host, { ROOT: 'rth-root', log: (m) => logs.push(m), offerTask: () => {}, journal: new TranscriptJournal() })
    assert.equal(ex.incarnation.launch_id, 'b7f3c2d4-launch')
    for (const d of effects) d?.()
    assert.equal(ex.disposed, true, 'the scope disposal fences the executor')
  } finally {
    if (saved === undefined) delete process.env.RTH_DSH_LAUNCH_ID
    else process.env.RTH_DSH_LAUNCH_ID = saved
  }
})
