import assert from 'node:assert/strict'
import { appendFileSync, existsSync, fstatSync as nodeFstatSync, fsyncSync as nodeFsyncSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync, writeSync as nodeWriteSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { createHash } from 'node:crypto'
import { Context } from '@deepseek-ai/cordis'
// The real dsh-events window and catalog, read-only: Mind consumes exactly what that plugin emits.
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import * as plugin from '../index.mjs'
import { loadDefinitions } from '../src/definitions.mjs'
import { freshness } from '../src/display.mjs'
import { JsonlTail } from '../src/journal.mjs'
import { HEARTBEAT_LEASE_MS, MindStream } from '../src/mind.mjs'
import { AGENTD_KINDS, CATEGORIES, HOST_KINDS, STATUSES, sentence } from '../src/sentences.mjs'
import { SuggestionLog } from '../src/suggestions.mjs'
import { needRth, rthFile } from '../../../test/rth-checkout.mjs'

const catalog = loadCatalog()
const BOOT = 'b0'
const P = `guest-${BOOT}`
const settle = () => new Promise((r) => setImmediate(r))

/** The real path: envelopes through a real EventWindow, its changes into a MindStream. */
function rig({ clock } = {}) {
  const window = new EventWindow(catalog, { limit: 500 })
  let t = 1_000_000
  const now = clock ?? (() => t)
  const mind = new MindStream({ now })
  window.onChange((change, payload) => {
    if (change === 'added') mind.agentd(payload, { via: 'rth-events observer feed' })
    else if (change === 'updated') mind.agentdPatch(payload)
    else if (change === 'status') mind.agentd(payload, { via: 'rth-events observer feed' })
    else if (change === 'link') mind.setLink(payload.state, payload.detail)
  })
  let seq = 0
  const send = (kind, fields = {}, { producer = P, skip = 0 } = {}) => {
    seq += 1 + skip
    const env = { v: 2, id: `${producer}:${seq}`, producer, seq, t_ns: seq * 1_000_000, source: 'guest', kind, ...fields }
    window.push(env)
    return env
  }
  const link = (state, detail = null) => window.emit('link', { state, detail })
  return { window, mind, send, link, advance: (ms) => (t += ms), rows: () => mind.rows, eps: () => mind.snapshot().episodes }
}

function typedEpisode(r, cmd = 'cmd-7', rid = 'r-12') {
  r.send('command_admitted', { command_id: cmd, generation: 4, status: 'admitted' })
  const d = r.send('decision', { decision_id: `d-${cmd}`, episode_id: 'host-attach-1', trigger_event: `host:${cmd}`, context_rev: 1, candidate_schema: 'v0', candidates: ['respond', 'wait'], selected: 'respond', policy: 'rule:always-respond@v0', latency_ns: 2_500_000, training_eligibility: 'unknown' })
  r.send('response_started', { cause: d.id, response_id: rid, request_id: cmd })
  r.send('agent_delta', { cause: d.id, response_id: rid, text: 'Oi, ' })
  r.send('agent_delta', { cause: d.id, response_id: rid, text: 'tudo bem?' })
  r.send('agent_done', { cause: d.id, response_id: rid, status: 'completed' })
  r.send('command_admitted', { command_id: cmd, generation: 4, status: 'completed', response_id: rid, decision_id: `d-${cmd}` })
  return d
}

// ── episodes ───────────────────────────────────────────────────────────────────────────────

test('a typed request, its decision and its reply group into one episode, completed, on one clock', () => {
  const r = rig()
  r.link('attached', 'observer')
  r.send('hello', { protocol: 2, agent: 'rth-agentd', version: 't', boot_id: BOOT, utc_ms_at_origin: 1_760_000_000_000, journal: '/rth/journal.jsonl' })
  typedEpisode(r)
  const eps = r.eps()
  assert.equal(eps.length, 1, JSON.stringify(eps.map((e) => e.title)))
  const ep = eps[0]
  assert.equal(ep.title, 'Typed request cmd-7 → decided respond → reply r-12')
  assert.equal(ep.status, 'completed')
  assert.match(ep.because, /outcome receipt/)
  // command_admitted ×2, decision, response_started, one coalesced delta row, agent_done
  assert.deepEqual(ep.rows.map((x) => x.kind), ['command_admitted', 'decision', 'response_started', 'agent_delta', 'agent_done', 'command_admitted'])
  assert.equal(ep.duration_ms, 6, 'first to last row on the agentd clock (1 ms per seq here)')
  assert.equal(ep.duration_basis, 'first to last row, one agentd clock')
  const delta = r.rows().find((x) => x.kind === 'agent_delta')
  assert.equal(delta.count, 2)
  assert.match(delta.sentence, /“Oi, tudo bem\?” \(2 parts\)/)
  assert.ok(delta.clock.wall_ms_estimate > 1_760_000_000_000, 'wall time is an estimate from Hello')
  assert.match(delta.clock.wall_basis, /^estimate/)
  // Hello is a system row, never part of an episode.
  assert.equal(r.rows().find((x) => x.kind === 'hello').episode, null)
})

test('two spoken replies on the same microphone stream (request_id mic-1) stay separate episodes', () => {
  const r = rig()
  r.link('attached')
  const chunk = r.send('user_audio', { stream_id: 'mic-1', chunk: 0, sample_offset: 0, sample_rate: 16000, format: 's16le', data: Buffer.alloc(640).toString('base64') })
  for (const n of [1, 2]) {
    // Both decisions name the same audio chunk as cause: a stream row is not an episode key.
    const d = r.send('decision', { cause: chunk.id, decision_id: `dv${n}`, episode_id: 'host-attach-1', trigger_event: chunk.id, candidates: ['respond'], selected: 'respond', policy: 'provider:gemini-auto-vad@v1', latency_ns: 0, training_eligibility: 'unknown' })
    r.send('response_started', { cause: d.id, response_id: `v${n}`, request_id: 'mic-1' })
    r.send('agent_done', { cause: d.id, response_id: `v${n}`, status: n === 1 ? 'cancelled' : 'completed' })
  }
  const eps = r.eps()
  assert.equal(eps.length, 2)
  assert.deepEqual(eps.map((e) => e.status), ['stopped', 'completed'])
  assert.match(eps[0].title, /^Spoken turn on mic-1 → decided respond → reply v1$/)
  assert.match(r.rows().find((x) => x.kind === 'response_started').sentence, /the microphone stream, not one utterance/)
})

test('no causal link and no grouping without an explicit id', () => {
  const r = rig()
  r.link('attached')
  // Same text, adjacent in time, no shared id and no cause: two episodes, no links.
  r.send('response_started', { response_id: 'a', request_id: 'x-1' })
  r.send('response_started', { response_id: 'b', request_id: 'x-2' })
  // A transcript that names a reply WITHOUT identity evidence (association unknown) does not join it.
  r.send('input_transcript', { text: 'oi', utterance_id: 'u1', association: 'unknown', response_id: 'a' })
  const eps = r.eps()
  const of = (rid) => eps.find((e) => e.rows.some((x) => r.mind.byId.get(x.id).fields.response_id === rid && x.kind === 'response_started'))
  assert.notEqual(of('a').id, of('b').id)
  const tr = r.rows().find((x) => x.kind === 'input_transcript')
  assert.notEqual(tr.episode, of('a').id, 'association unknown is not evidence')
  const defs = loadDefinitions()
  const da = r.mind.detail(r.rows()[0].id, defs)
  assert.deepEqual(da.chain.parents, [])
  assert.equal(da.chain.unresolved, null)
  assert.deepEqual(da.chain.children, [])
  // With an explicit cause the link appears both ways, and an unseen cause is named, not guessed.
  const p = r.send('decision', { decision_id: 'd9', trigger_event: 'host-zz:3', cause: 'host-zz:3', candidates: ['respond'], selected: 'respond', policy: 'rule', latency_ns: 1, episode_id: 'e', training_eligibility: 'unknown' })
  const c = r.send('response_started', { cause: p.id, response_id: 'c', request_id: 'x-3' })
  const dp = r.mind.detail(p.id, defs)
  assert.deepEqual(dp.chain.children.map((x) => x.event_id), [c.id])
  assert.equal(dp.chain.unresolved.event_id, 'host-zz:3')
  assert.match(dp.chain.unresolved.reason, /host frame/)
  assert.equal(r.mind.detail(c.id, defs).chain.parents[0].event_id, p.id)
})

test('host journal and agent rows join only through the ids they both carry', () => {
  const mind = new MindStream({ now: () => 1 })
  mind.host({ kind: 'command_written', command_id: 'dsh-1', envelope: 'host-h:9', t_ms: 10 }, { via: 'rth-host --log file', file: '/x', offset: 0 })
  mind.host({ kind: 'local_vad_onset', stream_id: 'mic-1', level_dbfs: -12, audible: false, action: 'nothing_audible', t_ms: 11 }, { via: 'rth-host --log file', file: '/x', offset: 50 })
  mind.agentd({ v: 2, id: `${P}:1`, producer: P, seq: 1, t_ns: 5, source: 'guest', kind: 'response_started', response_id: 'r1', request_id: 'dsh-1' }, { via: 'rth-events observer feed' })
  mind.host({ kind: 'command_finished', command_id: 'dsh-1', response_id: 'r1', status: 'completed', t_ms: 900 }, { via: 'rth-host --log file', file: '/x', offset: 99 })
  const eps = mind.snapshot().episodes
  assert.equal(eps.length, 1)
  assert.deepEqual(eps[0].lanes, ['host', 'agentd'])
  assert.equal(eps[0].status, 'completed')
  assert.equal(eps[0].duration_ms, null)
  assert.match(eps[0].duration_basis, /more than one clock/)
  assert.equal(mind.rows.find((x) => x.kind === 'local_vad_onset').episode, null, 'a speech candidate is not assigned to any request')
})

// ── plain language ─────────────────────────────────────────────────────────────────────────

test('every catalog wire kind and every host note kind renders one plain sentence, a category and a status', () => {
  const doc = JSON.parse(readFileSync(new URL('../../../catalog/events.v3.json', import.meta.url), 'utf8'))
  const wires = doc.events.map((e) => e.wire)
  assert.deepEqual(wires.filter((w) => !AGENTD_KINDS.includes(w)), [], 'add a sentence for these catalog kinds')
  for (const [lane, kinds] of [['agentd', wires], ['host', HOST_KINDS]]) {
    for (const kind of kinds) {
      const s = sentence({ lane, kind, fields: {} })
      assert.ok(s.known, `${lane}/${kind}`)
      assert.ok(CATEGORIES[s.category], `${lane}/${kind} category ${s.category}`)
      assert.ok(STATUSES.includes(s.status), `${lane}/${kind} status ${s.status}`)
      assert.doesNotMatch(s.sentence, /undefined|null|NaN|\[object/, `${lane}/${kind}: ${s.sentence}`)
      assert.ok(s.sentence.length > 10)
    }
  }
  assert.match(sentence({ lane: 'agentd', kind: 'response_started', fields: {} }).sentence, /Started reply unknown to request unknown/, 'missing data reads unknown')
  assert.equal(sentence({ lane: 'agentd', kind: 'future_kind', fields: {} }).known, false)
})

test('sentences say what was observed, never more', () => {
  const s = (lane, kind, fields, extra) => sentence({ lane, kind, fields, ...extra })
  const vad = s('host', 'local_vad_onset', { stream_id: 'mic-1', level_dbfs: -12.2, audible: false, action: 'nothing_audible' })
  assert.equal(vad.category, 'perceive')
  assert.match(vad.sentence, /speech candidate, energy onset at -12\.2 dBFS/)
  assert.match(vad.sentence, /Not proof anyone spoke/)
  assert.doesNotMatch(vad.sentence, /the owner|you said|user spoke/i)
  const tr = s('agentd', 'input_transcript', { text: 'oi tudo bem', association: 'unknown' })
  assert.match(tr.sentence, /Provider transcribed “oi tudo bem” \(not final; speaker not identified; reply association unknown\)/)
  assert.match(s('agentd', 'user_audio', { stream_id: 'mic-1' }, { audio: { chunks: 3, ms: 60 } }).sentence, /not evidence of speech/)
  assert.match(s('agentd', 'playback', { phase: 'finished', response_id: 'r', played_samples: 10, dropped_samples: 0 }).sentence, /not proof it was heard/)
  assert.match(s('agentd', 'agent_audio', { response_id: 'r' }, { audio: { chunks: 2, ms: 40 } }).sentence, /generated, not heard/)
  const adm = s('agentd', 'command_admitted', { command_id: 'c-7', status: 'admitted', generation: 3 })
  assert.equal(adm.status, 'pending')
  assert.match(adm.sentence, /not yet a result/)
  assert.equal(s('agentd', 'command_admitted', { status: 'interrupted' }).status, 'unknown')
  assert.equal(s('agentd', 'command_submitted', { command_id: 'c' }).status, 'attempted')
  assert.equal(s('host', 'cancel_written', { response_id: 'r' }).status, 'attempted')
  assert.equal(s('agentd', 'agent_done', { status: 'cancelled' }).status, 'stopped')
  assert.equal(s('agentd', 'playback', { phase: 'cleared', reason: 'device_unavailable' }).status, 'failed')
  assert.match(s('host', 'mute_request', { muted: true, via: 'tray' }).sentence, /requested, not yet applied/)
  assert.match(s('agentd', 'decision', { selected: 'respond', candidates: ['respond', 'wait'], policy: 'rule' }).sentence, /trigger unknown/)
})

// ── redaction ──────────────────────────────────────────────────────────────────────────────

test('secrets never reach a row, an audit record, the evidence or the snapshot', () => {
  const r = rig()
  r.link('attached')
  const KEY = 'AIzaSyD-THIS-IS-A-FAKE-KEY-0123456789'
  const BEARER = 'abcdef0123456789secret'
  r.send('log', { level: 'info', msg: `gemini connect url=wss://x?key=${KEY} Authorization: Bearer ${BEARER} GEMINI_API_KEY=${KEY}` })
  r.send('hello', { protocol: 2, agent: 'a', version: 'v', boot_id: BOOT, utc_ms_at_origin: 1, journal: '/j', api_key: 'plain-secret-value', token: 'tok-123' })
  const mind = new MindStream({ now: () => 1 })
  mind.host({ kind: 'started', pid: 1, authorization: 'Bearer zzz-host-secret', t_ms: 0 }, { via: 'rth-host --log file', file: '/x', offset: 0 })
  const defs = loadDefinitions()
  const blobs = [
    JSON.stringify(r.mind.snapshot()),
    ...r.rows().map((x) => JSON.stringify(r.mind.detail(x.id, defs))),
    JSON.stringify(mind.snapshot()),
    JSON.stringify(mind.detail(mind.rows[0].id, defs)),
  ].join('\n')
  for (const secret of [KEY, BEARER, 'plain-secret-value', 'tok-123', 'zzz-host-secret']) assert.ok(!blobs.includes(secret), `leaked ${secret}`)
  assert.match(blobs, /\[redacted\]/)
  assert.ok(blobs.includes(BOOT), 'ids are kept')
})

// ── gaps and freshness ─────────────────────────────────────────────────────────────────────

test('stream loss writes a gap marker, turns an open episode unknown, and a reattach says nothing was replayed', () => {
  const r = rig()
  r.link('attached')
  r.send('command_admitted', { command_id: 'c1', generation: 1, status: 'admitted' })
  r.send('response_started', { response_id: 'r1', request_id: 'c1' })
  assert.equal(r.eps()[0].status, 'pending')
  r.advance(100)
  r.link('waiting', 500)
  const lost = r.rows().at(-1)
  assert.equal(lost.kind, 'link_lost')
  assert.equal(lost.source, 'UI')
  assert.equal(lost.status, 'gap')
  assert.match(lost.sentence, /Observer link lost \(waiting: 500\).*1 open episode\(s\) marked unknown/)
  assert.equal(r.eps()[0].status, 'unknown')
  assert.match(r.eps()[0].because, /outcome was not observed/)
  r.link('connecting')
  r.link('attached')
  const back = r.rows().at(-1)
  assert.equal(back.kind, 'link_resumed')
  assert.match(back.sentence, /not replayed/)
  assert.equal(r.eps()[0].status, 'unknown', 'a reattach alone revives nothing')
  r.advance(10)
  r.send('agent_done', { response_id: 'r1', status: 'completed' })
  assert.equal(r.eps()[0].status, 'completed', 'a real outcome receipt after the gap settles it')
})

test('a sequence gap from one producer is a marker naming the missing range; heartbeats and coalesced parts count', () => {
  const r = rig()
  r.link('attached')
  r.send('response_started', { response_id: 'r', request_id: 'q' }) // 1
  r.send('agent_delta', { response_id: 'r', text: 'a' }) // 2
  r.send('agent_delta', { response_id: 'r', text: 'b' }) // 3 (an `updated` patch)
  r.send('heartbeat', { uptime_ms: 1, attachments: 1 }) // 4 (a `status`)
  assert.equal(r.rows().filter((x) => x.kind === 'seq_gap').length, 0)
  r.send('agent_done', { response_id: 'r', status: 'completed' }, { skip: 2 }) // 7
  const gaps = r.rows().filter((x) => x.kind === 'seq_gap')
  assert.equal(gaps.length, 1)
  assert.match(gaps[0].sentence, /^2 frame\(s\) from guest-b0 \(seq 5\.\.6\) were not observed on rth-events observer feed/)
  assert.match(gaps[0].sentence, /Incomplete evidence/)
  assert.equal(gaps[0].episode, null)
  // A new producer (a new boot) starts its own sequence: no false gap.
  r.send('hello', { boot_id: 'b1' }, { producer: 'guest-b1', skip: 40 })
  assert.equal(r.rows().filter((x) => x.kind === 'seq_gap').length, 1)
})

test('a silent heartbeat past its lease is a stall marker, and its return is marked', () => {
  let t = 0
  const r = rig({ clock: () => t })
  r.link('attached')
  r.send('heartbeat', { uptime_ms: 1, attachments: 1 })
  assert.equal(r.mind.nextDeadline(), HEARTBEAT_LEASE_MS + 1)
  t = HEARTBEAT_LEASE_MS + 2
  r.mind.tick()
  assert.equal(r.rows().at(-1).kind, 'heartbeat_stale')
  r.mind.tick()
  assert.equal(r.rows().filter((x) => x.kind === 'heartbeat_stale').length, 1, 'one marker per stall')
  t += 3000
  r.send('heartbeat', { uptime_ms: 2, attachments: 1 })
  assert.equal(r.rows().at(-1).kind, 'heartbeat_resumed')
  assert.match(r.rows().at(-1).sentence, /about 8 s of silence/)
})

test('the panel expires its own claims: a silent stream and a silent agent turn stale with no new frame', () => {
  const link = { state: 'attached', detail: null, since: 0, attaches: 1, heartbeat: { at: 1000 }, lease_ms: 5000 }
  const base = { stream: 'open', lastStreamAt: 1000, heartbeatMs: 1000, offsetMs: 0, link }
  let f = freshness(base, 1500)
  assert.equal(f.stream.state, 'live')
  assert.equal(f.agent.state, 'live')
  assert.equal(f.nextChange, 4001, 'the panel redraws itself when the stream lease runs out')
  f = freshness(base, 4002)
  assert.equal(f.stream.state, 'stale')
  assert.equal(f.agent.state, 'unknown', 'nothing is live while the panel is not receiving')
  f = freshness({ ...base, lastStreamAt: 6500 }, 6600)
  assert.equal(f.agent.state, 'stale', 'the stream beats, but the agent heartbeat is past its lease')
  assert.equal(freshness({ ...base, stream: 'reconnecting' }, 1500).stream.state, 'lost')
  assert.equal(freshness({ ...base, link: { ...link, state: 'waiting', detail: 500 } }, 1500).agent.state, 'down')
  assert.equal(freshness({ ...base, offsetMs: 4500 }, 1600).agent.state, 'stale', 'judged on the server clock')
})

// ── suggestions ────────────────────────────────────────────────────────────────────────────

test('the suggestion store is append-only and idempotent by key', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-'))
  try {
    const log = new SuggestionLog(dir)
    const a = { key: 'sugg-0001-aaaa', actor: 'The owner', scope: { episode: 'ep-m3', event: 'guest-b0:3', title: 'Typed request c-7' }, text: 'A person would wait for me to finish the sentence.' }
    const first = log.append(a, 0)
    assert.equal(first.duplicate, false)
    assert.equal(first.record.kind, 'proposal')
    assert.match(first.record.effect, /^none/)
    const bytes1 = readFileSync(log.path)
    const again = log.append(a, 999)
    assert.equal(again.duplicate, true)
    assert.equal(again.record.at, first.record.at)
    assert.deepEqual(readFileSync(log.path), bytes1, 'a retry writes nothing')
    assert.throws(() => log.append({ ...a, text: 'something else' }), (e) => e.status === 409)
    log.append({ ...a, key: 'sugg-0002-bbbb', text: 'Second idea' })
    const bytes2 = readFileSync(log.path)
    assert.ok(bytes2.length > bytes1.length && bytes2.subarray(0, bytes1.length).equals(bytes1), 'the earlier bytes are untouched')
    assert.equal(bytes2.toString().trim().split('\n').length, 2)
    for (const bad of [{}, { ...a, key: 'short' }, { ...a, key: 'sugg-0003-cccc', text: '' }, { ...a, key: 'sugg-0004-dddd', text: 5 }, { ...a, key: 'sugg-0005-eeee', scope: {} }, { ...a, key: 'sugg-0006-ffff', text: 'x'.repeat(5000) }]) {
      assert.throws(() => log.append(bad), (e) => e.status === 400)
    }
    assert.deepEqual(readFileSync(log.path), bytes2, 'refused input writes nothing')
    const names = Object.getOwnPropertyNames(SuggestionLog.prototype)
    assert.deepEqual(names.filter((n) => /delete|remove|clear|update|rewrite|truncate|reset|forget|edit/i.test(n)), [])
    const reloaded = new SuggestionLog(dir)
    assert.equal(reloaded.append(a).duplicate, true, 'idempotency survives a restart')
    assert.deepEqual(reloaded.list({ episode: 'ep-m3' }).map((x) => x.key), ['sugg-0001-aaaa', 'sugg-0002-bbbb'])
    // A secret pasted into a suggestion is redacted before it is written.
    log.append({ ...a, key: 'sugg-0007-gggg', text: 'try GEMINI_API_KEY=AIzaSyFAKEFAKEFAKEFAKEFAKE00' })
    assert.ok(!readFileSync(log.path, 'utf8').includes('AIzaSyFAKE'))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

// ── suggestion durability ───────────────────────────────────────────────────────────────────

const S1 = { key: 'reviewer-after-crash', actor: 'reviewer', scope: { episode: 'ep-m1' }, text: 'Acknowledged means durable.' }
function withDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-d-'))
  try {
    return fn(dir, join(dir, 'suggestions.jsonl'))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

test('a torn last line: the next suggestion starts on its own line, survives a reload once, and the fragment is reported', () => withDir((dir, file) => {
  const TORN = '{"format":"rth.mind-suggestion/v1","key":"crash-'
  writeFileSync(file, TORN)
  const log = new SuggestionLog(dir)
  assert.deepEqual(log.damaged.map((d) => [d.kind, d.offset, d.length]), [['torn_tail', 0, TORN.length]])
  assert.equal(log.damaged[0].sha256, createHash('sha256').update(TORN).digest('hex'))
  assert.equal(log.append(S1).duplicate, false)
  const bytes = readFileSync(file)
  assert.ok(bytes.subarray(0, TORN.length).equals(Buffer.from(TORN)), 'the torn bytes are kept, never rewritten')
  assert.equal(bytes[TORN.length], 0x0a, 'a newline fences the fragment off before the new record')
  const reloaded = new SuggestionLog(dir)
  assert.deepEqual(reloaded.records.map((r) => r.key), ['reviewer-after-crash'], 'the acknowledged record survives the restart, once')
  assert.deepEqual(reloaded.damaged.map((d) => [d.kind, d.offset, d.length]), [['malformed_line', 0, TORN.length]], 'the fragment is still reported, not dropped silently')
  assert.equal(reloaded.append(S1).duplicate, true, 'a same-key retry after the restart is a duplicate')
  assert.deepEqual(readFileSync(file), bytes, 'and writes nothing')
}))

test('a complete record missing its final newline is kept, and so is the next one', () => withDir((dir, file) => {
  const first = new SuggestionLog(dir)
  first.append({ ...S1, key: 'complete-0001' })
  const whole = readFileSync(file)
  writeFileSync(file, whole.subarray(0, whole.length - 1)) // the newline never reached the disk
  const log = new SuggestionLog(dir)
  assert.deepEqual(log.records.map((r) => r.key), ['complete-0001'])
  assert.deepEqual(log.damaged, [])
  log.append(S1)
  const reloaded = new SuggestionLog(dir)
  assert.deepEqual(reloaded.records.map((r) => r.key), ['complete-0001', 'reviewer-after-crash'])
  assert.deepEqual(reloaded.damaged, [])
  assert.equal(readFileSync(file, 'utf8').split('\n').filter(Boolean).length, 2)
}))

/** node:fs with one injected failure: the process "crashes" at that point (the call throws). */
function faulty(point) {
  return {
    writeSync: (fd, buf, off, len) => {
      if (point === 'mid-write') {
        const half = Math.floor(len / 2)
        nodeWriteSync(fd, buf, off, half)
        throw Object.assign(new Error('crash mid-write'), { code: 'EIO' })
      }
      if (point === 'short-writes') return nodeWriteSync(fd, buf, off, Math.min(7, len))
      return nodeWriteSync(fd, buf, off, len)
    },
    fsyncSync: (fd) => {
      if (point === 'before-fsync') throw Object.assign(new Error('crash before fsync'), { code: 'EIO' })
      if (point === 'dir-fsync' && nodeFstatSync(fd).isDirectory()) throw Object.assign(new Error('crash at dir fsync'), { code: 'EIO' })
      return nodeFsyncSync(fd)
    },
  }
}

test('crash points around the write and fsync: never acknowledged early, never duplicated, never lost once acknowledged', () => {
  // Mid-write: not acknowledged; the fragment is reported; the retry lands once, on its own line.
  withDir((dir, file) => {
    new SuggestionLog(dir).append({ ...S1, key: 'earlier-0001' })
    const log = new SuggestionLog(dir, { io: faulty('mid-write') })
    assert.throws(() => log.append(S1), (e) => e.status === 500 && /not stored durably/.test(e.message))
    assert.equal(log.byKey.has(S1.key), false, 'no in-memory acknowledgement either')
    const after = new SuggestionLog(dir)
    assert.deepEqual(after.records.map((r) => r.key), ['earlier-0001'])
    assert.equal(after.damaged.length, 1)
    assert.equal(after.damaged[0].kind, 'torn_tail')
    assert.equal(after.append(S1).duplicate, false)
    const final = new SuggestionLog(dir)
    assert.deepEqual(final.records.map((r) => r.key), ['earlier-0001', S1.key])
    assert.equal(final.damaged[0].kind, 'malformed_line')
  })
  // Written but the fsync failed: not acknowledged; if the line did reach the file, a retry is a duplicate.
  withDir((dir, file) => {
    const log = new SuggestionLog(dir, { io: faulty('before-fsync') })
    assert.throws(() => log.append(S1), (e) => e.status === 500)
    const after = new SuggestionLog(dir)
    assert.deepEqual(after.records.map((r) => r.key), [S1.key])
    const bytes = readFileSync(file)
    assert.equal(after.append(S1).duplicate, true)
    assert.deepEqual(readFileSync(file), bytes)
  })
  // A new file whose directory entry could not be fsynced: not acknowledged; the retry is idempotent.
  withDir((dir) => {
    const log = new SuggestionLog(dir, { io: faulty('dir-fsync') })
    assert.throws(() => log.append(S1), (e) => e.status === 500)
    const after = new SuggestionLog(dir)
    assert.deepEqual(after.records.map((r) => r.key), [S1.key], 'the fsynced line is there after the restart')
    assert.equal(after.append(S1).duplicate, true, 'so the retry is a duplicate')
    assert.equal(new SuggestionLog(dir).records.length, 1)
  })
  // Short writes are completed, not mistaken for success.
  withDir((dir) => {
    const log = new SuggestionLog(dir, { io: faulty('short-writes') })
    log.append(S1)
    log.append({ ...S1, key: 'second-0002', text: 'two' })
    assert.deepEqual(new SuggestionLog(dir).records.map((r) => r.key), [S1.key, 'second-0002'])
  })
  // The file stays owner-only.
  withDir((dir, file) => {
    new SuggestionLog(dir).append(S1)
    assert.equal(statSync(file).mode & 0o777, 0o600)
  })
})

/** node:fs with the first N matching calls failing with EIO, then normal; it counts successful fsyncs. */
function flaky(point, failures = 1) {
  const state = { left: failures, fsyncs: 0, fileFsyncs: 0 }
  const eio = (what) => Object.assign(new Error(`EIO at ${what}`), { code: 'EIO' })
  state.io = {
    writeSync: (fd, buf, off, len) => {
      if (point === 'mid-write' && state.left > 0) {
        state.left--
        nodeWriteSync(fd, buf, off, Math.floor(len / 2))
        throw eio('write')
      }
      return nodeWriteSync(fd, buf, off, len)
    },
    fsyncSync: (fd) => {
      const dir = nodeFstatSync(fd).isDirectory()
      if (state.left > 0 && ((point === 'file-fsync' && !dir) || (point === 'dir-fsync' && dir))) {
        state.left--
        throw eio(dir ? 'directory fsync' : 'file fsync')
      }
      nodeFsyncSync(fd)
      state.fsyncs++
      if (!dir) state.fileFsyncs++
    },
  }
  return state
}

test('same-process retry after an unconfirmed write: reconciled against the file, acknowledged once durable, one record on disk', () => {
  for (const point of ['file-fsync', 'dir-fsync']) {
    withDir((dir, file) => {
      const f = flaky(point)
      const log = new SuggestionLog(dir, { io: f.io }) // the live route keeps this one object
      assert.throws(() => log.append(S1), (e) => e.status === 500, point)
      assert.equal(log.byKey.has(S1.key), false, `${point}: no acknowledgement before durability`)
      const fsyncsBefore = f.fileFsyncs
      const retry = log.append(S1) // same object, no restart
      assert.equal(retry.duplicate, false, `${point}: the retry returns the original outcome (stored once)`)
      assert.match(retry.reconciled, /found complete on disk, then fsynced/)
      assert.ok(f.fileFsyncs > fsyncsBefore, `${point}: the ack came only after a successful fsync`)
      assert.equal(log.append(S1).duplicate, true, `${point}: a further retry is a duplicate`)
      const lines = readFileSync(file, 'utf8').split('\n').filter(Boolean)
      assert.equal(lines.length, 1, `${point}: exactly one record line on disk`)
      const reloaded = new SuggestionLog(dir)
      assert.deepEqual(reloaded.records.map((r) => r.key), [S1.key], `${point}: one record after a reload`)
      assert.equal(reloaded.records[0].at, retry.record.at, 'the original attempt\'s record, not a new one')
    })
  }
  // A cut-short write, retried in the same process: the fragment is fenced off, the record lands once.
  withDir((dir, file) => {
    const f = flaky('mid-write')
    const log = new SuggestionLog(dir, { io: f.io })
    assert.throws(() => log.append(S1), (e) => e.status === 500)
    const retry = log.append(S1)
    assert.equal(retry.duplicate, false)
    assert.equal(retry.reconciled, undefined, 'nothing complete was on disk, so it was written now')
    const reloaded = new SuggestionLog(dir)
    assert.deepEqual(reloaded.records.map((r) => r.key), [S1.key])
    assert.equal(reloaded.damaged.length, 1, 'the fragment is reported')
  })
  // While durability still cannot be confirmed, nothing is acknowledged, for any key.
  withDir((dir, file) => {
    const f = flaky('file-fsync', 2)
    const log = new SuggestionLog(dir, { io: f.io })
    assert.throws(() => log.append(S1), (e) => e.status === 500)
    assert.throws(() => log.append({ ...S1, key: 'other-0001', text: 'other' }), (e) => e.status === 500 && /still not confirmed durable/.test(e.message))
    assert.equal(readFileSync(file, 'utf8').split('\n').filter(Boolean).length, 1, 'the other key was not written either')
    assert.equal(log.append(S1).duplicate, false)
    assert.equal(log.append({ ...S1, key: 'other-0001', text: 'other' }).duplicate, false)
    assert.deepEqual(new SuggestionLog(dir).records.map((r) => r.key), [S1.key, 'other-0001'])
  })
})

// ── journal tail ───────────────────────────────────────────────────────────────────────────

test('a journal is read read-only with byte offsets and line hashes anyone can re-check, then followed', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-j-'))
  try {
    const file = join(dir, 'rth-host.jsonl')
    const l1 = `${JSON.stringify({ kind: 'started', pid: 42, t_ms: 0.1 })}\n`
    const l2 = `${JSON.stringify({ kind: 'mute_request', muted: true, via: 'tray', t_ms: 5 })}\n`
    writeFileSync(file, l1 + l2 + '{"kind":"partial"')
    const got = []
    const notes = []
    const tail = new JsonlTail(file, { onRecord: (rec, at) => got.push({ rec, at }), onNote: (t) => notes.push(t) }).start()
    assert.equal(got.length, 2, 'a trailing partial line waits')
    const bytes = readFileSync(file)
    for (const { at } of got) {
      const line = bytes.subarray(at.offset, at.offset + at.length)
      assert.equal(createHash('sha256').update(line).digest('hex'), at.line_sha256)
      assert.equal(at.history, true)
    }
    assert.equal(got[1].at.offset, Buffer.byteLength(l1))
    const waited = new Promise((res) => {
      const t0 = Date.now()
      const check = () => (got.length >= 3 || Date.now() - t0 > 3000 ? res() : setTimeout(check, 20))
      check()
    })
    await new Promise((r) => setTimeout(r, 150)) // the OS arms the watch asynchronously
    appendFileSync(file, ',"t_ms":9}\n')
    await waited
    tail.stop()
    assert.equal(got.length, 3)
    assert.equal(got[2].rec.kind, 'partial')
    assert.equal(got[2].at.history, false)
    assert.deepEqual(notes, [])
    // Bounded backfill: the cut is reported, and the first partial line is skipped.
    const small = []
    const n2 = []
    new JsonlTail(file, { backfillBytes: Buffer.byteLength(l2) + 5, onRecord: (rec) => small.push(rec), onNote: (t) => n2.push(t) }).start().stop()
    assert.deepEqual(n2, ['truncated_history'])
    assert.ok(small.every((x) => x.kind !== 'started'))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

// ── the plugin under Cordis ────────────────────────────────────────────────────────────────

async function loaded(config = {}, { rows = [] } = {}) {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const window = new EventWindow(catalog, { limit: 50 })
  let seq = 0
  const push = (kind, fields) => window.push({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  for (const [k, f] of rows) push(k, f)
  const state = { link: { state: 'attached', detail: 'observer' } }
  ctx.provide('rthEvents', { window, catalog, snapshot: () => ({ link: state.link }) })
  const fiber = ctx.plugin(plugin, config)
  await fiber
  await settle()
  const route = (path) => routes.find((r) => r.path === path)
  const call = async (path, init) => {
    const res = await route(path.split('?')[0]).fetch(new Request(`http://h${path}`, init))
    return { status: res.status, body: await res.json() }
  }
  return { ctx, fiber, routes, window, push, call, setLink: (l) => (state.link = l, window.emit('link', l)) }
}

// The protocol doc comments come from rth's rth-protocol source, passed explicitly.
test('the plugin reads the one rth-events window, serves rows, audit records and suggestions on connection.fetch only', needRth('crates/rth-protocol/src/lib.rs'), async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-p-'))
  try {
    const p = await loaded({ dataDir: dir, protocol: rthFile('crates/rth-protocol/src/lib.rs') }, { rows: [['hello', { protocol: 2, agent: 'rth-agentd', version: 't', boot_id: BOOT, utc_ms_at_origin: 1, journal: '/rth/journal.jsonl' }]] })
    assert.deepEqual(p.routes.map((r) => r.path).sort(), ['/api/rth-mind', '/api/rth-mind/event', '/api/rth-mind/stream', '/api/rth-mind/suggestions'])
    assert.ok(p.routes.every((r) => r.methods.every((m) => m === 'GET' || (m === 'POST' && r.path.endsWith('/suggestions')))), 'the only write is a suggestion')
    p.push('command_admitted', { command_id: 'cmd-7', generation: 1, status: 'admitted' })
    const d = p.push('decision', { decision_id: 'd1', trigger_event: 'host:cmd-7', candidates: ['respond'], selected: 'respond', policy: 'rule', latency_ns: 1e6, episode_id: 'x', training_eligibility: 'unknown' })
    p.push('response_started', { cause: d.id, response_id: 'r-12', request_id: 'cmd-7' })
    p.push('agent_done', { cause: d.id, response_id: 'r-12', status: 'completed' })
    const snap = (await p.call('/api/rth-mind')).body
    assert.equal(snap.rows[0].kind, 'hello')
    assert.equal(snap.rows[0].history, true)
    assert.equal(snap.rows[1].kind, 'preload', 'rows held before Mind loaded are followed by a boundary marker')
    assert.equal(snap.sources.agentd_feed.state, 'bound')
    assert.equal(snap.sources.brain.state, 'no feed')
    const ep = snap.episodes.find((e) => e.title.startsWith('Typed request cmd-7'))
    assert.equal(ep.status, 'completed')
    const started = snap.rows.find((r) => r.kind === 'response_started')
    const audit = (await p.call(`/api/rth-mind/event?id=${started.id}`)).body
    assert.equal(audit.ids.correlation.request_id, 'cmd-7')
    assert.equal(audit.chain.parents[0].kind, 'decision')
    assert.equal(audit.definition.catalog.name, 'message_started')
    assert.equal(audit.definition.protocol.variant, 'ResponseStarted')
    assert.match(audit.origins[0].agent_journal, /\/rth\/journal\.jsonl \(guest path named by Hello/)
    assert.match(audit.integrity.journal_record_hash, /^none per record/)
    assert.equal(audit.envelope.kind, 'response_started')
    assert.match(audit.evidence_sha256, /^[0-9a-f]{64}$/)
    assert.equal(createHash('sha256').update(JSON.stringify(audit.evidence)).digest('hex'), audit.evidence_sha256, 'the evidence hash is re-checkable')
    assert.equal((await p.call('/api/rth-mind/event?id=nope')).status, 404)
    const body = JSON.stringify({ key: 'k-0000-0001', actor: 'The owner', scope: { episode: ep.id, title: ep.title }, text: 'I would have asked a follow-up question.' })
    const post = (b) => p.call('/api/rth-mind/suggestions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: b })
    assert.equal((await post(body)).status, 201)
    const dup = await post(body)
    assert.equal(dup.status, 200)
    assert.equal(dup.body.duplicate, true)
    assert.equal((await post('not json')).status, 400)
    const list = (await p.call(`/api/rth-mind/suggestions?episode=${ep.id}`)).body.suggestions
    assert.equal(list.length, 1)
    assert.equal(readFileSync(join(dir, 'suggestions.jsonl'), 'utf8').trim().split('\n').length, 1)
    p.setLink({ state: 'waiting', detail: 250 })
    const after = (await p.call('/api/rth-mind')).body
    assert.equal(after.rows.at(-1).kind, 'link_lost')
    await p.fiber.dispose()
    assert.equal(p.routes.length, 0)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('rows held by rth-events before Mind loaded seed the sequence silently; only live frames make gap markers', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-s-'))
  try {
    // A held, coalesced reply (seq 2..4) and a done at 5: no gap, though the row's own seq is 2.
    const p = await loaded({ dataDir: dir }, { rows: [['response_started', { response_id: 'r', request_id: 'q' }], ['agent_delta', { response_id: 'r', text: 'a' }], ['agent_delta', { response_id: 'r', text: 'b' }], ['agent_delta', { response_id: 'r', text: 'c' }], ['agent_done', { response_id: 'r', status: 'completed' }]] })
    const gaps = async () => (await p.call('/api/rth-mind')).body.rows.filter((r) => r.kind === 'seq_gap')
    assert.equal((await gaps()).length, 0)
    p.push('log', { level: 'info', msg: 'live, in sequence' }) // seq 6
    assert.equal((await gaps()).length, 0)
    p.window.push({ v: 2, id: `${P}:9`, producer: P, seq: 9, t_ns: 9000, source: 'guest', kind: 'log', level: 'info', msg: 'live, after a skip' })
    const g = await gaps()
    assert.equal(g.length, 1)
    assert.match(g[0].sentence, /^2 frame\(s\) from guest-b0 \(seq 7\.\.8\)/)
    await p.fiber.dispose()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('without rth-events the plugin still serves and says the agent feed is not loaded', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-n-'))
  try {
    const ctx = new Context()
    const routes = []
    ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => {}) } })
    const fiber = ctx.plugin(plugin, { dataDir: dir })
    await fiber
    await settle()
    const snap = await (await routes.find((r) => r.path === '/api/rth-mind').fetch(new Request('http://h/api/rth-mind'))).json()
    assert.equal(snap.sources.agentd_feed.state, 'not loaded')
    assert.equal(snap.link.state, 'unknown')
    assert.equal(snap.rows.length, 0)
    assert.ok(!existsSync(join(dir, 'suggestions.jsonl')), 'nothing is written until a person suggests')
    await fiber.dispose()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('a configured host journal becomes rows with its file, offset and line hash', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-h-'))
  try {
    const file = join(dir, 'rth-host.jsonl')
    writeFileSync(file, [{ kind: 'started', pid: 77, t_ms: 0 }, { kind: 'local_vad_onset', stream_id: 'mic-1', level_dbfs: -20, audible: false, action: 'nothing_audible', t_ms: 12 }, { kind: 'mute_applied', muted: true, generation: 1, partial_samples_discarded: 0, t_ms: 30 }].map((r) => JSON.stringify(r)).join('\n') + '\n')
    const p = await loaded({ dataDir: dir, journals: { host: file } })
    const snap = (await p.call('/api/rth-mind')).body
    assert.equal(snap.sources.host_journal.state, 'following')
    const vad = snap.rows.find((r) => r.kind === 'local_vad_onset')
    assert.equal(vad.lane, 'host')
    assert.equal(vad.clock.t_ms, 12)
    assert.match(vad.clock.wall_basis, /no wall clock/)
    const audit = (await p.call(`/api/rth-mind/event?id=${vad.id}`)).body
    assert.equal(audit.origins[0].file, file)
    assert.match(audit.origins[0].process, /pid 77/)
    const bytes = readFileSync(file)
    const o = audit.origins[0]
    assert.equal(createHash('sha256').update(bytes.subarray(o.offset, o.offset + o.length)).digest('hex'), o.line_sha256)
    assert.match(audit.definition.glossary, /Not in catalog\/events\.v3\.json/)
    assert.match(snap.rows.find((r) => r.kind === 'mute_applied').sentence, /^Microphone muted at the host/)
    await p.fiber.dispose()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('the browser bundle is built in DSH client format and names the Mind tab', () => {
  const path = new URL('../lib/client.js', import.meta.url)
  assert.ok(existsSync(path), 'run: node scripts/build-client.mjs dsh-mind')
  const text = readFileSync(path, 'utf8')
  assert.match(text, /^window\.__ModuleLoader__\.load\(\{\n\tid: "@rth\/dsh-mind",/)
  assert.match(text, /"Mind"/)
  assert.match(text, /nothing below is live/, 'the freshness rule is bundled from src/display.mjs')
  assert.doesNotMatch(text, /require\("react-dom"\)|createRoot/, 'no React copy is bundled')
})
