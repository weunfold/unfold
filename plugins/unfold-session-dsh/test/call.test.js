// The composer's call button: what it may show comes only from the owner's receipts
// (`host.call`, `host.mic`), and a click is a pending request until a receipt answers it. In
// Unfold's instance the control endpoint has no call or mute verb, so the routes read as
// unavailable and refuse every request.
import test from 'node:test'
import assert from 'node:assert/strict'
import { LEASE_MS, PENDING_MS, callView, createCallReader, muteView, nextRead, pollMs } from '../src/call-view.js'
import { CALL_UNAVAILABLE, callStatus, fetchRoutes } from '../index.js'

const read = (state, extra = {}, mic = 'unmuted') => ({ call: { state, desired: ['starting', 'active'].includes(state) ? 'on' : 'off', reason: `${state} reason`, error: null, ...extra }, mic: { state: mic }, link: 'up', at: 1 })

test('each owner state maps to one button, and only receipts say what the call is', () => {
  assert.deepEqual(['off', 'starting', 'active', 'ending', 'error'].map((s) => [callView(read(s), null, 0).label, callView(read(s), null, 0).action]), [
    ['Start call', 'start'],
    ['Starting call…', 'end'],
    ['End call', 'end'],
    ['Ending call…', null],
    ['Start call', 'start'],
  ])
  const err = callView(read('error', { desired: 'on', error: 'microphone permission denied' }), null, 0)
  assert.equal(err.reason, 'microphone permission denied')
  assert.equal(err.tone, 'error')
  assert.equal(err.desired, 'on', 'desired and actual apart: wanted on, actually error')
  assert.equal(callView(null, null, 0).state, 'unknown')
  assert.equal(callView(null, null, 0).action, null)
  assert.equal(callView({ unavailable: 'owner status 0' }, null, 0).action, null)
  const old = callView({ call: null, mic: { state: 'unmuted' }, link: 'up' }, null, 0)
  assert.equal(old.label, 'Call unavailable', 'a host that predates calls is never shown as off')
  assert.match(callView({ ...read('off'), link: 'down' }, null, 0).title, /agent link: down/)
})

test('a click is pending until a receipt answers it, then the receipt shows; no receipt is said', () => {
  const p = { want: 'start', at: 1000 }
  const waiting = callView(read('off'), p, 1500)
  assert.equal(waiting.label, 'Start call', 'the button keeps the receipt state, never the click')
  assert.equal(waiting.action, null, 'no second request while one waits')
  assert.equal(waiting.busy, true)
  assert.equal(waiting.pending, 'waiting')
  assert.equal(callView(read('starting'), p, 1500).pending, null, 'desired on answers a start')
  const late = callView(read('off'), p, 1000 + PENDING_MS + 1)
  assert.equal(late.pending, 'no_receipt')
  assert.equal(late.action, 'start', 'after the timeout the button works again')
  assert.match(late.title, /no receipt/)
  assert.equal(callView(read('active'), { want: 'end', at: 0 }, 10).pending, 'waiting', 'an end is answered by desired off only')
})

test('mute shows beside an active call only, from the applied fence', () => {
  assert.equal(callView(read('off'), null, 0).mute, null)
  assert.equal(callView(read('starting'), null, 0).mute, null)
  assert.deepEqual([callView(read('active'), null, 0).mute.label, callView(read('active'), null, 0).mute.action], ['Mute', true])
  assert.deepEqual([callView(read('active', {}, 'muted'), null, 0).mute.label, callView(read('active', {}, 'muted'), null, 0).mute.action], ['Unmute', false])
  assert.equal(muteView({ state: 'muting' }).action, null, 'a request not applied yet is never shown as muted')
  assert.equal(muteView({ state: 'muting' }).muted, false)
  assert.equal(muteView(undefined), null)
  assert.equal(pollMs(callView(read('starting'), null, 0)), 600)
  assert.equal(pollMs(callView(read('active'), null, 0)), 3000)
  assert.equal(pollMs(callView(read('off'), null, 0)), 5000)
})

const route = (path) => fetchRoutes({}, '/api/unfold-session').find((r) => r.path === path)
const post = (body) => new Request('http://x/', { method: 'POST', body: typeof body === 'string' ? body : JSON.stringify(body) })

test('the call route reads as unavailable and refuses a request: the endpoint has no call verb', async () => {
  const r = route('/api/unfold-session/call')
  assert.deepEqual(r.methods, ['GET', 'POST'])
  const got = await (await r.fetch(new Request('http://x/'))).json()
  assert.equal(got.unavailable, CALL_UNAVAILABLE)
  assert.equal(got.call, undefined, 'no call state is claimed')
  for (const body of [{ call: 'start' }, { call: 'dial' }, 'not json']) assert.equal((await r.fetch(post(body))).status, 503)
  assert.deepEqual(Object.keys(callStatus()).sort(), ['at', 'unavailable'])
})

