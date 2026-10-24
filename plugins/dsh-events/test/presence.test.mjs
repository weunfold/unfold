// The presence reducer: pure, clock supplied, honest. Every pose has a source; sources are
// reported honestly; no thinking, ever; a stale freshness lease is unknown.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.mjs'
import { IDLE, LEASE, POSES, WINCE_MS, initialPresence, nextDeadline, presence, view } from '../src/presence.mjs'

const catalog = loadCatalog()
let seq = 0
const env = (kind, fields = {}) => ({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind, ...fields })
const ev = (kind, fields) => ({ type: 'event', d: catalog.describe(env(kind, fields)) })
const heartbeat = () => ev('heartbeat', { uptime_ms: 1, attachments: 1 })
const host = (mic, { unfinished = 0, io = 'vpio' } = {}) => ({ type: 'host', status: { host: { voice_policy: { voice_io: io }, mic: { state: mic } }, commands: { unfinished } } })

/** Fold [at, input] pairs from t=0. */
function run(steps, s = initialPresence(0)) {
  for (const [at, input] of steps) s = presence(s, input, at)
  return s
}
/** Attached, heartbeat fresh, host record says nothing is open: a live, idle presence at t=100. */
const live = () => run([[0, { type: 'link', state: 'attached', detail: 'observer' }], [50, host('unmuted')], [100, heartbeat()]])

test('before any link, and on a lost link, the pose is unknown', () => {
  assert.equal(view(initialPresence(0), 0).pose, 'unknown')
  const s = presence(live(), { type: 'link', state: 'waiting', detail: 500 }, 200)
  const v = view(s, 200)
  assert.equal(v.pose, 'unknown')
  assert.match(v.because, /waiting/)
})

test('a heartbeat lease that runs out shows unknown, and the deadline is armed for it', () => {
  const s = live()
  assert.equal(view(s, 100).pose, 'idle')
  assert.equal(view(s, 100 + LEASE.heartbeatMs).pose, 'idle')
  assert.equal(view(s, 101 + LEASE.heartbeatMs).pose, 'unknown')
  assert.equal(nextDeadline(s, 200), 100 + LEASE.heartbeatMs + 1, 'the plugin wakes exactly at expiry')
  assert.equal(view(presence(s, heartbeat(), 6000), 6000).pose, 'idle', 'a fresh heartbeat restores it')
})

test('after a reattach, requests are unknown until the host record reconciles them', () => {
  let s = run([[0, { type: 'link', state: 'attached' }], [10, heartbeat()]])
  assert.equal(view(s, 20).pose, 'unknown')
  assert.match(view(s, 20).because, /requests: unknown/)
  s = presence(s, host('unmuted', { unfinished: 2 }), 30)
  assert.equal(view(s, 40).pose, 'pending', 'the owner still has unfinished commands')
  s = presence(s, host('unmuted', { unfinished: 0 }), 50)
  assert.equal(view(s, 60).pose, 'idle')
})

test('command_admitted admitted/dispatched is pending (never thinking) until its terminal', () => {
  let s = presence(live(), ev('command_admitted', { command_id: 'c1', generation: 1, status: 'admitted' }), 200)
  let v = view(s, 200)
  assert.equal(v.pose, 'pending')
  assert.match(v.because, /command c1 admitted/)
  assert.ok(v.layers.request.sources.every(Boolean), 'the pose cites its source event')
  s = presence(s, ev('command_admitted', { command_id: 'c1', generation: 1, status: 'dispatched', response_id: 'r1' }), 300)
  assert.equal(view(s, 300).pose, 'pending', 'dispatched is still pending')
  s = presence(s, ev('response_started', { response_id: 'r1', request_id: 'c1' }), 400)
  s = presence(s, ev('agent_done', { response_id: 'r1', status: 'completed' }), 500)
  assert.equal(view(s, 500).pose, 'idle', 'message_finished ends it')
  // A duplicate's recorded terminal also ends it.
  s = presence(s, ev('command_admitted', { command_id: 'c2', generation: 1, status: 'admitted' }), 600)
  s = presence(s, ev('command_admitted', { command_id: 'c2', generation: 1, status: 'interrupted', duplicate: true }), 700)
  assert.equal(view(s, 700).pose, 'idle')
})

