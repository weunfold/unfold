// RTH trajectory adapter: taxonomy mapping, folding, and the honesty rules for spans, driven
// through the Mind path (EventWindow -> MindStream -> public rows).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import { MindStream } from '../../dsh-mind/src/mind.mjs'
import { DEFAULT_GROUPS, buildRthTrajectory, filterTurns, pairSpan, shortPaths, turnCard } from '../src/adapter.mjs'
import { KINDS, LANE, kindOf } from '../src/taxonomy.mjs'

const catalog = loadCatalog()
function rig(boot = 'g') {
  let seq = 0
  let t = 1000
  const window = new EventWindow(catalog, { limit: 2000 })
  const mind = new MindStream({ limit: 2000 })
  window.onChange((k, p) => {
    if (k === 'added') mind.agentd(p, { via: 'rth-events observer feed' })
    if (k === 'updated') mind.agentdPatch(p)
  })
  mind.setLink('attached', 'observer')
  const r = {
    agent: (kind, f = {}) => { seq++; window.push({ v: 2, id: `${boot}:${seq}`, producer: boot, seq, t_ns: seq * 10_000_000, source: 'guest', kind, ...f }) },
    host: (kind, f = {}) => { t += 100; const { offset, ...rest } = f; mind.host({ kind, t_ms: 't_ms' in f ? f.t_ms : t, ...rest }, { via: 'host journal', file: '/x/rth-host.jsonl', offset: offset ?? t, length: 1 }) },
    build() {
      const snap = mind.snapshot()
      return buildRthTrajectory({ rows: snap.rows, episodes: Object.fromEntries(snap.episodes.map((e) => [e.id, e])) })
    },
  }
  r.host('started', { pid: 4242, t_ms: 0 })
  return r
}
const cells = (b) => b.turns.flatMap((t) => t.groups.flatMap((g) => g.cells))
const one = (b, kind) => cells(b).find((c) => c.rthKind === kind)

test('taxonomy: every kind has a label, an icon, light and dark colours, a lane and a group; colour is never alone', () => {
  for (const [k, v] of Object.entries(KINDS)) {
    assert.ok(v.label && v.short && v.icon, k)
    assert.equal(v.color.length, 2, k)
    assert.ok(Number.isInteger(v.lane) && v.lane >= 0 && v.lane <= 4, k)
    assert.ok(['you', 'rth', 'voice', 'control', 'signals', 'faults'].includes(v.group), k)
    assert.ok(['user', 'message', 'context'].includes(v.upstream), `${k} rides on an upstream kind`)
  }
  const m = (lane, kind, ref = {}, status = 'observed') => kindOf({ lane, kind, ref, status })
  assert.equal(m('host', 'local_vad_onset'), 'vad')
  assert.equal(m('host', 'echo_gate'), 'echo')
  assert.equal(m('host', 'speaker_cleared'), 'interrupt')
  assert.equal(m('host', 'response_cancelled'), 'interrupt')
  assert.equal(m('host', 'playback'), 'speaking')
  assert.equal(m('host', 'mute_applied'), 'mic')
  assert.equal(m('host', 'call_devices_opened'), 'call')
  assert.equal(m('host', 'link_up'), 'link')
  assert.equal(m('host', 'provider_signal'), 'signal')
  assert.equal(m('agentd', 'decision'), 'decision')
  assert.equal(m('agentd', 'rejected'), 'fault')
  assert.equal(m('agentd', 'log', { level: 'error' }), 'fault')
  assert.equal(m('agentd', 'log', { level: 'info' }), 'signal')
  assert.equal(m('agentd', 'media_stored'), 'media')
  assert.equal(m('mind', 'journal_replaced', {}, 'gap'), 'gap')
  assert.equal(m('agentd', 'response_started'), null, 'reply parts build the one RTH record')
  assert.equal(m('agentd', 'input_transcript'), null, 'transcripts build the one YOU record')
})

