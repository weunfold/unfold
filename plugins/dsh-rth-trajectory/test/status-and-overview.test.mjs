// Cancellation meaning, the lifetime-qualified playback join, the exact-status guard, and the timed
// overview (clock+lifetime groups, producer timestamps only, unknowns without invented length).
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { MindStream } from '../../dsh-mind/src/mind.mjs'
import { buildRthTrajectory } from '../src/adapter.mjs'
import * as S from '../src/status.mjs'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const L = await import('../src/clock-layout.mjs').catch(() => ({}))
const host = (id, kind, t, ref = {}, episode = 'e', observed = t) => ({ id, kind, lane: 'host', ref: { host_life: 'h1', ...ref }, clock: { t_ms: t }, observed_at: observed, episode, sentence: kind, status: 'observed' })
const agent = (id, kind, tNs, ref = {}, episode = 'e', observed = tNs / 1e6) => ({ id, kind, lane: 'agentd', ref: { producer: 'g', ...ref }, clock: { t_ns: tNs }, observed_at: observed, episode, sentence: kind, status: 'observed' })
const build = (rows) => buildRthTrajectory({ rows, episodes: {} })
const cells = (b) => b.turns.flatMap((t) => t.groups.flatMap((g) => g.cells))
const reply = (b) => cells(b).find((c) => c.rthKind === 'rth')
const card = (b) => [...b.cards.values()][0]
const layout = (b) => {
  assert.equal(typeof L.layoutClockOverview, 'function', 'src/clock-layout.mjs exports layoutClockOverview')
  return L.layoutClockOverview(b.turns)
}
const START = host('s', 'response_started', 100, { response_id: 'R', request_id: 'q' })

// ------------------------------------------------------------------ a stop request is not an outcome
test('a stop request alone settles nothing and claims no interruption', () => {
  for (const stop of [host('x', 'cancel_written', 150, { response_id: 'R' }), agent('x', 'cancel', 150e6, { response_id: 'R' })]) {
    const b = build([START, agent('as', 'response_started', 90e6, { response_id: 'R', request_id: 'q' }), stop])
    assert.equal(reply(b).rthOutcome ?? null, null, stop.kind)
    assert.equal(S.stateOf(reply(b)), 'rth-pending', stop.kind)
    assert.ok(!card(b).chips.some((c) => /interrupt|cancelled/.test(c)), card(b).chips.join(','))
    assert.ok(card(b).chips.includes('stop requested'), 'kept as an attempted action')
  }
})

test('a stop request followed by a normal completion is Completed', () => {
  const b = build([START, host('x', 'cancel_written', 150, { response_id: 'R' }), host('d', 'response_done', 300, { response_id: 'R', status: 'completed' })])
  assert.equal(S.stateOf(reply(b)), 'complete')
  assert.ok(!card(b).chips.some((c) => /cancelled/.test(c)))
})

test('a confirmed local cancellation (the host marked the reply cancelled) is Cancelled', () => {
  const b = build([START, host('x', 'response_cancelled', 150, { response_id: 'R' })])
  assert.equal(S.stateOf(reply(b)), 'rth-cancelled')
  assert.ok(card(b).chips.includes('⚡ cancelled (host)'))
})

test('cleared playback while generation continues: output cleared, reply not cancelled', () => {
  const b = build([
    START,
    host('p1', 'playback', 120, { response_id: 'R', phase: 'started' }),
    host('p2', 'playback', 160, { response_id: 'R', phase: 'cleared' }),
    agent('as', 'response_started', 90e6, { response_id: 'R', request_id: 'q' }),
    agent('d', 'agent_delta', 170e6, { response_id: 'R' }),
  ])
  assert.notEqual(S.stateOf(reply(b)), 'rth-cancelled')
  assert.ok(card(b).chips.includes('playback cleared'))
  assert.ok(!card(b).chips.some((c) => /cancelled|interrupted/.test(c)))
})

test('through real Mind rows, a host stop that was only sent stays pending', () => {
  const mind = new MindStream({ limit: 50 })
  const ev = (o) => ({ via: 'host journal', file: '/x/rth-host.jsonl', offset: o, length: 1 })
  mind.host({ kind: 'started', pid: 1, t_ms: 0 }, ev(0))
  mind.host({ kind: 'response_started', response_id: 'R', request_id: 'q', t_ms: 10 }, ev(1))
  mind.host({ kind: 'cancel_written', response_id: 'R', t_ms: 20 }, ev(2))
  const b = buildRthTrajectory({ rows: mind.snapshot().rows })
  assert.equal(S.stateOf(reply(b)), 'rth-pending')
})

