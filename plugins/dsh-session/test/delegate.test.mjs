// Delegation: the executor against agentd's REAL ledger rules, through the
// test-only `rth-tasks-stdio` seam (crates/rth-tasks), with scripted children, a fake clock and
// fake persistence. Zero spend: no provider, no DSH child, no network, no rth-host, no agentd.
import nodeTest from 'node:test'
import { tasksStdio } from '../../../test/rth-checkout.mjs'
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DelegateExecutor } from '../src/delegate/executor.mjs'
import { PendingStore } from '../src/delegate/pending.mjs'
import { SpendLedger, worstCaseUsd, attemptBoundUsd, DEFAULTS } from '../src/delegate/budget.mjs'
import { makeLiveExecutor } from '../src/delegate/dsh-adapters.mjs'
import { buildSnapshot, workerPrompt } from '../src/delegate/snapshot.mjs'
import { TranscriptJournal } from '../src/delegate/journal.mjs'
import { validateResult } from '../src/delegate/validate.mjs'
import { canonical } from '../src/delegate/canon.mjs'

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

/** The real ledger, one process per test (killed at the end). */
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
  return { op, ready, logPath, close: () => new Promise((r) => { p.once('exit', r); p.stdin.end() }) }
}

const inc = (launch, pi) => ({ launch_id: launch, pid: 4242, plugin_instance: pi })

/** A fake clock and timers fired by hand. */
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

/** A scripted child: steps with usage, then a result; or hangs until disposed. */
function child(script) {
  return ({ hooks }) => {
    let settle
    const result = new Promise((r) => { settle = r })
    let disposed = false
    ;(async () => {
      for (const usage of script.steps ?? [{ inputTokens: 9000, outputTokens: 600 }]) {
        const gate = hooks.beforeStep()
        if (!gate.ok) return settle({ turnEnd: 'blocked', stopReason: null, text: '' })
        await null
        hooks.afterStep(usage, 40_000)
        if (disposed) return
      }
      if (script.hang) return
      settle(script.out ?? { turnEnd: 'completed', stopReason: 'stop', interrupted: false, text: REPORT, toolAttempts: 0 })
    })()
    return { result, dispose: () => { disposed = true; if (!script.stubborn) settle({ turnEnd: 'aborted', stopReason: null, text: '' }) } }
  }
}

/** One executor wired to the real ledger. Records agentd would publish go to onTaskState. */
async function rig(t, { script = {}, me = inc('L1', 'p1'), L, dir = mkdtempSync(join(tmpdir(), 'rth-dx-')), rows = null, limits, accept } = {}) {
  L ??= await ledger(t)
  const c = clock()
  const offered = []
  const persisted = rows ?? []
  let ex
  const stream = async (rec) => { if (rec) await ex.onTaskState(rec) }
  ex = new DelegateExecutor({
    incarnation: me,
    recipient: { provider: 'openai', model: 'fake-mini' },
    limits,
    accept,
    pending: new PendingStore(join(dir, 'pending')),
    spend: new SpendLedger(join(dir, 'spend.jsonl'), { prices: PRICES, now: () => Date.UTC(2026, 9, 23, 12) }),
    clock: c.now,
    setTimer: c.setTimer,
    clearTimer: c.clearTimer,
    postTask: async (taskId, update) => {
      const r = await L.op({ op: 'update', update, now: c.now() })
      return { status: 200, body: { record: r.record } }
    },
    postExecutor: async (s) => L.op({ op: 'executor_status', ...s, now: c.now() }),
    snapshotSource: () => {
      const j = new TranscriptJournal()
      j.ingest({ producer: 'guest-p1', seq: 1, kind: 'input_transcript', utterance_id: 'p1-u1', utterance_seq: 0, text: 'manda um relatório pro meu agente', final: true })
      return j.freeze()
    },
    offerTaskItem: (i) => offered.push(i),
    spawnChild: child(script),
    readBack: async () => (rows === 'error' ? { state: 'error' } : { state: 'ok', rows: persisted, lineage: 'rth-root@2026-10-22T20:30Z' }),
  })
  await ex.heartbeat(true)
  const call = async (id = 'c1') => {
    const a = await L.op({ op: 'tool_call', name: 'delegate', call_key: { instance_id: 'inst', provider_generation: 1, call_id: id }, args: { purpose: 'report', task: 'relatório pro meu agente de código', audience: 'coding_agent' }, now: c.now() })
    if (!a.task_id) return a
    const s = await L.op({ op: 'output_sent', task_id: a.task_id, now: c.now() })
    await stream(s.record)
    await new Promise((r) => setImmediate(r))
    return a
  }
  const get = async (id) => (await L.op({ op: 'get', task_id: id })).record
  return { L, c, ex, offered, persisted, call, get, stream, dir }
}