test('a typed turn: YOU, COMMAND, DECISION and RTH records in one turn; the RTH record uses Mind\'s reply metrics', () => {
  const r = rig()
  r.host('command_written', { command_id: 'q1' })
  r.agent('command_admitted', { command_id: 'q1', generation: 1, status: 'admitted' })
  r.agent('decision', { decision_id: 'd1', trigger_event: 'x', candidates: ['respond', 'wait'], selected: 'respond', policy: 'rule:always', latency_ns: 1, episode_id: 'e', training_eligibility: 'unknown' })
  r.host('response_started', { response_id: 'R1', request_id: 'q1' })
  r.agent('response_started', { response_id: 'R1', request_id: 'q1' })
  r.agent('agent_delta', { response_id: 'R1', text: 'Oi! Tudo bem.' })
  r.agent('agent_done', { response_id: 'R1', status: 'completed' })
  r.host('response_done', { response_id: 'R1', status: 'completed' })
  r.host('command_finished', { command_id: 'q1', response_id: 'R1', status: 'completed' })
  const b = r.build()
  const turn = b.turns.find((t) => t.turn === 1)
  assert.ok(turn, 'a numbered turn')
  const kinds = turn.groups[0].cells.map((c) => c.rthKind)
  assert.ok(kinds.includes('rth') && kinds.includes('command'), kinds.join(','))
  assert.ok(one(b, 'decision'), 'the decision is its own record (in the section its episode puts it)')
  const reply = one(b, 'rth')
  assert.equal(reply.kind, 'message')
  assert.equal(reply.rthSpan, 'measured')
  assert.equal(reply.rthClock, 'rth-host')
  assert.equal(reply.timeSeconds, 0.1, 'reply duration on the rth-host clock (response_started -> response_done)')
  assert.ok(reply.rthTiming.some((s) => s.name === 'first text received' && s.ms > 0 && s.clock === 'agent'))
  assert.ok(reply.rthTiming.some((s) => s.name === 'provider time to first token' && s.ms === null), 'provider TTFT stays not measured')
  assert.equal(reply.assistantMetrics, undefined, 'no upstream TTFT/decoding split is fed (decoding is not established)')
  const cmd = one(b, 'command')
  assert.equal(cmd.rthSpan, 'measured')
  assert.match(b.cards.get(1).chips.join(' '), /first text/)
  assert.equal(b.strip.tokens, 'not linked yet')
})

test('honesty: an ambiguous reply id gets no width and no status; a span never crosses producers or lifetimes', () => {
  const r = rig()
  r.host('response_started', { response_id: 'R', request_id: 'q1', t_ms: 100 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 200 })
  r.host('started', { pid: 2, t_ms: 0 })
  r.host('response_started', { response_id: 'R', request_id: 'q2', t_ms: 300 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 600 })
  const reply = one(r.build(), 'rth')
  assert.equal(reply.rthSpan, 'unknown')
  assert.equal(reply.timeSeconds, null)
  assert.match(reply.rthWhy, /ambiguous/)
  assert.match(turnCard(1, [reply]).chips.join(' '), /timing not attributed/)
  const h = (life, t) => ({ lane: 'host', ref: { host_life: life }, clock: { t_ms: t } })
  const a = (p, ns) => ({ lane: 'agentd', ref: { producer: p }, clock: { t_ns: ns } })
  assert.equal(pairSpan(h('host-1', 100), a('g', 2e8)).ms, null, 'never host vs agent')
  assert.equal(pairSpan(h('host-1', 100), h('host-2', 300)).ms, null, 'never across host lifetimes')
  assert.equal(pairSpan(h(null, 100), h(null, 300)).ms, null, 'never an unestablished lifetime')
  assert.equal(pairSpan(h('host-1', 100), { lane: 'host', ref: { host_life: 'host-1' }, clock: { t_ms: null } }).ms, null, 'never a missing time')
  assert.equal(pairSpan(a('g', 1e8), a('h', 3e8)).ms, null, 'never across agent boots')
  assert.equal(pairSpan(h('host-1', 100), h('host-1', 350)).ms, 250)
})