test('the mute route refuses every request: the microphone is the app\'s own', async () => {
  const r = route('/api/unfold-session/mute')
  assert.deepEqual(r.methods, ['POST'])
  for (const body of [{ muted: true }, { muted: 'yes' }]) assert.equal((await r.fetch(post(body))).status, 503)
})

test('an unavailable read shows the call as unknown, with no button that claims a state', () => {
  const v = callView({ ...callStatus(), receivedAt: 0 }, null, 0)
  assert.deepEqual([v.state, v.label, v.action, v.mute], ['unknown', 'Call unavailable', null, null])
})

// No stale authority while a read hangs or answers late.

const ok = (body) => ({ ok: true, status: 200, json: async () => body })
const activeMuted = { call: { state: 'active', desired: 'on', reason: 'r' }, mic: { state: 'muted' }, link: 'up' }

test('a receipt is evidence only for its local lease: then the call and mute are unknown', () => {
  const v = callView({ ...activeMuted, receivedAt: 0 }, null, LEASE_MS)
  assert.deepEqual([v.label, v.mute?.label], ['End call', 'Unmute'])
  const stale = callView({ ...activeMuted, receivedAt: 0 }, null, LEASE_MS + 1)
  assert.equal(stale.state, 'unknown')
  assert.equal(stale.mute, null, 'no mute claim from a stale read')
  assert.equal(stale.action, 'end', 'End stays offered as a best-effort escape')
  assert.equal(stale.label, 'End call (state unknown)')
  assert.match(stale.title, /no fresh receipt/)
  assert.equal(pollMs(stale), 600)
  // The server's clock (`at`) is never the lease: an hour-old read is unknown whatever `at` says.
  assert.equal(callView({ ...activeMuted, at: Date.now(), receivedAt: 0 }, null, 3_600_000).state, 'unknown')
  const offStale = callView({ call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' }, receivedAt: 0 }, null, LEASE_MS + 1)
  assert.equal(offStale.action, null, 'no Start from an unknown state')
})

test('a hung GET is aborted, and the view goes unknown while it hangs', async () => {
  let t = 0
  const reader = createCallReader({ fetch: async () => ok(activeMuted), now: () => t, timeoutMs: 50 })
  let read = nextRead(null, await reader.load())
  assert.equal(callView(read, null, t).label, 'End call')
  // The next GET never answers: it is aborted at the timeout, and is no receipt.
  const hung = createCallReader({
    fetch: (_url, { signal }) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('aborted')))),
    now: () => t,
    timeoutMs: 50,
  })
  t = LEASE_MS + 1 // the lease runs out while the read is still pending
  assert.equal(callView(read, null, t).state, 'unknown', 'unknown during the hang, before any answer')
  const got = await hung.load()
  assert.match(got.unavailable, /no answer within/)
  read = nextRead(read, got)
  const v = callView(read, null, t)
  assert.deepEqual([v.state, v.label, v.action, v.mute], ['unknown', 'End call (state unknown)', 'end', null], 'a timed-out read keeps no state, only the End escape')
  assert.match(v.title, /cannot be read: no answer within/)
})

test('a late answer from an older request never overwrites a newer view', async () => {
  const answers = []
  const reader = createCallReader({ fetch: () => new Promise((resolve) => answers.push(resolve)), now: () => 1 })
  const first = reader.load()
  const second = reader.load()
  answers[1](ok({ call: { state: 'off', desired: 'off' }, mic: { state: 'unmuted' } }))
  let read = nextRead(null, await second)
  answers[0](ok(activeMuted))
  const late = await first
  assert.equal(late.stale, true)
  read = nextRead(read, late)
  assert.equal(callView(read, null, 1).label, 'Start call', 'the newer receipt (off) stands')
  assert.equal(reader.latest().call.state, 'off')
})

test('a failed read is unknown, and the next good read recovers', async () => {
  let fail = true
  const reader = createCallReader({ fetch: async () => (fail ? { ok: false, status: 502, json: async () => ({}) } : ok(activeMuted)), now: () => 5 })
  let read = nextRead(null, await reader.load())
  assert.equal(callView(read, null, 5).label, 'Call unavailable')
  assert.equal(callView(read, null, 5).action, null, 'no escape without a good receipt that said a call was up')
  fail = false
  read = nextRead(read, await reader.load())
  assert.equal(callView(read, null, 5).label, 'End call')
  fail = true
  read = nextRead(read, await reader.load())
  assert.deepEqual([callView(read, null, 5).state, callView(read, null, 5).label, callView(read, null, 5).action], ['unknown', 'End call (state unknown)', 'end'])
})
