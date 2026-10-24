// Evidence-bound joins, playback and command identity, inherited status, one time base per axis,
// the shared stream and the icon notices, driven through raw projection rows, real Mind rows
// (MindStream), the built fork (functions extracted from the build output) and the shared stream
// region of src/runtime.mjs under fakes.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import { freshness as mindFreshness } from '../../dsh-mind/src/display.mjs'
import { MindStream } from '../../dsh-mind/src/mind.mjs'
import * as A from '../src/adapter.mjs'
import { KINDS } from '../src/taxonomy.mjs'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const sha = (b) => createHash('sha256').update(b).digest('hex')
const row = (id, kind, t, ref = {}, episode = null, lane = 'host') => ({
  id, kind, lane, ref: lane === 'host' ? { host_life: 'h1', ...ref } : { producer: 'boot', ...ref }, clock: { t_ms: t, t_ns: t * 1e6 },
  observed_at: t, episode, sentence: kind, status: 'observed',
})
const build = (rows, extra = {}) => A.buildRthTrajectory({ rows, episodes: {}, ...extra })
const cells = (b) => b.turns.flatMap((t) => t.groups.flatMap((g) => g.cells))
const turnOf = (b, kind) => b.turns.find((t) => t.groups[0].cells.some((c) => c.rthKind === kind))

// One fresh build of the fork, shared by the rendered-fork cases below.
const dir = mkdtempSync(join(tmpdir(), 'rth-traj-repair-'))
execFileSync(process.execPath, [join(ROOT, 'build.mjs'), '--out', join(dir, 'client.js')], { stdio: 'pipe' })
const bundle = readFileSync(join(dir, 'client.js'), 'utf8')
rmSync(dir, { recursive: true, force: true })
const fn = (name) => {
  const m = bundle.match(new RegExp(`(?<=\\n\\t\\t)function ${name}\\([^)]*\\) \\{[\\s\\S]*?\\n\\t\\t\\}`))
  assert.ok(m, `the built fork defines ${name}`)
  return m[0]
}
const status = await import('../src/status.mjs').catch(() => null)

// ------------------------------------------------------------------ evidence-bound joins only
test('an utterance is never joined to a reply by proximity or by a session-wide stream set', () => {
  const b = build([
    row('v', 'local_vad_onset', 1, { stream_id: 'OTHER' }),
    { ...row('u', 'input_transcript', 2, { stream_id: 'ACTUAL', utterance_id: 'u' }, 'utterance'), sentence: 'Provider transcribed "UNRELATED INPUT" (final; speaker not identified)' },
    row('r', 'response_started', 3, { response_id: 'R', request_id: 'OTHER' }, 'reply'),
    row('d', 'response_done', 4, { response_id: 'R', status: 'completed' }, 'reply'),
  ])
  for (const t of b.turns) assert.ok(!(t.groups[0].cells.some((c) => c.rthKind === 'voice') && t.groups[0].cells.some((c) => c.rthKind === 'rth')), 'no turn holds both')
  assert.ok(b.turns.every((t) => (t.rthEpisodes ?? [t.rthEpisode]).filter(Boolean).length <= 1), 'one Mind episode per turn')
  const reply = turnOf(b, 'rth')
  assert.ok(!b.cards.get(reply.turn).chips.includes('voice'), 'no voice chip from a stream id')
  const you = cells(b).find((c) => c.rthKind === 'voice')
  assert.match(you.rthAssociation ?? '', /not available/, 'the heard record says its reply association is unavailable')
})

test('two utterances on one stream, then a reply naming that stream: neither joins, no voice chip', () => {
  const b = build([
    row('v1', 'local_vad_onset', 1, { stream_id: 'mic-1' }),
    { ...row('t1', 'input_transcript', 2, { stream_id: 'mic-1' }), sentence: 'Provider transcribed "one" (final; speaker not identified)' },
    { ...row('t2', 'input_transcript', 3, { stream_id: 'mic-1' }), sentence: 'Provider transcribed "two" (final; speaker not identified)' },
    row('r', 'response_started', 4, { response_id: 'R', request_id: 'mic-1' }, 'reply'),
    row('d', 'response_done', 5, { response_id: 'R', status: 'completed' }, 'reply'),
  ])
  const reply = turnOf(b, 'rth')
  assert.ok(!reply.groups[0].cells.some((c) => c.rthKind === 'voice'))
  assert.ok(!b.cards.get(reply.turn).chips.includes('voice'))
  assert.equal(cells(b).filter((c) => c.rthKind === 'voice').length, 2, 'one heard record per final utterance')
})