test('a voice reply: YOU · VOICE folds partials and stays unjoined; playback is a host report with its own measured span; an unlinked speaker clear is observed, not causal', () => {
  const r = rig()
  r.host('local_vad_onset', { stream_id: 'mic-1', level_dbfs: -20, action: 'nothing_audible', audible: false })
  r.agent('input_transcript', { text: 'ola', utterance_id: 'u1', final: false })
  r.agent('input_transcript', { text: 'ola tudo bem', utterance_id: 'u1', final: true })
  r.host('response_started', { response_id: 'V1', request_id: 'mic-1' })
  r.agent('response_started', { response_id: 'V1', request_id: 'mic-1' })
  r.host('playback', { response_id: 'V1', phase: 'started', played_samples: 512 })
  r.host('speaker_cleared', { reason: 'provider_interrupted', dropped_samples: 4800 })
  r.host('playback', { response_id: 'V1', phase: 'finished', played_samples: 24000 })
  r.host('response_done', { response_id: 'V1', status: 'completed' })
  const b = r.build()
  const speaking = one(b, 'speaking')
  assert.equal(speaking.rthSpan, 'measured')
  assert.equal(speaking.rthClock, 'rth-host')
  assert.match(speaking.text, /not proof it was heard/)
  assert.ok(one(b, 'interrupt'), 'the speaker clear is its own record')
  const card = [...b.cards.values()][0]
  assert.equal(b.cards.size, 1, 'one numbered turn: the reply (Mind keeps the utterance in its own episode)')
  assert.ok(!card.chips.some((c) => /interrupted|cancelled/.test(c)), 'a speaker clear without a reply id is not this reply\'s interruption')
  assert.ok(!card.chips.includes('voice'), 'a stream id is not an utterance-to-reply association')
  assert.ok(card.chips.some((c) => /^speaking /.test(c)), 'speaking chip from the measured playback span of this reply id')
  assert.match(one(b, 'voice').rthAssociation, /not available/)
  assert.equal(one(b, 'interrupt').rthLane, LANE.SPEAK)
  assert.ok(one(b, 'vad'))
})

test('noise control: SIGNAL and ECHO GUARD rows fold into one quiet summary record per section; Signals on shows the single rows instead', () => {
  const r = rig()
  for (let i = 0; i < 5; i++) r.host('provider_signal', { signal: 'voice_activity_start', response_id: null })
  for (let i = 0; i < 3; i++) r.host('echo_gate', { action: 'nothing_audible', audible: false })
  const full = r.build()
  assert.ok(!DEFAULT_GROUPS.includes('signals'), 'calm by default: Signals starts off')
  const b = { turns: filterTurns(full.turns, new Set(DEFAULT_GROUPS)) }
  const folds = cells(b).filter((c) => c.rthFolded)
  assert.ok(folds.length >= 1)
  const total = folds.reduce((n, c) => n + c.rthFolded.length, 0)
  assert.ok(total >= 8, `folded ${total}`)
  assert.match(folds.map((c) => c.text).join(' '), /signals?.*echo check/)
  assert.ok(cells(b).every((c) => !c.rthQuiet), 'no loose signal rows while Signals is off')
  assert.equal(full.counts.signals, 8, 'the chip count counts each signal once')
  const on = { turns: filterTurns(full.turns, new Set([...DEFAULT_GROUPS, 'signals'])) }
  assert.equal(cells(on).filter((c) => c.rthFolded).length, 0, 'the summary gives way to the single rows')
  assert.equal(cells(on).filter((c) => c.rthQuiet).length, 8)
})

test('filters drop whole groups and keep records whole; GAP and FAULT are their own records', () => {
  const r = rig()
  r.agent('rejected', { reason: 'bad frame' })
  r.host('mute_applied', { muted: true, generation: 3 })
  const b = r.build()
  assert.ok(one(b, 'fault'))
  assert.ok(one(b, 'mic'))
  const onlyFaults = filterTurns(b.turns, new Set(['faults']))
  assert.ok(onlyFaults.flatMap((t) => t.groups.flatMap((g) => g.cells)).every((c) => KINDS[c.rthKind].group === 'faults'))
  const all = filterTurns(b.turns, null)
  assert.deepEqual(cells({ turns: all }).map((c) => c.recordId), cells(b).filter((c) => !c.rthFolded).map((c) => c.recordId), 'no filter = every group on: single rows, no summaries')
})

test('identity: record ids are stable and unique, indexes are sequential', () => {
  const r = rig()
  r.host('command_written', { command_id: 'q9' })
  r.host('response_started', { response_id: 'R9', request_id: 'q9' })
  r.host('response_done', { response_id: 'R9', status: 'completed' })
  const a = r.build()
  const b = r.build()
  assert.deepEqual(cells(a).map((c) => c.recordId), cells(b).map((c) => c.recordId))
  const ids = cells(a).map((c) => c.recordId)
  assert.equal(new Set(ids).size, ids.length)
  assert.deepEqual(cells(a).map((c) => c.index), cells(a).map((_, i) => i + 1))
})

