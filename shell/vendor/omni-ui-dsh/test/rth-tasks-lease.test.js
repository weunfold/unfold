// Delegate task authority and freshness: producer lifetimes, the server lease and the browser lease.
import test from 'node:test'
import assert from 'node:assert/strict'
import { LEASE_MS, Projection } from '../src/rth-tasks.js'
import { createLeaseMemo } from '../src/client/rth-lease.js'

const row = (rev, lifetime, execution = 'running', publication = 'none', updated_at = 0) => ({ task_id: 'task', rev, lifetime, execution, publication, updated_at, report_text: 'the durable report', delivered_at: 1000 })

test('a retired, missing or invalid snapshot is rejected before any row changes', () => {
  const p = new Projection()
  assert.equal(p.applySnapshot({ lifetime: 'old', tasks: [row(1, 'old')] }, 0), true)
  assert.equal(p.applySnapshot({ lifetime: 'new', tasks: [row(5, 'new', 'execution_done', 'delivered')] }, 10), true)
  assert.equal(p.view(11)[0].card, 'done')
  assert.equal(p.applySnapshot({ lifetime: 'old', tasks: [row(2, 'old')] }, 12), false, 'a retired producer is refused')
  const after = p.view(13)[0]
  assert.equal(after.card, 'done', 'the delivered report survives a late old snapshot')
  assert.equal(after.report_text, 'the durable report')
  assert.equal(p.applySnapshot({ tasks: [row(9, undefined)] }, 14), false, 'no producer identity, no authority')
  assert.equal(p.applySnapshot({ lifetime: '', tasks: [] }, 15), false)
  assert.equal(p.view(16)[0].card, 'done')
  const fresh = new Projection()
  assert.equal(fresh.applySnapshot({ tasks: [row(1, undefined)] }, 0), false)
  assert.equal(fresh.view(1).length, 0, 'nothing shows as working without a current producer')
  // A legitimate new-current update of the same task id replaces the row.
  assert.equal(p.applySnapshot({ lifetime: 'newer', tasks: [row(1, 'newer', 'running')] }, 20), true)
  assert.equal(p.view(21)[0].card, 'working')
})

test('freshness is a local monotonic deadline; producer clocks cannot extend it', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'L', tasks: [row(1, 'L', 'running', 'none', 3_600_000)] }, 0) // a producer clock an hour ahead
  assert.equal(p.view(LEASE_MS + 1)[0].card, 'unknown', 'a future producer timestamp does not keep it working')
  // The same observation repeated is not a renewal.
  p.applySnapshot({ lifetime: 'L', tasks: [row(1, 'L', 'running', 'none', 3_600_000)] }, LEASE_MS + 2)
  assert.equal(p.view(LEASE_MS + 3)[0].card, 'unknown', 'a repeated stale snapshot does not revive it')
  // A real renewal (a newer revision) does.
  p.applySnapshot({ lifetime: 'L', tasks: [row(2, 'L')] }, LEASE_MS + 4)
  assert.equal(p.view(LEASE_MS + 5)[0].card, 'working')
})

test('in the browser an expired observation stays expired after a wall-clock rollback', () => {
  const memo = createLeaseMemo()
  const task = { task_id: 'task', obs: 'o1', terminal: false, card: 'working', label: 'Writing', lease_remaining_ms: LEASE_MS, deadline_mono: LEASE_MS }
  assert.equal(memo.leased(task, 0).card, 'working')
  assert.equal(memo.leased(task, LEASE_MS + 1).card, 'unknown')
  assert.equal(memo.leased(task, 1).card, 'unknown', 'a clock moving back does not revive the same observation')
  assert.equal(memo.leased({ ...task, obs: 'o2', deadline_mono: 2 + LEASE_MS }, 2).card, 'working', 'a new observation does')
})