test('no input ever yields thinking, a mood or a confidence', () => {
  const kinds = ['command_admitted', 'response_started', 'agent_delta', 'agent_audio', 'decision', 'tool_call', 'some_future_kind']
  let s = live()
  let t = 100
  for (const k of kinds) {
    s = presence(s, ev(k, { command_id: 'c', status: 'admitted', response_id: 'r' }), (t += 10))
    const v = view(s, t)
    assert.ok(POSES.includes(v.pose), v.pose)
    assert.ok(!['thinking', 'working'].includes(v.pose))
    assert.ok(!('mood' in v) && !('confidence' in v))
  }
})

test('device-rendered audio: started → speaking, finished/cleared → stops; until reported it is not observed', () => {
  let s = live()
  assert.equal(view(s, 100).layers.audio.state, 'not_observed', 'agentd does not forward host playback reports today')
  s = presence(s, ev('playback', { response_id: 'r1', epoch: 1, phase: 'started', played_samples: 0, dropped_samples: 0, sample_rate: 24000 }), 200)
  assert.equal(view(s, 200).pose, 'speaking')
  s = presence(s, ev('playback', { response_id: 'r1', epoch: 1, phase: 'cleared', reason: 'superseded', played_samples: 10, dropped_samples: 5, sample_rate: 24000 }), 300)
  assert.equal(view(s, 300).pose, 'idle')
  s = presence(s, ev('playback', { response_id: 'r2', epoch: 2, phase: 'started', played_samples: 0, dropped_samples: 0, sample_rate: 24000 }), 400)
  s = presence(s, ev('playback', { response_id: 'r2', epoch: 2, phase: 'finished', played_samples: 10, dropped_samples: 0, sample_rate: 24000 }), 500)
  assert.equal(view(s, 500).layers.audio.state, 'quiet')
})

test('device_unavailable is a fault (wince and a mark), never a yield', () => {
  let s = presence(live(), ev('playback', { response_id: 'r1', epoch: 1, phase: 'cleared', reason: 'device_unavailable', played_samples: 0, dropped_samples: 9, sample_rate: 24000 }), 200)
  assert.equal(view(s, 200).pose, 'wince')
  assert.equal(view(s, 200 + WINCE_MS).pose, 'idle', 'the wince is bounded')
  assert.equal(view(s, 200 + WINCE_MS).faults.length, 1, 'the fault mark stays')
  s = presence(s, ev('journal_fault', { detail: 'disk full' }), 300)
  assert.match(view(s, 300).because, /history could not be saved/)
  s = presence(s, ev('agent_done', { response_id: 'r9', status: 'failed' }), 400)
  assert.equal(view(s, 400).faults.length, 3)
  s = presence(s, ev('hello', { protocol: 2, agent: 'rth-agentd', version: '0', boot_id: 'b1', journal: 'j' }), 500)
  s = presence(s, ev('hello', { protocol: 2, agent: 'rth-agentd', version: '0', boot_id: 'b2', journal: 'j' }), 600)
  assert.equal(view(s, 600).faults.length, 0, 'a new agent boot starts without the old marks')
})

test('sleepy after ~2 min idle, asleep after ~10, and any event wakes it; heartbeats are not activity', () => {
  let s = live()
  let t = 100
  for (; t < 100 + IDLE.asleepMs + 1000; t += 1000) s = presence(s, heartbeat(), t)
  s = presence(s, host('unmuted'), t)
  // The last activity is the attach at t=0: heartbeats and host reads never count.
  assert.equal(view(s, IDLE.sleepyMs - 1).pose, 'idle')
  assert.equal(view(s, IDLE.sleepyMs).pose, 'sleepy')
  assert.equal(view(s, t).pose, 'asleep')
  assert.match(view(s, t).because, /says nothing about the microphone/)
  assert.equal(view(s, t).layers.mic.state, 'not_muted', 'sleep never implies the mic is off')
  s = presence(s, ev('log', { level: 'info', msg: 'x' }), t + 1)
  assert.equal(view(s, t + 1).pose, 'idle')
})