test('command ids are qualified by host lifetime: a reply never takes another process\'s command', () => {
  // Mind joined them into one episode by id; the processes differ, so nothing is typed or attributed.
  const cross = build([
    row('c1', 'command_written', 1, { command_id: 'C', host_life: 'h1' }, 'e'),
    row('r2', 'response_started', 2, { response_id: 'R2', request_id: 'C', host_life: 'h2' }, 'e'),
    row('d2', 'response_done', 3, { response_id: 'R2', status: 'completed', host_life: 'h2' }, 'e'),
  ])
  assert.ok(![...cross.cards.values()][0].chips.includes('typed'), 'no typed mode from another process')
  assert.match(cells(cross).find((c) => c.rthKind === 'typed')?.rthAssociation ?? '', /not established/)
  // The same id reused in two processes, each with its own reply: each pairs only within its process.
  const reused = build([
    row('c1', 'command_written', 1, { command_id: 'C', host_life: 'h1' }, 'e1'),
    row('r1', 'response_started', 2, { response_id: 'R1', request_id: 'C', host_life: 'h1' }, 'e1'),
    row('f1', 'command_finished', 3, { command_id: 'C', status: 'completed', host_life: 'h1' }, 'e1'),
    row('c2', 'command_written', 4, { command_id: 'C', host_life: 'h2' }, 'e2'),
    row('r2', 'response_started', 5, { response_id: 'R2', request_id: 'C', host_life: 'h2' }, 'e2'),
  ])
  const cmds = cells(reused).filter((c) => c.rthKind === 'command')
  assert.equal(cmds.length, 2)
  assert.match(cmds[0].text, /completed \(host\)/, 'h1 finish attributed to the h1 write')
  assert.doesNotMatch(cmds[1].text, /completed/, 'h2 has no finish of its own; h1\'s is not borrowed')
  assert.equal(cmds[1].rthOutcome ?? null, null)
  // Written twice within one process: ambiguous, no outcome.
  const twice = build([
    row('c1', 'command_written', 1, { command_id: 'C' }, 'e1'),
    row('c2', 'command_written', 2, { command_id: 'C' }, 'e1'),
    row('f', 'command_finished', 3, { command_id: 'C', status: 'completed' }, 'e1'),
  ])
  for (const c of cells(twice).filter((x) => x.rthKind === 'command')) assert.equal(c.rthOutcome ?? null, null)
})

test('overlapping replies and an unrelated speaker-clear: no causal interrupted chip', () => {
  const b = build([
    row('a1', 'response_started', 1, { response_id: 'R1', request_id: 'q1' }, 'e1'),
    row('b1', 'response_started', 2, { response_id: 'R2', request_id: 'q2' }, 'e2'),
    row('x', 'speaker_cleared', 2.5, {}),
    row('a2', 'response_done', 3, { response_id: 'R1', status: 'completed' }, 'e1'),
    row('b2', 'response_done', 4, { response_id: 'R2', status: 'completed' }, 'e2'),
  ])
  for (const [, card] of b.cards) assert.ok(!card.chips.some((c) => /interrupted|cancelled/.test(c)), card.chips.join(','))
  const replies = cells(b).filter((c) => c.rthKind === 'rth')
  assert.equal(replies.length, 2)
  assert.ok(cells(b).some((c) => c.rthKind === 'interrupt'), 'the speaker clear is still shown as observed')
})