// ── the happy path, end to end ───────────────────────────────────────────────────────────

test('report: accepted → claim → running → returned → committed → append → flush → read-back → delivered', async (t) => {
  const r = await rig(t)
  const a = await r.call()
  assert.equal(a.output.status, 'accepted')
  assert.match(a.output.say, /Do not say you are writing it/)
  const id = a.task_id
  await r.ex.tasks.get(id).done
  let s = await r.get(id)
  assert.equal(s.execution, 'execution_done')
  assert.equal(s.publication, 'committed')
  assert.deepEqual(s.pub_owner, inc('L1', 'p1'))
  assert.equal(r.offered.length, 1, 'one task turn offered to rth-root')
  // The pending file exists before any append, with a null floor.
  const pend = r.ex.d.pending.read(id)
  assert.equal(pend.state, 'ok')
  assert.equal(pend.rec.seq_floor, null)
  // Inside the held turn: the floor is persisted before the append.
  const prep = r.ex.prepareAppend(id, 10)
  assert.equal(r.ex.d.pending.read(id).rec.seq_floor, 10)
  r.persisted.push({ seq: 10, digest: sha(prep.text) })
  // The owner's chain: this turn appended seq 10, a durability listener answered the flush.
  const out = await r.ex.afterFlush(id, { appendedSeq: 10, appendedNow: true, flushed: true })
  assert.equal(out.outcome, 'delivered')
  s = await r.get(id)
  assert.equal(s.publication, 'delivered')
  assert.equal(r.ex.d.pending.read(id).state, 'missing', 'unlinked after delivered')
  assert.ok(s.input_sha256, 'the frozen input digest is on the ledger')
  // The worker's slot is free; the next call is accepted.
  assert.equal((await r.call('c2')).output.status, 'accepted')
})

test('invalid output fails and never publishes', async (t) => {
  const r = await rig(t, { script: { out: { turnEnd: 'completed', stopReason: 'max-tokens', text: REPORT } } })
  const a = await r.call()
  await r.ex.tasks.get(a.task_id).done
  const s = await r.get(a.task_id)
  assert.equal(s.execution, 'settled_failed')
  assert.match(s.reason, /invalid_output:finish_max-tokens/)
  assert.equal(s.publication, 'none')
  assert.equal(r.offered.length, 0)
})

// ── budgets (soft) ──────────────────────────────────────────────────────────────────