test('the mic indicator is its own layer: applied mute only, with a lease; requested-but-unapplied is unknown', () => {
  let s = live()
  assert.equal(view(s, 100).layers.mic.state, 'not_muted')
  s = presence(s, host('muting'), 200)
  assert.equal(view(s, 200).layers.mic.state, 'muting', 'a request the host has not applied is not shown as muted')
  assert.equal(view(s, 200).layers.mic.at, 200, 'the sample time travels with the layer (native labels lease it)')
  s = presence(s, host('muted'), 300)
  assert.equal(view(s, 300).layers.mic.state, 'muted')
  assert.equal(view(s, 300).pose, 'idle', 'the creature pose is independent of the mic')
  assert.equal(view(s, 300 + LEASE.hostMs + 1).layers.mic.state, 'unknown', 'a stale host read is unknown')
  s = presence(s, { type: 'host', status: null }, 400)
  assert.equal(view(s, 400).layers.mic.state, 'unknown')
  s = presence(s, host('unmuted', { io: 'off' }), 500)
  assert.equal(view(s, 500).layers.mic.state, 'off')
  s = presence(s, { type: 'host', status: { host: { voice_policy: { voice_io: 'vpio' } }, commands: { unfinished: 0 } } }, 600)
  assert.equal(view(s, 600).layers.mic.state, 'unknown', 'an older host without a mute state')
})

test('replay is deterministic: the same inputs and clock give the same view', () => {
  const steps = [[0, { type: 'link', state: 'attached' }], [5, host('muted')], [10, heartbeat()], [20, ev('command_admitted', { command_id: 'c', generation: 1, status: 'admitted' })]]
  seq = 0
  const a = view(run(steps), 30)
  seq = 0
  const b = view(run(steps), 30)
  assert.deepEqual(a, b)
})

test('reducer purity: inputs never mutate the previous state', () => {
  const s = live()
  const frozen = JSON.stringify(s)
  presence(s, ev('command_admitted', { command_id: 'c', generation: 1, status: 'admitted' }), 200)
  presence(s, { type: 'link', state: 'attached' }, 300)
  assert.equal(JSON.stringify(s), frozen)
})

test('plugin: the presence rides ingest and link, is pushed on change only, and has its own routes', async () => {
  const { Context } = await import('@deepseek-ai/cordis')
  const { SessionProjectionRegistry } = await import('@deepseek-ai/dsh-session-projection')
  const plugin = await import('../index.mjs')
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  const registered = []
  ctx.provide('webServer', { register: (route) => (registered.push(route), () => {}) })
  const fiber = ctx.plugin(plugin, { agentd: { enabled: false }, transport: 'webServer' })
  await fiber
  await new Promise((r) => setImmediate(r))
  const service = ctx.get('rthEvents')
  assert.deepEqual(registered.map((r) => r.path).filter((p) => p.includes('presence')), ['/api/rth-presence', '/api/rth-presence/stream'])
  const sent = []
  const hub = service.presenceStreams
  const send = hub.send
  hub.send = (event, data, id) => (sent.push([event, data.pose, id]), send(event, data, id))
  assert.equal(service.presence().view.pose, 'unknown')
  service.setLink('attached', 'observer')
  service.hostStatus({ host: { voice_policy: { voice_io: 'scripted' }, mic: { state: 'muted' } }, commands: { unfinished: 0 } })
  service.ingest(env('heartbeat', { uptime_ms: 1, attachments: 1 }))
  const { view: v, rev } = service.presence()
  assert.equal(v.pose, 'idle')
  assert.equal(v.layers.mic.state, 'muted')
  const before = sent.length
  service.ingest(env('heartbeat', { uptime_ms: 2, attachments: 1 }))
  assert.equal(sent.length, before, 'an unchanged view is not pushed again')
  assert.equal(service.presence().rev, rev)
  service.ingest(env('command_admitted', { command_id: 'c1', generation: 1, status: 'admitted' }))
  assert.deepEqual(sent.at(-1).slice(0, 2), ['presence', 'pending'])
  await fiber.dispose()
})