test('the built renderer labels a stop-request-only reply pending, not cancelled', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-traj-overview-'))
  execFileSync(process.execPath, [join(ROOT, 'build.mjs'), '--out', join(dir, 'client.js')], { stdio: 'pipe' })
  const bundle = readFileSync(join(dir, 'client.js'), 'utf8')
  rmSync(dir, { recursive: true, force: true })
  const fn = (name) => bundle.match(new RegExp(`(?<=\\n\\t\\t)function ${name}\\([^)]*\\) \\{[\\s\\S]*?\\n\\t\\t\\}`))[0]
  const ctx = vm.createContext({ RTH: S })
  vm.runInContext(`${fn('stateOf')}\n${fn('statusLabel')}`, ctx)
  const b = build([START, host('x', 'cancel_written', 150, { response_id: 'R' })])
  assert.equal(ctx.statusLabel(ctx.stateOf({ cell: reply(b) }), (k) => k), 'rth.status.pending')
})

test('status guard: exact known statuses only; anything else is unclassified, never success', () => {
  const st = (rthOutcome, rthKind = 'rth') => S.stateOf({ rthKind, rthOutcome })
  assert.equal(st('completed'), 'complete')
  assert.equal(st('cancelled'), 'rth-cancelled')
  assert.equal(st('failed'), 'error')
  for (const odd of ['incomplete', 'not_completed', 'Completed', 'completed_partial', 'cancel_failed', 'errorless']) assert.equal(st(odd), 'rth-other', odd)
  assert.equal(st('interrupted', 'command'), 'rth-other', 'agent restarted before a result: not a success, not classified')
  assert.equal(st('rejected', 'command'), 'error')
})

// ------------------------------------------------------------------ playback joins carry the lifetime
const PLAY = (life, a = 100, z = 1000, ids = ['p1', 'p2']) => [host(ids[0], 'playback', a, { response_id: 'R', phase: 'started', host_life: life }), host(ids[1], 'playback', z, { response_id: 'R', phase: 'finished', host_life: life })]
const DONE = host('d', 'response_done', 50, { response_id: 'R', status: 'completed' })
test('playback from another host process never feeds the reply card', () => {
  const b = build([host('s', 'response_started', 10, { response_id: 'R', request_id: 'q' }), DONE, ...PLAY('h2')])
  assert.equal(reply(b).rthPlayback, 'not reported')
  assert.ok(!card(b).chips.some((c) => /^speaking/.test(c)), card(b).chips.join(','))
  assert.ok(cells(b).some((c) => c.rthKind === 'speaking' && c.rthSpan === 'measured'), 'the h2 playback stays visible on its own')
})

test('same process gives the speaking chip; unknown lifetime or conflicting evidence gives none', () => {
  const same = build([host('s', 'response_started', 10, { response_id: 'R', request_id: 'q' }), DONE, ...PLAY('h1')])
  assert.ok(card(same).chips.includes('speaking 900 ms'), card(same).chips.join(','))
  const unknown = build([host('s', 'response_started', 10, { response_id: 'R', request_id: 'q', host_life: null }), { ...DONE, ref: { ...DONE.ref, host_life: null } }, ...PLAY(null)])
  assert.ok(!card(unknown).chips.some((c) => /^speaking/.test(c)))
  const twice = build([host('s', 'response_started', 10, { response_id: 'R', request_id: 'q' }), DONE, ...PLAY('h1'), host('p3', 'playback', 500, { response_id: 'R', phase: 'started' })])
  assert.ok(!card(twice).chips.some((c) => /^speaking/.test(c)))
})

// ------------------------------------------------------------------ timed overview contract
test('timeline: a 100 ms host reply in host Think and a 30 ms first-text interval in agent Think, independent groups', () => {
  const b = build([
    host('c', 'command_written', 90, { command_id: 'q' }),
    host('s', 'response_started', 100, { response_id: 'R', request_id: 'q' }),
    agent('a', 'command_admitted', 900000000, { command_id: 'q', status: 'admitted' }),
    agent('as', 'response_started', 905000000, { response_id: 'R', request_id: 'q' }),
    agent('f', 'agent_delta', 930000000, { response_id: 'R' }),
    host('d', 'response_done', 200, { response_id: 'R', status: 'completed' }, 'e', 10001),
  ].map((r) => (r.id === 's' ? { ...r, observed_at: 10000 } : r)))
  const g = layout(b).groups
  const hostG = g.find((x) => x.clock === 'host' && x.life === 'h1')
  const agentG = g.find((x) => x.clock === 'agent' && x.life === 'g')
  assert.ok(hostG && agentG, 'one group per (clock, lifetime)')
  const hr = hostG.items.find((i) => i.kind === 'rth')
  assert.equal(hr.lane, 1)
  assert.equal(hr.end - hr.start, 100, 'host-observed reply: 100 ms, unaffected by observation delay')
  assert.match(hr.name, /host-observed reply/)
  const ar = agentG.items.find((i) => i.kind === 'rth')
  assert.equal(ar.lane, 1)
  assert.equal(ar.end - ar.start, 30)
  assert.equal(ar.name, 'first text received')
  assert.ok(!hostG.items.some((i) => i.name === 'first text received'), 'never drawn inside the host bar')
  for (const grp of g) {
    assert.match(grp.axisLabel, /relative to first retained event/)
    assert.ok(grp.ticks.length >= 2)
    assert.equal(grp.origin, Math.min(...grp.items.flatMap((i) => [i.start, i.end].filter((v) => v != null))))
  }
})