test('soft budget: a step over US$0.05 cancels at the step boundary; missing usage is charged at the bound', async (t) => {
  const r = await rig(t, { script: { steps: [{ inputTokens: 100_000, outputTokens: 30_000 }, null], hang: true } })
  const a = await r.call()
  await new Promise((res) => setTimeout(res, 30))
  const s = await r.get(a.task_id)
  assert.equal(s.execution, 'settled_failed')
  assert.equal(s.reason, 'budget:task')
  const rows = readFileSync(join(r.dir, 'spend.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l))
  assert.ok(rows[0].usd >= 0.05 && rows[0].estimated === false)
  // The stated worst case is finite and computable (3 steps × 2 attempts × bound).
  const w = worstCaseUsd(40_000, PRICES)
  assert.equal(w, 3 * 2 * attemptBoundUsd(40_000, PRICES))
})

test('soft budget: a fourth step is refused (steps cap 3)', async (t) => {
  const r = await rig(t, { script: { steps: [{ inputTokens: 1, outputTokens: 1 }, { inputTokens: 1, outputTokens: 1 }, { inputTokens: 1, outputTokens: 1 }, { inputTokens: 1, outputTokens: 1 }] } })
  const a = await r.call()
  await r.ex.tasks.get(a.task_id).done
  assert.match((await r.get(a.task_id)).reason, /invalid_output:turn_blocked/)
})

// ── acceptance-run limits: one zero-tool request, ≤2000 output tokens ──

const A2 = { maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000 }

test('the everyday defaults are 3 steps, 2 attempts, 4000; an acceptance run sets its own', () => {
  assert.deepEqual([DEFAULTS.maxSteps, DEFAULTS.attemptsPerStep, DEFAULTS.maxOutputTokens], [3, 2, 4000])
  assert.equal(worstCaseUsd(40_000, PRICES, { ...DEFAULTS, ...A2 }), attemptBoundUsd(40_000, PRICES, 2000), 'A2: one attempt of one step')
})

test('with the acceptance-run limits, a second step is refused and is never charged (steps cap 1)', async (t) => {
  const r = await rig(t, { limits: A2, script: { steps: [{ inputTokens: 1, outputTokens: 1 }, { inputTokens: 1, outputTokens: 1 }] } })
  const a = await r.call()
  await r.ex.tasks.get(a.task_id).done
  assert.match((await r.get(a.task_id)).reason, /invalid_output:turn_blocked/)
  const rows = readFileSync(join(r.dir, 'spend.jsonl'), 'utf8').trim().split('\n')
  assert.equal(rows.length, 1, 'one step ran and was charged; the second never started')
})

test('the live worker request asks for the run\'s limit, not a literal; reasoning only when configured', async (t) => {
  const seen = []
  const ctx = {
    get: (n) => (n === 'subagents' ? { startActivation: (o) => { seen.push(o); return { result: new Promise(() => {}), dispose() {} } } } : undefined),
    agents: { get: () => ({ id: 'rth-root' }) },
    on() {},
    effect() {},
  }
  const prev = process.env.RTH_DSH_LAUNCH_ID
  process.env.RTH_DSH_LAUNCH_ID = 'L-a2r1'
  t.after(() => { if (prev === undefined) delete process.env.RTH_DSH_LAUNCH_ID; else process.env.RTH_DSH_LAUNCH_ID = prev })
  const launch = async (extra) => {
    const dir = mkdtempSync(join(tmpdir(), 'rth-dl-'))
    const config = { delegate: { provider: 'openai', model: 'fake-mini', prices: PRICES, pendingDir: join(dir, 'pending'), spendLedger: join(dir, 'spend.jsonl'), ...extra } }
    const ex = makeLiveExecutor(ctx, config, { request: async () => ({ status: 200, body: {} }) }, { ROOT: 'rth-root', log() {}, offerTask() {}, journal: { freeze: () => new TranscriptJournal().freeze() } })
    ex.d.spawnChild({ childId: `c-${seen.length}`, prompt: 'p', signal: new AbortController().signal, hooks: {} })
    await new Promise((r) => setImmediate(r))
    return { ex, o: seen.at(-1).request.agentOptions }
  }
  const everyday = await launch({})
  assert.equal(everyday.o.maxTokens, 4000, 'the everyday default, from the limits')
  assert.ok(!('reasoningEffort' in everyday.o), 'no reasoning effort forced on everyday reports')
  const a2 = await launch({ ...A2, reasoningEffort: 'off' })
  assert.equal(a2.o.maxTokens, 2000)
  assert.equal(a2.o.maxTokens, a2.ex.limits.maxOutputTokens)
  assert.deepEqual([a2.ex.limits.maxSteps, a2.ex.limits.attemptsPerStep], [1, 1])
  assert.equal(a2.o.reasoningEffort, 'off')
  assert.equal(seen.at(-1).request.agentPreset, 'rth-delegate')
})

// ── deadline and teardown ────────────────────────────────────────────────────────

test('deadline: the task timer disposes a hanging child; teardown evidence settles it', async (t) => {
  const r = await rig(t, { script: { steps: [], hang: true } })
  const a = await r.call()
  await new Promise((res) => setTimeout(res, 10))
  assert.equal((await r.get(a.task_id)).execution, 'running')
  r.c.advance(90_001)
  await new Promise((res) => setTimeout(res, 20))
  const s = await r.get(a.task_id)
  assert.equal(s.execution, 'settled_failed')
  assert.equal(s.reason, 'deadline')
})

test('noncooperative teardown: no evidence in 10 s → teardown_unknown; the slot stays held', async (t) => {
  const r = await rig(t, { script: { steps: [], hang: true, stubborn: true } })
  const a = await r.call()
  await new Promise((res) => setTimeout(res, 10))
  // agentd's own deadline (its clock) commits the cancel; the executor sees it.
  const ticked = await r.L.op({ op: 'tick', now: r.c.now() + 90_001 })
  for (const rec of ticked.records) await r.stream(rec)
  await new Promise((res) => setTimeout(res, 10))
  r.c.advance(10_001)
  await r.ex.tasks.get(a.task_id).teardownDone
  assert.equal((await r.get(a.task_id)).execution, 'teardown_unknown')
  assert.equal((await r.call('c2')).output.reason, 'busy')
})

// ── delivery evidence ───────────────────────────────────────────

async function committed(t, opts = {}) {
  const r = await rig(t, opts)
  const a = await r.call()
  await r.ex.tasks.get(a.task_id).done
  return { ...r, id: a.task_id }
}

test('a failed flush is delivery_failed{flush_failed}; the pending report is kept', async (t) => {
  const r = await committed(t)
  r.ex.prepareAppend(r.id, 10)
  const out = await r.ex.afterFlush(r.id, { appendedSeq: 10, appendedNow: true, flushError: new Error('EIO') })
  assert.equal(out.outcome, 'flush_failed')
  const s = await r.get(r.id)
  assert.equal(s.publication, 'delivery_failed')
  assert.equal(s.reason, 'flush_failed')
})

test('flush true but no row on re-read (no writer for rth-root) is not_persisted, never delivered', async (t) => {
  const r = await committed(t, { rows: [] })
  r.ex.prepareAppend(r.id, 10)
  assert.equal((await r.ex.afterFlush(r.id, { appendedSeq: 10, appendedNow: true, flushed: true })).outcome, 'not_persisted')
  assert.equal((await r.get(r.id)).publication, 'delivery_failed')
})

test('an unreadable store is unknown, not absence: no state change, the file stays', async (t) => {
  const r = await committed(t, { rows: 'error' })
  r.ex.prepareAppend(r.id, 10)
  assert.equal((await r.ex.afterFlush(r.id, { appendedSeq: 10, appendedNow: true, flushed: true })).outcome, 'readback_error')
  assert.equal((await r.get(r.id)).publication, 'committed')
  assert.equal(r.ex.d.pending.read(r.id).state, 'ok')
})

test('visible_observed by a non-owner is recorded only; it never delivers', async (t) => {
  const r = await committed(t)
  const other = new DelegateExecutor({ ...r.ex.d, incarnation: inc('L1', 'p2') })
  await other.heartbeat(true)
  const rec = await other.observe(r.id, { seq: 10 }, (await r.get(r.id)).result_sha256)
  assert.equal(rec.outcome, 'applied')
  assert.equal((await r.get(r.id)).publication, 'committed', 'visible is not durable')
  assert.equal(r.ex.d.pending.read(r.id).state, 'ok', 'the recovery copy stays')
})

// ── successor recovery: takeover only with the owner's exit receipt ─────────────────────

async function successor(t, setup) {
  const r = await committed(t)
  const pendDir = join(r.dir, 'pending')
  await setup?.(r)
  await r.L.op({ op: 'register_launch', launch_id: 'L1', secret_sha256: sha('s1') })
  // Same process (HMR): no takeover.
  const hmr = new DelegateExecutor({ ...r.ex.d, incarnation: inc('L1', 'p2'), pending: new PendingStore(pendDir) })
  await hmr.heartbeat(true)
  const refused = await hmr.recover([r.id])
  assert.match(refused[r.id], /takeover_refused/)
  // The launcher's authenticated exit receipt for L1, then a new DSH process L2.
  assert.equal((await r.L.op({ op: 'exit_receipt', launch_id: 'L1', launch_proof: 'wrong' })).outcome, 'refused')
  await r.L.op({ op: 'exit_receipt', launch_id: 'L1', launch_proof: 's1', code: 0 })
  await r.L.op({ op: 'register_launch', launch_id: 'L2', secret_sha256: sha('s2') })
  const offered = []
  const e2 = new DelegateExecutor({ ...r.ex.d, incarnation: inc('L2', 'p1'), pending: new PendingStore(pendDir), offerTaskItem: (i) => offered.push(i) })
  await e2.heartbeat(true)
  return { ...r, e2, offered }
}

test('successor: floor null → the append never began → append once under epoch 2', async (t) => {
  const r = await successor(t)
  const out = await r.e2.recover([r.id])
  assert.equal(out[r.id], 'reappend')
  const s = await r.get(r.id)
  assert.equal(s.pub_epoch, 2)
  assert.deepEqual(s.pub_owner, inc('L2', 'p1'))
  assert.equal(s.execution, 'execution_done')
  assert.equal(r.offered.length, 1)
})

test('successor: a visible row without a durable receipt is visible_unconfirmed; no second copy', async (t) => {
  const r = await successor(t, async (r) => {
    const prep = r.ex.prepareAppend(r.id, 10)
    r.persisted.push({ seq: 10, digest: sha(prep.text) }) // written; the owner died before its receipt
  })
  const out = await r.e2.recover([r.id])
  assert.equal(out[r.id], 'visible_unconfirmed')
  assert.equal((await r.get(r.id)).publication, 'visible_unconfirmed')
  assert.equal(r.offered.length, 0, 'never a second append')
  assert.equal(r.e2.d.pending.read(r.id).state, 'ok', 'the recovery copy stays')
})

test('successor: the old owner’s durable receipt covering the row → delivered', async (t) => {
  const r = await successor(t, async (r) => {
    const prep = r.ex.prepareAppend(r.id, 10)
    r.persisted.push({ seq: 10, digest: sha(prep.text) })
    const p = r.ex.d.pending.read(r.id).rec
    r.ex.d.pending.write({ ...p, durable_receipt: { task_id: r.id, key: `rth-task-${r.id}`, seq: 10, result_sha256: p.result_sha256, session_lineage: 'rth-root@2026-10-22T20:30Z' } })
  })
  assert.equal((await r.e2.recover([r.id]))[r.id], 'delivered:old_receipt')
  assert.equal((await r.get(r.id)).publication, 'delivered')
})

test('successor: floor set, row absent, owner gone → append once', async (t) => {
  const r = await successor(t, async (r) => { r.ex.prepareAppend(r.id, 10) })
  assert.equal((await r.e2.recover([r.id]))[r.id], 'reappend')
})

// ── pending file, snapshot, validator (pure) ────────────────────────────────────────────

test('pending: atomic replace leaves the old file on a crash; malformed receipts never validate', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-p-'))
  const p = new PendingStore(dir, { faults: {} })
  const base = { task_id: 'b1-t1', text: 'x', result_sha256: sha('x'), seq_floor: null, durable_receipt: null }
  p.write(base)
  p.faults.crashBeforeRename = true
  assert.throws(() => p.write({ ...base, seq_floor: 5 }))
  assert.equal(p.read('b1-t1').rec.seq_floor, null, 'old or new, never torn')
  for (const bad of [
    { ...base, task_id: 'b1-t2' },
    { ...base, seq_floor: 5, durable_receipt: { task_id: 'b1-t1', key: 'rth-task-b1-t1', seq: 4, result_sha256: sha('x'), session_lineage: 'L' } },
    { ...base, seq_floor: 5, durable_receipt: { task_id: 'b1-t1', key: 'rth-task-b1-t1', seq: 5, result_sha256: sha('y'), session_lineage: 'L' } },
    { ...base, seq_floor: 5, durable_receipt: { task_id: 'b1-t1', key: 'rth-task-b1-t1', seq: 5, result_sha256: sha('x'), session_lineage: '' } },
  ]) {
    writeFileSync(join(dir, 'b1-t1.json'), JSON.stringify(bad))
    assert.equal(p.read('b1-t1').state, 'unreadable', JSON.stringify(bad))
  }
  writeFileSync(join(dir, 'b1-t1.json'), '{not json')
  assert.equal(p.read('b1-t1').state, 'unreadable', 'never read as a null floor')
})

test('snapshot: journal ids, chronological, finality labelled, no pairing, omissions listed', () => {
  const j = new TranscriptJournal()
  const g = (seq, kind, f) => j.ingest({ producer: 'guest-p1', seq, kind, ...f })
  g(1, 'input_transcript', { utterance_id: 'p1-u1', utterance_seq: 0, text: 'Oi, ' })
  g(2, 'input_transcript', { utterance_id: 'p1-u1', utterance_seq: 1, text: 'tudo bem?', final: true })
  g(2, 'input_transcript', { utterance_id: 'p1-u1', utterance_seq: 1, text: 'tudo bem?', final: true }) // replayed part: kept once
  g(3, 'response_started', { response_id: 'p1-v1' })
  g(4, 'agent_delta', { response_id: 'p1-v1', text: 'Tudo ótimo.' })
  g(5, 'agent_done', { response_id: 'p1-v1', status: 'completed' })
  g(6, 'input_transcript', { utterance_id: 'p1-u2', utterance_seq: 0, text: 'sem fim' }) // closed by u3
  g(7, 'input_transcript', { utterance_id: 'p1-u3', utterance_seq: 0, text: 'ainda falando' }) // open
  g(8, 'response_started', { response_id: 'p1-v2' })
  g(9, 'agent_delta', { response_id: 'p1-v2', text: 'cortado' })
  g(10, 'agent_done', { response_id: 'p1-v2', status: 'cancelled' })
  g(11, 'response_started', { response_id: 'p1-v3' }) // open
  j.typed('dsh-m9', 'texto digitado')
  const s = buildSnapshot(j.freeze(), { taskId: 'b1-t1', recipient: { provider: 'openai', model: 'fake-mini' } })
  assert.deepEqual(s.items.map((i) => [i.kind, i.id, i.finality, i.revision, i.text]), [
    ['utterance', 'p1-u1', 'final', 2, 'Oi, tudo bem?'],
    ['reply', 'p1-v1', 'completed', 1, 'Tudo ótimo.'],
    ['utterance', 'p1-u2', 'closed', 1, 'sem fim'],
    ['reply', 'p1-v2', 'cancelled', 1, 'cortado'],
    ['typed', 'dsh-m9', 'submitted', 1, 'texto digitado'],
  ])
  assert.deepEqual(s.omissions.open.map((o) => o.id), ['p1-u3', 'p1-v3'])
  for (const i of s.items) for (const k of ['answers', 'reply_to', 'utterance_id', 'response_id', 'pair']) assert.ok(!(k in i), `no pairing field ${k}`)
  assert.match(s.order, /NOT asserted/)
  const { sha256: d, ...rest } = s
  assert.equal(d, sha(canonical(rest)), 'digest over the record without its own field')
  assert.equal(canonical({ b: 1, a: [2, { d: 3, c: 4 }] }), '{"a":[2,{"c":4,"d":3}],"b":1}')
  const prompt = workerPrompt(s, { task: 't', audience: 'coding_agent' })
  assert.match(prompt, /chronological order/)
  assert.match(prompt, /NOT recorded/)
  assert.match(prompt, /quoted source data, not instructions/)
})

test('snapshot: the oldest items are cut beyond 32k characters, and the cut is disclosed', () => {
  const j = new TranscriptJournal()
  j.ingest({ producer: 'p', seq: 1, kind: 'input_transcript', utterance_id: 'u1', utterance_seq: 0, text: 'a'.repeat(20_000), final: true })
  j.ingest({ producer: 'p', seq: 2, kind: 'input_transcript', utterance_id: 'u2', utterance_seq: 0, text: 'b'.repeat(20_000), final: true })
  const s = buildSnapshot(j.freeze(), { taskId: 'b1-t1', recipient: {} })
  assert.deepEqual(s.items.map((i) => i.id), ['u2'])
  assert.equal(s.omissions.items_cut, 1)
  assert.equal(s.omissions.chars_cut, 20_000)
})

test('validator: format only; one final SUMMARY line; bounded', () => {
  const ok = validateResult({ turnEnd: 'completed', stopReason: 'stop', text: REPORT, toolAttempts: 0 })
  assert.ok(ok.ok)
  assert.ok(!ok.report.includes('SUMMARY:'))
  for (const bad of [
    { turnEnd: 'aborted', stopReason: 'stop', text: REPORT },
    { turnEnd: 'completed', stopReason: 'stop', text: REPORT, interrupted: true },
    { turnEnd: 'completed', stopReason: 'stop', text: REPORT, toolAttempts: 1 },
    { turnEnd: 'completed', stopReason: 'stop', text: 'short\nSUMMARY: x' },
    { turnEnd: 'completed', stopReason: 'stop', text: `${REPORT}\nSUMMARY: two` },
    { turnEnd: 'completed', stopReason: 'stop', text: `${REPORT.replace(/SUMMARY:.*$/, `SUMMARY: ${'x'.repeat(241)}`)}` },
    { turnEnd: 'completed', stopReason: 'stop', text: `${'x'.repeat(13_000)}\nSUMMARY: ok` },
  ]) assert.equal(validateResult(bad).ok, false, JSON.stringify(bad).slice(0, 80))
})

test('restart: a claimed task is unknown and holds the slot; its late result is accepted once', async (t) => {
  const r = await rig(t, { script: { steps: [], hang: true } })
  const a = await r.call()
  await new Promise((res) => setTimeout(res, 10))
  await r.L.close()
  const L2 = await ledger(t, r.L.logPath, 'b2')
  assert.deepEqual(L2.ready.recovered.unknown, [a.task_id])
  assert.equal((await L2.op({ op: 'get', task_id: a.task_id })).record.execution, 'unknown')
})

// ── acceptance worker spend ledger wiring ──
import { AcceptLedger, initSpend, SPEND_SCHEMA } from '../src/delegate/accept-ledger.mjs'

const A2_WORKER = { dir: 'worker', cap_pusd: 700_000_000_000, max_attempts: 1, attempt_bound_pusd: 618_000_000_000, model: 'fake-mini', context_window: 400_000, options: { maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off', routeMaxRetries: 0 }, prices: { input: 1_500_000, cached: 150_000, output: 9_000_000, source: 'test fixture', read: '2026-10-23' } }
function a2Spend() {
  const dir = join(mkdtempSync(join(tmpdir(), 'rth-a2s-')), 'spend')
  const sha = initSpend(dir, { schema: SPEND_SCHEMA, accept_id: 'acc-wire', stop: 'stop.json', layers: { worker: A2_WORKER } })
  const ledger = () => AcceptLedger.open({ control: join(dir, 'control.json'), controlSha256: sha, layer: 'worker', expect: { model: 'fake-mini', options: { maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off' } } }).ledger
  return { dir, sha, ledger }
}
const files = (d) => readdirSync(join(d, 'worker')).filter((f) => f !== 'open.json').sort()

test('acceptance ledger: a step runs only after its attempt is claimed and settles from its usage; the next step is refused by the durable allowance', async (t) => {
  const s = a2Spend()
  const r = await rig(t, { accept: s.ledger(), script: { steps: [{ inputTokens: 3000, outputTokens: 400 }, { inputTokens: 1, outputTokens: 1 }] } })
  const a = await r.call()
  await r.ex.tasks.get(a.task_id).done
  assert.match((await r.get(a.task_id)).reason, /invalid_output:turn_blocked/, 'the second step never ran')
  assert.deepEqual(files(s.dir), ['attempt-000.json', 'attempt-000.settle.json'])
  assert.equal(JSON.parse(readFileSync(join(s.dir, 'worker', 'attempt-000.settle.json'), 'utf8')).cost_pusd, 3000 * 1_500_000 + 400 * 9_000_000)
})

test('acceptance ledger: missing usage fails the task, keeps the reservation and writes the stop marker; a stopped acceptance starts no step', async (t) => {
  const s = a2Spend()
  const r = await rig(t, { accept: s.ledger(), script: { steps: [null], hang: true } })
  const a = await r.call()
  await new Promise((res) => setTimeout(res, 30))
  const got = await r.get(a.task_id)
  assert.equal(got.execution, 'settled_failed')
  assert.equal(got.reason, 'spend')
  assert.deepEqual(files(s.dir), ['attempt-000.json'])
  assert.ok(existsSync(join(s.dir, 'stop.json')))
  const s2 = a2Spend()
  writeFileSync(join(s2.dir, 'stop.json'), '{"layer":"realtime"}')
  const r2 = await rig(t, { accept: s2.ledger(), script: { steps: [{ inputTokens: 1, outputTokens: 1 }] } })
  const b = await r2.call()
  await r2.ex.tasks.get(b.task_id).done
  assert.match((await r2.get(b.task_id)).reason, /invalid_output:turn_blocked/)
  assert.deepEqual(files(s2.dir), [], 'no attempt was claimed')
})

test('acceptance ledger: the live executor stays off when the acceptance record does not match its configuration', async (t) => {
  const s = a2Spend()
  const ctx = { get: () => undefined, agents: { get: () => undefined }, on() {}, effect() {} }
  const prev = process.env.RTH_DSH_LAUNCH_ID
  process.env.RTH_DSH_LAUNCH_ID = 'L-a2s'
  t.after(() => { if (prev === undefined) delete process.env.RTH_DSH_LAUNCH_ID; else process.env.RTH_DSH_LAUNCH_ID = prev })
  const logs = []
  const mk = (extra) => makeLiveExecutor(ctx, { delegate: { provider: 'openai', model: 'fake-mini', prices: PRICES, pendingDir: join(s.dir, '..', `p${logs.length}`), spendLedger: join(s.dir, '..', `s${logs.length}.jsonl`), accept: { control: join(s.dir, 'control.json'), controlSha256: s.sha }, ...extra } }, { request: async () => ({}) }, { ROOT: 'rth-root', log: (m) => logs.push(m), offerTask() {}, journal: { freeze: () => new TranscriptJournal().freeze() } })
  assert.equal(mk({ maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 4000, reasoningEffort: 'off' }), null)
  assert.match(logs.at(-1), /delegate stays off: spend: option maxOutputTokens/)
  assert.equal(mk({ maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off', accept: { control: join(s.dir, 'control.json'), controlSha256: '0'.repeat(64) } }), null)
  assert.match(logs.at(-1), /not the pinned one/)
  const ok = mk({ maxSteps: 1, attemptsPerStep: 1, maxOutputTokens: 2000, reasoningEffort: 'off' })
  assert.ok(ok?.d.accept, 'a matching record arms the ledger')
})

test('acceptance ledger: a replayed identical usage event is one charge; a changed one fails the task', async (t) => {
  for (const changed of [false, true]) {
    const s = a2Spend()
    const accept = s.ledger()
    const r = await rig(t, { accept, script: { steps: [], hang: true } })
    const a = await r.call()
    for (let i = 0; i < 500 && !r.ex.tasks.get(a.task_id)?.hooks; i++) await new Promise((res) => setTimeout(res, 10)) // until the run's hooks exist (was a fixed 10 ms)
    const hooks = r.ex.tasks.get(a.task_id).hooks
    assert.equal(hooks.beforeStep().ok, true)
    hooks.afterStep({ inputTokens: 100, outputTokens: 10 }, 1000)
    hooks.afterStep({ inputTokens: 100, outputTokens: changed ? 11 : 10 }, 1000)
    await new Promise((res) => setTimeout(res, 30))
    assert.deepEqual(files(s.dir), ['attempt-000.json', 'attempt-000.settle.json'])
    assert.equal(existsSync(join(s.dir, 'stop.json')), changed, `changed=${changed}`)
  }
})

// ── a replayed usage event and the soft budget ──

test('a replayed worker usage event does not recharge the soft budget or cancel', async (t) => {
  const s = a2Spend()
  const r = await rig(t, { accept: s.ledger(), script: { steps: [], hang: true } })
  const a = await r.call()
  await new Promise(res => setTimeout(res, 10))
  const task = r.ex.tasks.get(a.task_id)
  const hooks = task.hooks
  assert.equal(hooks.beforeStep().ok, true)
  const usage = {inputTokens:90000,outputTokens:0}
  hooks.afterStep(usage,1000)
  assert.equal(r.ex.d.spend.taskTotal(a.task_id),0.036)
  assert.equal(task.overBudget,undefined)
  hooks.afterStep(usage,1000)
  await new Promise(res => setTimeout(res,30))
  const observed = {softTotal:r.ex.d.spend.taskTotal(a.task_id),overBudget:task.overBudget,record:await r.get(a.task_id),hardStop:existsSync(join(s.dir,'stop.json'))}
  console.log(JSON.stringify(observed))
  assert.equal(observed.softTotal,0.036,'an identical usage event must not charge the soft ledger twice')
  assert.equal(observed.overBudget,undefined,'an identical replay must not cancel the task')
})

// The whole hook is idempotent for an identical completion of the same attempt.
async function sb4(t, first, second) {
  const s = a2Spend()
  const r = await rig(t, { accept: s.ledger(), script: { steps: [], hang: true } })
  const a = await r.call()
  for (let i = 0; i < 500 && !r.ex.tasks.get(a.task_id)?.hooks; i++) await new Promise((res) => setTimeout(res, 10)) // until the run's hooks exist (was a fixed 10 ms)
  const task = r.ex.tasks.get(a.task_id)
  assert.equal(task.hooks.beforeStep().ok, true)
  task.hooks.afterStep(first, 1000)
  const afterFirst = r.ex.d.spend.taskTotal(a.task_id)
  if (second) task.hooks.afterStep(second, 1000)
  await new Promise((res) => setTimeout(res, 30))
  const rec = await r.get(a.task_id)
  return { afterFirst, soft: r.ex.d.spend.taskTotal(a.task_id), over: task.overBudget, execution: rec.execution, reason: rec.reason, publication: rec.publication, stop: existsSync(join(s.dir, 'stop.json')), settles: files(s.dir).filter((f) => f.endsWith('.settle.json')).length }
}

test('the first genuine charge still applies; an identical replay leaves the task running and publishable', async (t) => {
  const o = await sb4(t, { inputTokens: 90000, outputTokens: 0 }, { inputTokens: 90000, outputTokens: 0 })
  assert.deepEqual([o.afterFirst, o.soft, o.over, o.stop, o.settles], [0.036, 0.036, undefined, false, 1])
  assert.equal(o.execution, 'running', 'not cancelled')
  assert.equal(o.publication, 'none', 'nothing published yet, and nothing blocks it')
})

test('a genuinely over-cap first result still cancels on the soft cap', async (t) => {
  const o = await sb4(t, { inputTokens: 150000, outputTokens: 0 }, null)
  assert.deepEqual([o.over, o.execution, o.reason, o.stop], ['budget:task', 'settled_failed', 'budget:task', false])
})

test('changed replayed usage still stops on the hard ledger, and is not soft-charged', async (t) => {
  const o = await sb4(t, { inputTokens: 90000, outputTokens: 0 }, { inputTokens: 90001, outputTokens: 0 })
  assert.deepEqual([o.soft, o.over, o.stop, o.execution, o.reason], [0.036, 'spend', true, 'settled_failed', 'spend'])
})