test('plugin: the presence stream heartbeats while a reader is connected, so a reader can lease it', async () => {
  const { Context } = await import('@deepseek-ai/cordis')
  const { SessionProjectionRegistry } = await import('@deepseek-ai/dsh-session-projection')
  const plugin = await import('../index.mjs')
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  ctx.provide('webServer', { register: () => () => {} })
  const fiber = ctx.plugin(plugin, { agentd: { enabled: false }, transport: 'webServer', presence: { heartbeatMs: 20 } })
  await fiber
  const hub = ctx.get('rthEvents').presenceStreams
  const sent = []
  const send = hub.send
  hub.send = (event, data, id) => (sent.push(event), send(event, data, id))
  await new Promise((r) => setTimeout(r, 90))
  assert.ok(sent.filter((e) => e === 'heartbeat').length >= 2, `heartbeats: ${sent}`)
  assert.ok(!sent.includes('presence'), 'an unchanged view is not re-sent; the heartbeat carries liveness')
  await fiber.dispose()
  const n = sent.length
  await new Promise((r) => setTimeout(r, 60))
  assert.equal(sent.length, n, 'unload stops the heartbeat')
})

// The ACTUAL command lifecycle on the live feed: the admission ack
// carries no response_id and no `dispatched` follows; the reply names the command as request_id.
const admitted = (id) => ev('command_admitted', { command_id: id, generation: 1, status: 'admitted' })
const started = (rid, request_id) => ev('response_started', request_id === undefined ? { response_id: rid } : { response_id: rid, request_id })
const done = (rid, status) => ev('agent_done', { response_id: rid, status })

test('actual lifecycle: each of completed, cancelled and failed ends only its own command; a concurrent one survives', () => {
  for (const status of ['completed', 'cancelled', 'failed']) {
    let s = live()
    s = presence(s, admitted('c1'), 200)
    s = presence(s, admitted('c2'), 210)
    s = presence(s, started('r1', 'c1'), 300)
    s = presence(s, done('r1', status), 400)
    assert.deepEqual(Object.keys(s.commands), ['c2'], `${status}: only c1 is cleared`)
    const after = view(s, 400 + WINCE_MS)
    assert.equal(after.pose, 'pending', `${status}: the concurrent c2 is still pending`)
    assert.match(after.layers.request.label, /command c2 admitted/)
    assert.doesNotMatch(after.layers.request.label, /c1|r1/)
    if (status === 'failed') assert.equal(view(s, 400).pose, 'wince', 'a failed reply is a fault')
    s = presence(s, started('r2', 'c2'), 500)
    s = presence(s, done('r2', 'completed'), 600)
    assert.deepEqual(s.commands, {})
    assert.equal(view(s, 600 + WINCE_MS).pose, 'idle', `${status}: no open request once both end`)
    assert.equal(view(s, 600 + WINCE_MS).layers.request.label, 'requests: none open')
  }
})

test('actual lifecycle: unrelated or absent request_ids clear nothing and invent no command', () => {
  let s = presence(live(), admitted('c1'), 200)
  s = presence(s, started('rMic', 'mic-1'), 300) // a provider-opened (spoken) reply
  s = presence(s, started('rNone'), 310) // a reply without request_id
  assert.deepEqual(Object.keys(s.commands), ['c1'], 'no phantom command')
  assert.equal(s.commands.c1.response_id, null, 'c1 is not linked to an unrelated reply')
  s = presence(s, done('rMic', 'completed'), 400)
  s = presence(s, done('rNone', 'cancelled'), 410)
  assert.deepEqual(Object.keys(s.commands), ['c1'], 'their terminals leave c1 pending')
  assert.equal(view(s, 410).pose, 'pending')
  assert.match(view(s, 410).layers.request.label, /^pending: command c1 admitted, not finished$/)
})

test('actual lifecycle: message_started links without mutating its input or changing the pose', () => {
  const before = presence(live(), admitted('c1'), 200)
  const frozen = JSON.stringify(before)
  const after = presence(before, started('r1', 'c1'), 300)
  assert.equal(JSON.stringify(before), frozen, 'the input state is unchanged')
  assert.equal(view(before, 300).pose, 'pending')
  assert.equal(view(after, 300).pose, 'pending', 'still pending: a started reply is not an answer yet')
  assert.deepEqual(Object.keys(after.commands), ['c1'])
  assert.equal(after.commands.c1.response_id, 'r1', 'the command now knows its reply')
  assert.equal(after.commands.c1.since, before.commands.c1.since)
})