test('timeline: varying only observed_at changes no duration and no relative position inside a group', () => {
  const rows = (o) => [
    host('s', 'response_started', 100, { response_id: 'R', request_id: 'q' }, 'e', o[0]),
    host('d', 'response_done', 200, { response_id: 'R', status: 'completed' }, 'e', o[1]),
    host('p1', 'playback', 120, { response_id: 'R', phase: 'started' }, 'e', o[2]),
    host('p2', 'playback', 180, { response_id: 'R', phase: 'finished' }, 'e', o[3]),
    host('m', 'mute_applied', 150, {}, null, o[4]),
  ]
  const shape = (o) => layout(build(rows(o))).groups.map((g) => [g.clock, g.life, g.items.map((i) => [i.kind, i.x0, i.x1, i.end == null ? null : i.end - i.start]).sort()])
  const base = shape([1, 2, 3, 4, 5])
  assert.deepEqual(shape([10000, 10001, 10002, 10003, 10004]), base, 'delayed batch')
  assert.deepEqual(shape([5, 4, 3, 2, 1e12]), base, 'reordered and wall-clock jump')
})

test('timeline: mixed clocks never pair; restarts are separate groups; nothing crosses a group', () => {
  const b = build([
    host('s', 'response_started', 100, { response_id: 'R', request_id: 'q' }, 'e1'),
    agent('x', 'response_done', 5e6, { response_id: 'R', status: 'completed' }, 'e1'),
    host('s2', 'response_started', 10, { response_id: 'R2', request_id: 'q2', host_life: 'h2' }, 'e2'),
    host('d2', 'response_done', 40, { response_id: 'R2', status: 'completed', host_life: 'h2' }, 'e2'),
  ])
  const g = layout(b).groups
  assert.ok(g.find((x) => x.clock === 'host' && x.life === 'h1') && g.find((x) => x.clock === 'host' && x.life === 'h2'))
  const r1 = g.find((x) => x.life === 'h1').items.find((i) => i.kind === 'rth')
  assert.equal(r1.end, null, 'an agent end never closes a host bar')
  for (const grp of g) for (const i of grp.items) assert.ok(i.x0 >= -1e-9 && i.x0 <= 1 + 1e-9 && (i.x1 == null || (i.x1 >= i.x0 && i.x1 <= 1 + 1e-9)))
})

test('timeline: unknowns keep no invented length (open, end-only, backwards, missing, non-finite, duplicate, zero)', () => {
  const open = layout(build([START]))
  const o = open.groups[0].items.find((i) => i.kind === 'rth')
  assert.equal(o.end, null)
  assert.equal(o.shape, 'start')
  const endOnly = layout(build([host('p2', 'playback', 900, { response_id: 'R', phase: 'finished' })]))
  assert.equal(endOnly.groups[0].items[0].shape, 'end')
  const back = layout(build([host('p1', 'playback', 900, { response_id: 'R', phase: 'started' }), host('p2', 'playback', 100, { response_id: 'R', phase: 'finished' })]))
  assert.ok(back.groups[0].items.every((i) => i.end == null || i.end >= i.start), 'clock went backwards: no span')
  const nonFinite = layout(build([{ ...START, clock: { t_ms: Number.NaN } }]))
  assert.equal(nonFinite.groups.length, 0)
  assert.ok(Object.keys(nonFinite.unplaced).length >= 1, 'listed as unplaced, not pinned to zero')
  const unknownLife = layout(build([{ ...START, ref: { ...START.ref, host_life: null } }]))
  assert.equal(unknownLife.groups.length, 0, 'unknown identity is never a guessed group')
  const dup = layout(build([host('p1', 'playback', 100, { response_id: 'R', phase: 'started' }), host('p3', 'playback', 300, { response_id: 'R', phase: 'started' }), host('p2', 'playback', 900, { response_id: 'R', phase: 'finished' })]))
  assert.ok(!dup.groups.flatMap((g) => g.items).some((i) => i.kind === 'speaking' && i.end != null), 'duplicate starts: no span')
  const zero = layout(build([host('p1', 'playback', 500, { response_id: 'R', phase: 'started' }), host('p2', 'playback', 500, { response_id: 'R', phase: 'finished' })]))
  const z = zero.groups[0].items.find((i) => i.kind === 'speaking')
  assert.equal(z.shape, 'zero', 'a genuine zero-length interval is a measured-0 tick, distinct from missing')
})

test('timeline: the built view uses the clock overview for Duration and keeps sequence as the alternative', () => {
  const view = readFileSync(join(ROOT, 'src/view.js'), 'utf8')
  assert.match(view, /actualDuration \? \(0, react_jsx_runtime\.jsx\)\(RTH\.ClockOverview/)
  assert.match(view, /const timelineMode = "sequence";/, 'upstream\'s timeline only in sequence mode')
  assert.doesNotMatch(view, /"actual"|"duration"|"time"/)
})