test('a host cancellation naming the reply in its lifetime is the only cancelled evidence', () => {
  const linked = build([
    row('s', 'response_started', 1, { response_id: 'R', request_id: 'q' }, 'e'),
    row('x', 'response_cancelled', 2, { response_id: 'R' }, 'e'),
  ])
  assert.ok([...linked.cards.values()][0].chips.includes('⚡ cancelled (host)'))
  const otherLife = build([
    row('s', 'response_started', 1, { response_id: 'R', request_id: 'q', host_life: 'h1' }, 'e'),
    row('x', 'response_cancelled', 2, { response_id: 'R', host_life: 'h2' }, 'e'),
  ])
  assert.ok(![...otherLife.cards.values()][0].chips.some((c) => /cancelled|interrupted/.test(c)), 'a cancel from another host process is not this reply')
})

// ------------------------------------------------------------------ playback, audio, commands, wording
test('generated agent audio is not playback', () => {
  const c = cells(build([row('a', 'agent_audio', 10, { response_id: 'R' }, 'reply', 'agentd')]))
  assert.equal(c.length, 1)
  assert.notEqual(c[0].rthKind, 'speaking')
  assert.doesNotMatch(c[0].text, /playback/i)
  assert.ok(KINDS[c[0].rthKind], 'a kind of its own in the taxonomy')
})

test('two playback starts and one finish for one reply and lifetime: no measured span', () => {
  const c = cells(build([
    row('s1', 'playback', 100, { response_id: 'R', phase: 'started' }, 'reply'),
    row('s2', 'playback', 900, { response_id: 'R', phase: 'started' }, 'reply'),
    row('e', 'playback', 1000, { response_id: 'R', phase: 'finished' }, 'reply'),
  ])).filter((x) => x.rthKind === 'speaking')
  assert.ok(c.length >= 1)
  for (const x of c) {
    assert.equal(x.timeSeconds, null)
    assert.notEqual(x.rthSpan, 'measured')
  }
  assert.match(c.map((x) => x.rthTiming?.[0]?.why ?? '').join(' '), /2 playback starts/)
})

test('a single playback interval is measured with named endpoints and evidence', () => {
  const c = cells(build([
    row('s', 'playback', 100, { response_id: 'R', phase: 'started' }, 'reply'),
    row('e', 'playback', 1000, { response_id: 'R', phase: 'finished' }, 'reply'),
  ])).find((x) => x.rthKind === 'speaking')
  assert.equal(c.rthSpan, 'measured')
  assert.equal(c.timeSeconds, 0.9)
  assert.deepEqual(c.rthTiming[0].start, { kind: 'playback', row: 's' })
  assert.deepEqual(c.rthTiming[0].end, { kind: 'playback', row: 'e' })
  assert.equal(c.rthTiming[0].clock, 'rth-host')
})

test('a command written in one host process and finished in another has no outcome', () => {
  const c = cells(build([
    row('c1', 'command_written', 100, { command_id: 'C', host_life: 'h1' }, 'command'),
    row('c2', 'command_finished', 200, { command_id: 'C', host_life: 'h2', status: 'completed' }, 'command'),
  ])).filter((x) => x.rthKind === 'command')
  assert.equal(c.length, 1)
  assert.doesNotMatch(c[0].text, /completed/)
  assert.equal(c[0].isError, false)
  assert.equal(c[0].rthOutcome ?? null, null)
  assert.match(c[0].rthTiming[0].why ?? '', /different rth-host processes|not attributed/)
})

test('echo and VAD wording keeps the source uncertainty (real Mind rows)', () => {
  const mind = new MindStream({ limit: 100 })
  const ev = (o) => ({ via: 'host journal', file: '/x/rth-host.jsonl', offset: o, length: 1 })
  mind.host({ kind: 'started', pid: 1, t_ms: 0 }, ev(0))
  mind.host({ kind: 'echo_gate', stream_id: 'mic-1', action: 'nothing_audible', audible: false, t_ms: 10 }, ev(1))
  mind.host({ kind: 'echo_probe', stream_id: 'mic-1', t_ms: 20 }, ev(2))
  mind.host({ kind: 'local_vad_onset', stream_id: 'mic-1', level_dbfs: -20, action: 'nothing_audible', audible: false, t_ms: 30 }, ev(3))
  const b = A.buildRthTrajectory({ rows: mind.snapshot().rows })
  const on = A.filterTurns(b.turns, null)
  const texts = cells({ turns: on }).map((c) => `${c.rthKind}: ${c.text}`).join('\n')
  assert.doesNotMatch(texts, /ignored own voice/)
  assert.doesNotMatch(texts, /speech started/)
  assert.match(texts, /vad: .*(possible|Not proof)/)
})