test('redaction: local paths reach no record field (row text, Raw/inspector details, folded rows, search and copy sources)', () => {
  const window = new EventWindow(catalog, { limit: 50 })
  const mind = new MindStream({ limit: 50 })
  window.onChange((k, p) => { if (k === 'added') mind.agentd(p, { via: 'rth-events observer feed' }) })
  const file = '/Users/someone/Library/Application Support/rth/app/bot/rth-host.jsonl'
  const home = '/Users/someone/workspace/rth/.unfold-home/host/rth-host.jsonl'
  mind.host({ kind: 'started', pid: 1, t_ms: 0 }, { via: 'host journal', file, offset: 0, length: 1 })
  mind.marker('journal_replaced', `Host journal: ${home}: another file now has this path (inode 1 -> 2); reading it again from its start`)
  mind.host({ kind: 'provider_signal', signal: 'x', t_ms: 5 }, { via: 'host journal', file, offset: 9, length: 1 })
  const snap = mind.snapshot()
  assert.ok(JSON.stringify(snap.rows).includes('/Users/someone'), 'the fixture really carries local paths')
  const b = buildRthTrajectory({ rows: snap.rows, episodes: {} })
  const json = JSON.stringify(b.turns)
  assert.ok(!json.includes('/Users/'), json.match(/.{40}\/Users\/.{40}/)?.[0])
  assert.ok(!json.includes('someone'))
  assert.match(json, /…\/rth-host\.jsonl/)
  const gap = one(b, 'gap')
  assert.ok(gap, 'the replaced journal is a GAP record')
  assert.match(gap.text, /^Host journal: …\/rth-host\.jsonl: another file/)
  assert.ok(!json.includes('Application Support'), 'a path with a space is scrubbed whole')
  assert.equal(shortPaths(`journal ${file}: replaced`), 'journal …/rth-host.jsonl: replaced')
  assert.equal(shortPaths('heard: it is 3/4 done'), 'heard: it is 3/4 done', 'prose with slashes is left alone')
})

test('real-shaped fixture (host journal of a recorded voice session, synthetic text): replies as turns, unjoined utterances, linked barge-in, measured and unknown spans', async () => {
  const { readFileSync } = await import('node:fs')
  const mind = new MindStream({ limit: 2000 })
  const file = '/Users/someone/rth/app/bot/rth-host.jsonl'
  const recs = readFileSync(new URL('../fixtures/voice-session-host.jsonl', import.meta.url), 'utf8').trim().split('\n').map((l) => JSON.parse(l))
  mind.host({ kind: 'started', pid: 1, t_ms: 0 }, { via: 'host journal', file, offset: 0, length: 1 }) // the replaying host's own lifetime record
  recs.forEach((r, i) => mind.host(r, { via: 'host journal', file, offset: 100 + i * 100, length: 99 }))
  const b = buildRthTrajectory({ rows: mind.snapshot().rows })
  const numbered = b.turns.filter((t) => t.turn !== null)
  assert.equal(numbered.length, 12)
  assert.ok(numbered.every((t) => t.groups[0].title === 'Reply' && !b.cards.get(t.turn).chips.includes('voice')), 'host-heard transcripts carry no reply association')
  assert.ok(numbered.every((t) => !t.groups[0].cells.some((c) => c.rthKind === 'voice')), 'no utterance is moved into a reply turn')
  assert.equal(b.turns.filter((t) => t.groups[0].title === 'Heard (no reply linked)').length >= 1, true)
  const barge = numbered.filter((t) => b.cards.get(t.turn).chips.includes('⚡ cancelled (host)'))
  assert.equal(barge.length, 1, 'only the reply its own host cancellation names')
  assert.ok(b.cards.get(barge[0].turn).chips.includes('playback cleared'), 'its playback was cleared (output cleared, a separate fact)')
  assert.ok(barge[0].groups[0].cells.find((c) => c.rthKind === 'rth').rthCancelledBy.length >= 1)
  assert.ok([...b.cards.values()].filter((c) => c.chips.some((x) => /^speaking /.test(x))).length >= 2, 'speaking chips only from measured playback spans')
  const replies = numbered.map((t) => t.groups[0].cells.find((c) => c.rthKind === 'rth'))
  assert.equal(replies.filter((r) => r.rthSpan === 'measured').length, 11)
  assert.equal(replies.at(-1).rthSpan, 'unknown', 'the last reply, cut off by the end of the journal, has a start and no end')
  assert.equal(b.strip.avgFirstTextMs, null, 'host-only journal: first text is not measured (agent clock rows absent)')
  assert.ok(!JSON.stringify(b.turns).includes('/Users/'))
})
