// The delegate task projection and its exhaustive mapping.
import test from 'node:test'
import assert from 'node:assert/strict'
import { CARD, EXECUTION, HANDOFF, LEASE_MS, PUBLICATION, Projection, isProjectedId, mapHandoff, mapReport, projectedId } from '../src/rth-tasks.js'

test('every execution x publication combination maps to a known card, and unknown is never stopped', () => {
  for (const e of EXECUTION) for (const p of PUBLICATION) {
    const m = mapReport(e, p)
    assert.ok(CARD.includes(m.card), `${e}/${p}`)
    if (m.label.toLowerCase().includes('unknown')) assert.equal(m.card, 'unknown', `${e}/${p}`)
    if (m.card === 'stopped') assert.equal(e, 'cancelled', 'only a real cancellation is stopped')
    if (m.card === 'done') assert.equal(p, 'delivered', 'done needs the delivered receipt')
  }
  assert.equal(mapReport('mystery', 'none').card, 'unknown', 'unrecognized never falls back to working')
  assert.equal(mapReport('running', 'mystery').card, 'unknown')
})

test('precedence: publication outranks execution', () => {
  assert.equal(mapReport('execution_done', 'none').card, 'working', 'worker done, posting pending')
  assert.equal(mapReport('execution_done', 'unknown').card, 'unknown', 'lost delivery acknowledgement')
  assert.equal(mapReport('settled_exited', 'visible_unconfirmed').card, 'unknown')
  assert.equal(mapReport('settled_exited', 'delivered').card, 'done')
  assert.equal(mapReport('teardown_unknown', 'none').card, 'unknown')
  assert.equal(mapReport('cancelled', 'none').card, 'stopped')
})

test('handoff cards have their own mapping', () => {
  for (const s of HANDOFF) assert.ok(CARD.includes(mapHandoff(s).card))
  assert.equal(mapHandoff('done').card, 'done')
  assert.equal(mapHandoff('other').card, 'unknown')
})

test('stale and reordered updates never move a task backwards', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'L1', tasks: [{ task_id: 't1', rev: 2, lifetime: 'L1', execution: 'running', publication: 'none' }] }, 1000)
  p.applySnapshot({ lifetime: 'L1', tasks: [{ task_id: 't1', rev: 1, lifetime: 'L1', execution: 'accepted', publication: 'none' }] }, 1001)
  assert.equal(p.view(1500)[0].card, 'working')
  assert.equal(p.view(1500)[0].detail.rev, 2)
})

test('an active task without a renewal expires to unknown; a delivered one stays historical', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'L1', tasks: [
    { task_id: 't1', rev: 1, lifetime: 'L1', execution: 'running', publication: 'none' },
    { task_id: 't2', rev: 3, lifetime: 'L1', execution: 'settled_exited', publication: 'delivered', delivered_at: 5, report_text: 'R' },
  ] }, 0)
  const rows = Object.fromEntries(p.view(LEASE_MS + 1).map(r => [r.task_id, r]))
  assert.equal(rows.t1.card, 'unknown')
  assert.equal(rows.t1.ended_at, null, 'unknown never fabricates an end time')
  assert.equal(rows.t1.started_at, null)
  assert.equal(rows.t2.card, 'done')
  assert.equal(rows.t2.report_text, 'R')
})

test('a new producer lifetime turns the old lifetime\'s active rows unknown', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'L1', tasks: [{ task_id: 't1', rev: 5, lifetime: 'L1', execution: 'running', publication: 'none' }] }, 10)
  p.applySnapshot({ lifetime: 'L2', tasks: [{ task_id: 't2', rev: 1, lifetime: 'L2', execution: 'accepted', publication: 'none' }] }, 11)
  const rows = Object.fromEntries(p.view(12).map(r => [r.task_id, r]))
  assert.equal(rows.t1.card, 'unknown')
  assert.equal(rows.t2.card, 'working')
})

test('projected ids are namespaced and can never collide with a session id', () => {
  assert.equal(projectedId('abc'), 'rth-task:abc')
  assert.ok(isProjectedId('rth-task:abc'))
  assert.ok(!isProjectedId('rth-root'))
})

test('the channel view merges projected tasks without touching Bot state', async () => {
  const { install } = await import('../src/host/channel.js')
  const state = { revision: 1, mainBotIds: [], bots: {}, rooms: {}, agents: {}, tasks: {}, exchangeTurns: {}, questions: {}, prefs: {} }
  const records = [{ task_id: 't9', rev: 1, lifetime: 'L', execution: 'running', publication: 'none', updated_at: Date.now(), title: 'Report' }]
  const rt = { ctx: { get: name => (name === 'rthDelegation' ? { snapshot: () => ({ lifetime: 'L', tasks: records }) } : undefined), on() {}, effect() {} }, state }
  try { install(rt) } catch { /* the rest of the channel install needs a full runtime; the projection is set first */ }
  const view = rt.rthTasksView()
  assert.equal(view[0].id, 'rth-task:t9')
  assert.deepEqual(state.agents, {}, 'no Bot membership')
  assert.deepEqual(state.tasks, {}, 'no Bot task record, so resume never sees it')
})

// The producer's explicit liveness renewal. A long run keeps one ledger rev; only a newer
// `live_seq` (the executor still holds the live activation) renews it, and an unchanged record
// never does.
test('a newer live_seq at the same rev renews freshness; a repeated record does not', () => {
  const p = new Projection()
  const rec = { task_id: 't', rev: 4, lifetime: 'L', execution: 'running', publication: 'none' }
  assert.equal(p.applySnapshot({ lifetime: 'L', tasks: [{ ...rec, live_seq: 1 }] }, 0), true)
  const first = p.view(0)[0]
  p.applySnapshot({ lifetime: 'L', tasks: [{ ...rec, live_seq: 1 }] }, LEASE_MS - 1) // the same record: no renewal
  assert.equal(p.view(LEASE_MS + 1)[0].card, 'unknown', 'an unchanged record expires')
  p.applySnapshot({ lifetime: 'L', tasks: [{ ...rec, live_seq: 2 }] }, LEASE_MS + 2)
  const renewed = p.view(LEASE_MS + 3)[0]
  assert.equal(renewed.card, 'working', 'the executor renewed it explicitly')
  assert.notEqual(renewed.obs, first.obs, 'a renewal is a new observation for the browser lease')
  p.applySnapshot({ lifetime: 'L', tasks: [{ ...rec, rev: 5, live_seq: 0 }] }, LEASE_MS + 4)
  assert.equal(p.view(LEASE_MS + 5)[0].detail.rev, 5, 'a newer rev wins whatever its live_seq')
  p.applySnapshot({ lifetime: 'L', tasks: [{ ...rec, rev: 4, live_seq: 9 }] }, LEASE_MS + 6)
  assert.equal(p.view(LEASE_MS + 7)[0].detail.rev, 5, 'an older rev never returns, however live')
})