// ------------------------------------------------------------------ inherited status
test('the built fork reports open, cancelled, ambiguous and completed replies as such', () => {
  assert.ok(status, 'src/status.mjs exists')
  const ctx = vm.createContext({ RTH: status })
  vm.runInContext(`${fn('stateOf')}\n${fn('statusLabel')}`, ctx)
  const replyOf = (rows) => cells(build(rows)).find((c) => c.rthKind === 'rth')
  const open = replyOf([row('r', 'response_started', 3, { response_id: 'R', request_id: 'q' }, 'reply')])
  const cancelled = replyOf([row('r', 'response_started', 3, { response_id: 'R', request_id: 'q' }, 'reply'), row('x', 'response_cancelled', 4, { response_id: 'R' }, 'reply')])
  const ambiguous = replyOf([
    row('r1', 'response_started', 1, { response_id: 'R', request_id: 'q1' }, 'reply'),
    row('r2', 'response_started', 2, { response_id: 'R', request_id: 'q2', host_life: 'h2' }, 'reply'),
  ])
  const done = replyOf([row('r', 'response_started', 3, { response_id: 'R', request_id: 'q' }, 'reply'), row('d', 'response_done', 4, { response_id: 'R', status: 'completed' }, 'reply')])
  const t = (k) => k
  const state = (c) => ctx.stateOf({ cell: c })
  assert.notEqual(state(open), 'complete')
  assert.equal(ctx.statusLabel(state(open), t), 'rth.status.pending')
  assert.equal(ctx.statusLabel(state(cancelled), t), 'rth.status.cancelled')
  assert.equal(ctx.statusLabel(state(ambiguous), t), 'rth.status.unavailable')
  assert.equal(ctx.statusLabel(state(done), t), 'status.completed')
  const foreignEnd = replyOf([
    row('r', 'response_started', 3, { response_id: 'R', request_id: 'q', host_life: 'h1' }, 'reply'),
    row('d', 'response_done', 4, { response_id: 'R', status: 'completed', host_life: 'h2' }, 'reply'),
  ])
  assert.equal(ctx.statusLabel(state(foreignEnd), t), 'rth.status.unavailable', 'an end from another host process is not this reply\'s outcome')
  const vad = cells(build([row('v', 'local_vad_onset', 1, { stream_id: 'm' })]))[0]
  assert.equal(ctx.statusLabel(state(vad), t), 'rth.status.observed', 'an observed record claims no outcome')
  const crossCmd = cells(build([
    row('c1', 'command_written', 1, { command_id: 'C', host_life: 'h1' }),
    row('c2', 'command_finished', 2, { command_id: 'C', host_life: 'h2', status: 'completed' }),
  ])).find((c) => c.rthKind === 'command')
  assert.notEqual(state(crossCmd), 'complete', 'an unattributed command finish is not success')
})