// `host.call` (rth-host's call owner): the mic indicator leads with the call.
const hostCall = (call, mic = 'unmuted', io = 'vpio') => ({ type: 'host', status: { host: { voice_policy: { voice_io: io }, mic: { state: mic }, ...(call === undefined ? {} : { call }) }, commands: { unfinished: 0 } } })
const micAt = (s, t) => view(s, t).layers.mic

test('call-aware mic: no call, starting, active (muted / not muted), ending and a failed start', () => {
  let s = presence(live(), hostCall({ state: 'off', desired: 'off', reason: null }), 200)
  assert.equal(micAt(s, 200).label, 'mic: no call: the call devices are released and no microphone is open')
  assert.deepEqual(micAt(s, 200).call, { state: 'off', reason: null })
  assert.equal(micAt(s, 200).state, 'not_muted', 'the mute fence is still reported as the fence (the native tray reads it)')
  assert.doesNotMatch(micAt(s, 200).label, /vpio|call active/, 'no call is never "not muted (vpio)"')

  s = presence(s, hostCall({ state: 'starting', desired: 'on', reason: 'devices opening' }), 300)
  assert.match(micAt(s, 300).label, /^mic: call starting \(devices opening\); not muted/)
  s = presence(s, hostCall({ state: 'active', desired: 'on', reason: 'devices open (call 1)' }), 400)
  assert.match(micAt(s, 400).label, /^mic: call active; not muted \(vpio\)/)
  s = presence(s, hostCall({ state: 'active', desired: 'on' }, 'muted'), 500)
  assert.equal(micAt(s, 500).label, 'mic: call active; muted at the host: no microphone audio is sent (input only)')
  assert.equal(micAt(s, 500).state, 'muted')
  s = presence(s, hostCall({ state: 'active', desired: 'on' }, 'muting'), 550)
  assert.match(micAt(s, 550).label, /^mic: call active; mute requested, not yet applied/, 'a requested mute is never shown as applied')
  s = presence(s, hostCall({ state: 'ending', desired: 'off', reason: 'devices releasing' }, 'muted'), 600)
  assert.match(micAt(s, 600).label, /^mic: call ending \(devices releasing\); muted at the host/)
  s = presence(s, hostCall({ state: 'error', desired: 'on', reason: 'start failed', error: 'VoiceProcessingIO unavailable' }), 700)
  assert.equal(micAt(s, 700).label, 'mic: no call: the start failed (VoiceProcessingIO unavailable)')
  assert.equal(view(s, 700).pose, 'idle', 'the creature pose never follows the call or the mic')
})

test('call-aware mic: hosts without a call owner or without a `call` key are never "in a call"', () => {
  const before = presence(live(), hostCall(undefined), 200) // older host: no `call` key
  assert.equal(micAt(before, 200).call, null)
  assert.match(micAt(before, 200).label, /^mic: not muted \(vpio\)/, 'the fence-only wording, unchanged')
  const noOwner = presence(live(), hostCall(null), 200) // a host running no call owner
  assert.equal(micAt(noOwner, 200).call, null)
  assert.doesNotMatch(micAt(noOwner, 200).label, /call/)
  const odd = presence(live(), hostCall({ state: 'warming_up' }), 200) // a future state this reducer does not know
  assert.deepEqual(micAt(odd, 200).call, { state: 'unknown', reason: null })
  assert.match(micAt(odd, 200).label, /^mic: call state unknown \(warming_up\)/)
})

test('call-aware mic: a stale host read is unknown, whatever the call said', () => {
  const s = presence(live(), hostCall({ state: 'active', desired: 'on' }, 'muted'), 300)
  assert.match(micAt(s, 300).label, /call active/)
  const stale = micAt(s, 300 + LEASE.hostMs + 1)
  assert.equal(stale.state, 'unknown')
  assert.equal(stale.label, 'mic: unknown (host status is stale)')
  assert.equal(stale.call, undefined, 'no call is claimed once the lease is gone')
  const gone = micAt(presence(s, { type: 'host', status: null }, 400), 400)
  assert.equal(gone.state, 'unknown')
  assert.equal(gone.call, null)
})
