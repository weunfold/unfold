// The state read at client ingress: the lease is normalized at receive time on the local
// monotonic clock, and deadlines abort the underlying read.
import test from 'node:test'
import assert from 'node:assert/strict'
import { createStateIngress } from '../src/client/state-ingress.js'
import { createLeaseMemo } from '../src/client/rth-lease.js'

const task = (obs = 'L:1', remaining = 30000) => ({ task_id: 't', obs, terminal: false, card: 'working', label: 'Writing', lease_remaining_ms: remaining })

function fakeClock(start = 0) {
  let t = start
  return { now: () => t, set: v => { t = v } }
}

test('a response delayed 14 s within the cutoff keeps the 30 s observation at 30 s, not 44 s', async () => {
  const clock = fakeClock()
  const ingress = createStateIngress({ deadlineMs: 15000, now: clock.now, call: async () => { clock.set(14000); return { rthTasks: [task()] } } })
  const value = await ingress.read({})
  const memo = createLeaseMemo()
  assert.equal(memo.leased(value.rthTasks[0], 29000).card, 'working')
  assert.equal(memo.leased(value.rthTasks[0], 30001).card, 'unknown', 'expires 30 s after the request, not 30 s after receipt')
})

test('holding the roster until 60 s before the first Tasks render shows unknown', async () => {
  const clock = fakeClock()
  const ingress = createStateIngress({ deadlineMs: 15000, now: clock.now, call: async () => ({ rthTasks: [task()] }) })
  const value = await ingress.read({})
  assert.equal(createLeaseMemo().leased(value.rthTasks[0], 60000).card, 'unknown', 'a late mount cannot start a fresh lease')
})

test('a late answer is refused at acceptance even when the deadline timer never ran', async () => {
  const clock = fakeClock()
  const ingress = createStateIngress({ deadlineMs: 15000, now: clock.now, call: async () => { clock.set(20000); return { rthTasks: [task()] } } })
  await assert.rejects(ingress.read({}), /deadline/)
})

test('repeated hung reads are aborted, and a healthy read then recovers', async () => {
  const aborted = []
  let healthy = false
  const ingress = createStateIngress({
    deadlineMs: 30,
    call: (endpoint, payload, signal) => {
      if (healthy) return Promise.resolve({ rthTasks: [] })
      return new Promise(() => { signal?.addEventListener('abort', () => aborted.push(signal.reason?.message ?? 'aborted')) })
    },
  })
  for (let i = 0; i < 3; i++) await assert.rejects(ingress.read({}), /deadline/)
  assert.equal(aborted.length, 3, 'every timed-out read was aborted, not left outstanding')
  healthy = true
  assert.deepEqual(await ingress.read({}), { rthTasks: [] })
})

test('a superseding read aborts the older one, and disposal aborts the current one', async () => {
  const signals = []
  const ingress = createStateIngress({ deadlineMs: 10000, call: (e, p, signal) => { signals.push(signal); return new Promise(() => {}) } })
  const first = ingress.read({})
  const second = ingress.read({})
  await assert.rejects(first, /superseded/)
  assert.equal(signals[0].aborted, true)
  ingress.dispose()
  await assert.rejects(second, /disposed/)
  assert.equal(signals[1].aborted, true)
})

test('server expiry, old-observation non-revival and renewal still hold', () => {
  const memo = createLeaseMemo()
  const expired = { ...task('L:1', 0), deadline_mono: 0 }
  assert.equal(memo.leased(expired, 1).card, 'unknown')
  assert.equal(memo.leased({ ...task('L:1'), deadline_mono: 99999 }, 2).card, 'unknown', 'the same observation never revives')
  assert.equal(memo.leased({ ...task('L:2'), deadline_mono: 40000 }, 3).card, 'working', 'a new observation renews')
})