// ------------------------------------------------------------------ one time base per axis
test('upstream\'s timeline only ever gets sequence mode; Duration is the producer-clock overview', () => {
  const view = readFileSync(join(ROOT, 'src/view.js'), 'utf8')
  assert.match(view, /const timelineMode = "sequence";/)
  assert.doesNotMatch(view, /"actual"|"duration"|"time"/, 'no upstream timed mode string')
  assert.match(view, /actualDuration \? \(0, react_jsx_runtime\.jsx\)\(RTH\.ClockOverview/, 'Duration renders the clock-group overview')
  assert.match(view, /RTH\.layoutClockOverview\(timelineTurns\)/)
  assert.match(bundle, /hidden: rthSequenceOnly === true/, 'the hide hook stays available')
})

test('sequence placement ignores delayed batches, wall-clock jumps and mixed lifetimes', () => {
  const ctx = vm.createContext({})
  const start = bundle.indexOf('//#region lib/types/client/timeline.js')
  vm.runInContext(bundle.slice(start, bundle.indexOf('//#endregion', start)), ctx)
  const variant = (obs) => build([
    { ...row('s', 'response_started', 100, { response_id: 'R', request_id: 'q' }, 'reply'), observed_at: obs[0] },
    { ...row('e', 'response_done', 200, { response_id: 'R', status: 'completed' }, 'reply'), observed_at: obs[1] },
    { ...row('p', 'playback', 50, { response_id: 'R2', phase: 'started', host_life: 'h2' }, 'r2'), observed_at: obs[2] },
    { ...row('f', 'playback', 900, { response_id: 'R2', phase: 'finished', host_life: 'h2' }, 'r2'), observed_at: obs[3] },
  ]).turns
  const spans = (turns) => ctx.deriveTrajectoryTimeline(turns, 'sequence').spans.map((s) => [s.start, s.end])
  const base = spans(variant([1, 2, 3, 4]))
  assert.deepEqual(spans(variant([10000, 10001, 10002, 10003])), base, 'delayed batch')
  assert.deepEqual(spans(variant([1, 2, 1e12, 1e12 + 1])), base, 'wall-clock jump')
  // Timed modes are not offered, and even if called they draw no invented interval.
  const delayed = variant([10000, 10001, 10002, 10003])
  for (const mode of ['actual', 'duration', 'time']) {
    for (const s of ctx.deriveTrajectoryTimeline(delayed, mode).spans) assert.ok(s.end - s.start <= 1 || mode !== 'actual', `${mode}: ${s.start}-${s.end}`)
  }
  const actual = ctx.deriveTrajectoryTimeline(delayed, 'actual').spans
  assert.ok(actual.every((s) => s.end <= 10003), 'no span reaches past the last observation')
})

test('an RTH tooltip never splices observation time with a producer duration', () => {
  assert.ok(status)
  const ctx = vm.createContext({ RTH: status })
  vm.runInContext(fn('timelineRecordDetail'), ctx)
  const reply = cells(build([
    { ...row('s', 'response_started', 100, { response_id: 'R', request_id: 'q' }, 'reply'), observed_at: 10000 },
    { ...row('e', 'response_done', 200, { response_id: 'R', status: 'completed' }, 'reply'), observed_at: 10001 },
  ])).find((c) => c.rthKind === 'rth')
  const d = ctx.timelineRecordDetail(reply)
  assert.equal(d.startedAt, undefined, 'no observation-time start next to a host-clock duration')
  assert.equal(d.durationMs, 100)
})

// ------------------------------------------------------------------ the shared stream
function hub() {
  const runtime = readFileSync(join(ROOT, 'src/runtime.mjs'), 'utf8')
  let now = 1000
  const timers = new Map()
  let nextTimer = 1
  const connections = []
  class Source {
    constructor() { this.handlers = {}; connections.push(this) }
    addEventListener(k, f) { this.handlers[k] = f }
    close() { this.closed = true }
  }
  const ctx = vm.createContext({
    EventSource: Source, URL, Map, Set, Math, Number, Object, Array, JSON,
    Date: { now: () => now }, document: { baseURI: 'http://fake.invalid/', visibilityState: 'visible' },
    requestAnimationFrame: (f) => f(),
    setTimeout: (f, ms) => { const id = nextTimer++; timers.set(id, { f, at: now + ms }); return id },
    clearTimeout: (id) => { timers.delete(id) },
    buildRthTrajectory: A.buildRthTrajectory, RTH_SESSION: 'rth-root',
  })
  const code = runtime.slice(runtime.indexOf('const initial ='), runtime.indexOf('// ---------------------------------------------------------------- icons')).replaceAll('export ', '')
  vm.runInContext(code, ctx)
  const send = (i, event, data) => connections[i].handlers[event]({ data: JSON.stringify(data) })
  const advance = (ms) => {
    now += ms
    for (const [id, t] of [...timers]) if (t.at <= now) { timers.delete(id); t.f() }
  }
  return { ctx, connections, timers, send, advance, source: vm.runInContext('sourceFor("rth-root")', ctx), now: () => now }
}

test('isolation (source): an unrelated session gets the not-RTH source, no turns, and never opens an EventSource', () => {
  // A source-level assertion on the shared-stream block: this is not a rendered or native
  // session-switch test.
  const h = hub()
  const other = vm.runInContext('sourceFor("unrelated-session")', h.ctx)
  const snap = other.getSnapshot()
  assert.equal(snap.notRth, true)
  assert.equal(snap.turns.length, 0)
  let wakes = 0
  const off = other.subscribe(() => { wakes++ })
  off()
  assert.equal(h.connections.length, 0, 'subscribe/unsubscribe on another session constructs no EventSource')
  assert.equal(wakes, 0)
  assert.equal(other.getSnapshot().notRth, true, 'and its snapshot stays the not-RTH one')
  // the control: the RTH session's source does connect, once
  const offRth = h.source.subscribe(() => {})
  assert.equal(h.connections.length, 1)
  offRth()
})

test('a silent but open stream stops being live after three missed heartbeats, without new input', () => {
  const h = hub()
  let wakes = 0
  const off = h.source.subscribe(() => { wakes++ })
  assert.notEqual(h.source.getSnapshot().stream, 'live', 'opening is not live')
  h.send(0, 'snapshot', { rows: [], episodes: [], heartbeat_ms: 5000 })
  assert.equal(h.source.getSnapshot().stream, 'live')
  const before = wakes
  h.advance(15_001)
  assert.equal(h.source.getSnapshot().stream, 'stale', 'expired by the browser clock alone')
  assert.ok(wakes > before, 'a scheduled expiry woke the subscribers')
  h.send(0, 'heartbeat-mind', { at: h.now() })
  assert.equal(h.source.getSnapshot().stream, 'live', 'fresh evidence recovers')
  off()
  assert.equal(h.connections[0].closed, true)
  assert.equal(h.timers.size, 0, 'the final unsubscribe clears the lease timer')
})

test('the lease is Mind\'s rule (three beats), checked against dsh-mind/src/display.mjs', () => {
  const h = hub()
  const off = h.source.subscribe(() => {})
  h.send(0, 'snapshot', { rows: [], episodes: [], heartbeat_ms: 2000 })
  for (const dt of [0, 5999, 6000, 6001, 30_000]) {
    const t0 = h.now()
    h.advance(dt)
    const ours = h.source.getSnapshot().stream
    const mind = mindFreshness({ stream: 'open', lastStreamAt: t0, heartbeatMs: 2000, offsetMs: 0, link: null }, h.now()).stream.state
    assert.equal(ours, mind, `after ${dt} ms`)
    h.send(0, 'heartbeat-mind', { at: h.now() })
  }
  off()
})

test('arrivals are stable identities: append + evict at the window limit counts one new record', () => {
  assert.equal(typeof A.newSince, 'function')
  const h = hub()
  const off = h.source.subscribe(() => {})
  h.send(0, 'snapshot', { rows: [row('a', 'mute_applied', 1), row('b', 'mute_applied', 2)], episodes: [], heartbeat_ms: 5000 })
  const s1 = h.source.getSnapshot()
  const mark = s1.latestArrival
  assert.ok(Number.isFinite(mark))
  h.send(0, 'evicted', { id: 'a' })
  h.send(0, 'row', row('c', 'mute_applied', 3))
  const s2 = h.source.getSnapshot()
  assert.equal(A.newSince(s2.turns, mark, s2.arrivals), 1)
  assert.ok(s2.latestArrival > mark, 'the follow watermark moved although the max index did not')
  assert.equal(s2.arrivals.size, 2, 'bookkeeping is bounded by the window (evicted ids are dropped)')
  off()
})

test('folding and the Signals filter do not change the new-record count', () => {
  const h = hub()
  const off = h.source.subscribe(() => {})
  h.send(0, 'snapshot', { rows: [row('a', 'mute_applied', 1)], episodes: [], heartbeat_ms: 5000 })
  const mark = h.source.getSnapshot().latestArrival
  h.send(0, 'row', row('s1', 'provider_signal', 2, { signal: 'x' }))
  h.send(0, 'row', row('s2', 'provider_signal', 3, { signal: 'y' }))
  const s = h.source.getSnapshot()
  const folded = A.filterTurns(s.turns, new Set(A.DEFAULT_GROUPS))
  const single = A.filterTurns(s.turns, null)
  assert.equal(A.newSince(folded, mark, s.arrivals), 2)
  assert.equal(A.newSince(single, mark, s.arrivals), 2)
  off()
})

test('the view tracks arrivals and selection by record identity, not by display index', () => {
  const view = readFileSync(join(ROOT, 'src/view.js'), 'utf8')
  assert.match(view, /latestArrival/)
  assert.match(view, /RTH\.newSince\(/)
  assert.doesNotMatch(view, /newestIndex/)
  assert.match(view, /selectedRecordId/)
  assert.match(view, /addEventListener\("scroll"/, 'scrolling away pauses Live')
})

// ------------------------------------------------------------------ icon notices
test('icons come from a pinned lucide-static release; full ISC and Feather MIT notices ship', () => {
  const src = join(ROOT, 'third_party/lucide-static-0.460.0')
  assert.ok(existsSync(join(src, 'LICENSE')) && existsSync(join(src, 'SOURCE.json')))
  const meta = JSON.parse(readFileSync(join(src, 'SOURCE.json'), 'utf8'))
  assert.equal(meta.integrity, 'sha512-X6pIdg7jVxv7YQ/uR241hwhNiztcAfmj181TbcX7HCxxk/3mGaRtAc6b2ftUvQBufbJE6ehgyzO2uVsa604tWg==')
  const isc = readFileSync(join(src, 'LICENSE'), 'utf8')
  const feather = readFileSync(join(ROOT, 'third_party/feather-icons-4.29.2/LICENSE'), 'utf8')
  assert.match(isc, /Permission to use, copy, modify, and\/or distribute this software/)
  assert.match(isc, /Cole Bemis 2013-2022 as part of Feather \(MIT\)/)
  assert.match(feather, /Copyright \(c\) 2013-2023 Cole Bemis/)
  const { ICONS, ICON_SOURCE } = await_import_icons()
  assert.equal(ICON_SOURCE.version, '0.460.0')
  for (const k of Object.values(KINDS)) {
    assert.ok(ICONS[k.icon], `icon ${k.icon} is pinned`)
    assert.equal(meta.icons[k.icon], ICON_SOURCE.sha256[k.icon], `icon ${k.icon} matches the release svg`)
  }
  const header = bundle.slice(0, bundle.indexOf('*/') + 2)
  const notice = readFileSync(join(ROOT, 'NOTICE.md'), 'utf8')
  for (const text of [header, notice]) {
    assert.ok(text.includes('Permission to use, copy, modify, and/or distribute this software for any'), 'Lucide ISC permission notice')
    assert.ok(text.includes('Copyright (c) 2013-2023 Cole Bemis'), 'Feather MIT copyright')
    assert.equal(text.split('Permission is hereby granted, free of charge').length - 1, 2, 'two MIT permission notices: DeepSeek and Feather')
    assert.ok(text.includes('lucide-static 0.460.0'))
  }
})
function await_import_icons() {
  const p = join(ROOT, 'src/icons.generated.mjs')
  assert.ok(existsSync(p), 'src/icons.generated.mjs exists')
  const ctx = vm.createContext({})
  vm.runInContext(readFileSync(p, 'utf8').replaceAll('export const ', 'var '), ctx)
  return { ICONS: ctx.ICONS, ICON_SOURCE: ctx.ICON_SOURCE }
}
void sha
