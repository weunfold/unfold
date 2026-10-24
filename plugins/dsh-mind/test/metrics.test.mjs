// Mind activity metrics (src/metrics.mjs), through the real path: EventWindow -> MindStream (agentd frames and rth-host
// journal records) -> public rows with their structured projection (ref) -> moments -> metrics.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import { MindStream } from '../src/mind.mjs'
import { moments } from '../src/moments.mjs'
import { PRICES, metricLine, modelFor, replyCaption, replyMetrics, sessionSummary, usageReports } from '../src/metrics.mjs'
import { appendFileSync, mkdtempSync, renameSync, truncateSync, unlinkSync, writeFileSync as writeFile } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { followHostJournal } from '../src/journal.mjs'

const catalog = loadCatalog()
function rig(boot = 'g', { started = true } = {}) {
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
    boot,
    agent: (kind, f = {}) => { seq++; window.push({ v: 2, id: `${r.boot}:${seq}`, producer: r.boot, seq, t_ns: seq * 10_000_000, source: 'guest', kind, ...f }) },
    file: '/x/rth-host.jsonl',
    host: (kind, f = {}) => { t += 100; mind.host({ kind, t_ms: 't_ms' in f ? f.t_ms : t, ...f }, { via: 'host journal', file: r.file, offset: f.offset ?? t, length: 1 }) },
    mind,
    view() {
      const snap = mind.snapshot()
      const rows = snap.rows
      const byId = new Map(rows.map((x) => [x.id, x]))
      const eps = Object.fromEntries(snap.episodes.map((e) => [e.id, e]))
      const items = moments(rows, eps).map((m) => (m.type === 'reply' ? { ...m, metrics: replyMetrics(m, byId), model: modelFor(m, rows, byId) } : m))
      return { rows, items, replies: items.filter((m) => m.type === 'reply'), summary: sessionSummary(items, rows) }
    },
  }
  if (started) r.host('started', { pid: 4242, t_ms: 0 })
  return r
}
const span = (met, name) => met.spans.find((s) => s.name === name)
const typedTurn = (r, Q, R, { status = 'completed', play = false } = {}) => {
  r.host('command_written', { command_id: Q })
  r.agent('command_admitted', { command_id: Q, generation: 1, status: 'admitted' })
  r.host('response_started', { response_id: R, request_id: Q })
  r.agent('response_started', { response_id: R, request_id: Q })
  if (play) r.host('playback', { response_id: R, phase: 'started', played_samples: 512 })
  r.agent('agent_delta', { response_id: R, text: 'Oi' })
  r.agent('agent_done', { response_id: R, status })
  r.host('response_done', { response_id: R, status: status === 'cancelled' ? 'cancelled' : status })
}

// ---- no order or proximity joins ----

test('A started, B started, A ended, B ended, usage 629: neither reply gets the tokens; nothing is double billed', () => {
  const r = rig()
  r.host('response_started', { response_id: 'A', request_id: 'mic-1' })
  r.host('response_started', { response_id: 'B', request_id: 'mic-1' })
  r.host('response_done', { response_id: 'A', status: 'completed' })
  r.host('response_done', { response_id: 'B', status: 'completed' })
  r.agent('log', { level: 'info', msg: 'gemini usage gen=255 total_tokens=629' })
  const v = r.view()
  for (const m of v.replies) {
    assert.equal(m.metrics.tokens.value, null)
    assert.equal(m.metrics.cost.value, null)
  }
  assert.equal(v.summary.tokensLinked, 0)
  assert.equal(v.summary.usageReports, 1, 'the report is counted apart, once')
})

test('interleaved host/agent arrival, usage before the terminal, a delayed usage after another start: no reply inherits tokens', () => {
  const r = rig()
  r.agent('response_started', { response_id: 'A', request_id: null })
  r.host('response_started', { response_id: 'A', request_id: null })
  r.agent('log', { level: 'info', msg: 'gemini usage gen=1 total_tokens=500' })
  r.agent('agent_delta', { response_id: 'A', text: 'x' })
  r.host('response_started', { response_id: 'B', request_id: null })
  r.agent('agent_done', { response_id: 'A', status: 'completed' })
  r.host('response_done', { response_id: 'A', status: 'completed' })
  r.agent('log', { level: 'info', msg: 'gemini usage gen=1 total_tokens=700' })
  r.host('response_done', { response_id: 'B', status: 'completed' })
  const v = r.view()
  assert.ok(v.replies.every((m) => m.metrics.tokens.value === null))
  assert.equal(v.summary.usageReports, 2)
})

test('a duplicated or replayed usage report counts once', () => {
  const r = rig()
  r.agent('log', { level: 'info', msg: 'gemini usage gen=1 total_tokens=500' })
  const rows = r.view().rows
  assert.equal(usageReports([...rows, ...rows]).length, 1)
})

test('a cancelled reply then a new one: each keeps its own status and spans; no span uses the other\'s rows', () => {
  const r = rig()
  typedTurn(r, 'q1', 'A', { status: 'cancelled' })
  typedTurn(r, 'q2', 'B')
  const v = r.view()
  const [a, b] = v.replies
  assert.equal(a.metrics.status, 'cancelled')
  assert.equal(b.metrics.status, 'completed')
  const aRows = new Set(a.rows)
  for (const sp of b.metrics.spans) if (sp.start) assert.ok(!aRows.has(sp.start.row) && !aRows.has(sp.end.row))
  assert.equal(v.summary.completed, 1, 'a cancelled reply is not averaged with completed ones')
})

test('a reconnect reusing local ids (a new rth-host process): nothing subtracts across processes', () => {
  const r = rig()
  r.host('command_written', { command_id: 'q1', t_ms: 100 })
  r.host('started', { pid: 2, t_ms: 5 })
  r.host('response_started', { response_id: 'r1', request_id: 'q1', t_ms: 50 })
  r.host('response_done', { response_id: 'r1', status: 'completed', t_ms: 9000 })
  const [m] = r.view().replies
  const fin = span(m.metrics, 'reply finished')
  assert.equal(fin.ms, null)
  assert.match(fin.why, /different rth-host processes|another rth-host process/)
})

test('an unrelated end-of-speech signal is never used as a start', () => {
  const r = rig()
  r.host('provider_signal', { signal: 'voice_activity_end', response_id: null })
  r.host('response_started', { response_id: 'v1', request_id: 'mic-1' })
  r.host('playback', { response_id: 'v1', phase: 'started', played_samples: 512 })
  r.host('response_done', { response_id: 'v1', status: 'completed' })
  const [m] = r.view().replies
  for (const sp of m.metrics.spans) if (sp.start) assert.notEqual(sp.start.kind, 'provider_signal')
  assert.ok(span(m.metrics, 'playback started after the reply began').ms >= 0, 'the voice span runs from its own reply start')
})

// ---- named endpoints, clock origins, honest labels ----

test('a restart with increasing-looking timestamps from another process is not subtracted', () => {
  const r = rig()
  r.host('command_written', { command_id: 'q1', t_ms: 100 })
  r.host('started', { pid: 3, t_ms: 200 }) // a new process whose clock happens to read later
  r.host('response_started', { response_id: 'r1', request_id: 'q1', t_ms: 300 })
  r.host('response_done', { response_id: 'r1', status: 'completed', t_ms: 900 })
  const [m] = r.view().replies
  assert.equal(span(m.metrics, 'reply finished').ms, null)
  assert.equal(span(m.metrics, 'reply duration').ms, 600, 'within the new process it is measured')
})

test('a first event re-added after eviction is never a "first"', () => {
  const rows = [
    { id: 'a', lane: 'agentd', kind: 'command_admitted', clock: { t_ns: 1e9 }, ref: { command_id: 'q', status: 'admitted', producer: 'g' } },
    { id: 'b', lane: 'agentd', kind: 'response_started', clock: { t_ns: 1.1e9 }, ref: { response_id: 'R', request_id: 'q', producer: 'g' } },
    { id: 'c', lane: 'agentd', kind: 'agent_delta', clock: { t_ns: 1.5e9 }, ref: { response_id: 'R', producer: 'g', continuation: true } },
  ]
  const met = replyMetrics({ rows: ['a', 'b', 'c'] }, new Map(rows.map((x) => [x.id, x])))
  assert.match(span(met, 'first text received').why, /evicted/)
})

test('terminal before playback completes; no playback reporting; generated audio without device playback', () => {
  const r = rig()
  typedTurn(r, 'q1', 'A', { play: true })
  r.host('playback', { response_id: 'A', phase: 'finished', played_samples: 9000 })
  typedTurn(r, 'q2', 'B')
  r.agent('agent_audio', { response_id: 'B', samples: 24000 })
  const [a, b] = r.view().replies
  assert.equal(a.metrics.playback, 'reported')
  assert.ok(span(a.metrics, 'reply finished').ms >= 0, 'finished = the host-observed terminal, not playback completion')
  assert.equal(b.metrics.playback, 'not reported', 'generated audio is not device playback; missing is "not reported", never "none"')
})

test('compact and expanded labels agree; provider TTFT stays explicitly unavailable', () => {
  const r = rig()
  typedTurn(r, 'q1', 'A', { play: true })
  const [a] = r.view().replies
  const line = metricLine(a.metrics)
  assert.match(line, /^finished in [\d.]+ s · first text [\d.]+ s · playback reported$/)
  for (const name of ['reply finished', 'first text received']) assert.ok(span(a.metrics, name).ms != null)
  assert.match(span(a.metrics, 'provider time to first token').why, /not instrumented/)
  assert.doesNotMatch(line, /token|TTFT/i)
})

// ---- per-record model, coverage, no repricing ----

test('mixed-model history: each reply keeps the model in force before it; a later model note changes nothing earlier', () => {
  const r = rig()
  r.agent('log', { level: 'info', msg: 'gemini ready gen=1 model=gemini-2.5-flash-native-audio-preview-12-2025 ws_connect_ms=1' })
  typedTurn(r, 'q1', 'A')
  r.agent('log', { level: 'info', msg: 'gemini ready gen=2 model=gemini-3.8-live ws_connect_ms=1' })
  typedTurn(r, 'q2', 'B')
  const [a, b] = r.view().replies
  assert.equal(a.model, 'gemini-2.5-flash-native-audio-preview-12-2025')
  assert.equal(b.model, 'gemini-3.8-live')
  const r2 = rig()
  typedTurn(r2, 'q1', 'A')
  assert.equal(r2.view().replies[0].model, null, 'unknown model stays unknown')
})

test('partial coverage is disclosed; no zero substitution; no cost from a tariff table', () => {
  const r = rig()
  typedTurn(r, 'q1', 'A')
  r.host('response_started', { response_id: 'B', request_id: 'mic-1' }) // never ends in this window
  r.agent('response_started', { response_id: 'B', request_id: 'mic-1' }) // the agent's own chain carries its status
  r.agent('agent_done', { response_id: 'B', status: 'completed' })
  const v = r.view()
  assert.equal(v.summary.replies, 2)
  assert.equal(v.summary.completed, 2)
  assert.equal(v.summary.timed, 1, 'one of two completed replies has both host endpoints')
  for (const m of v.replies) assert.equal(m.metrics.cost.value, null)
  assert.equal(PRICES.id, 'gemini-api-pricing@2026-10-19')
  assert.ok(Object.isFrozen(PRICES.models['gemini-3.8-live']), 'the pinned table cannot be edited in place')
})

test('two replies sharing one episode are not attributed (ambiguous), never guessed', () => {
  const rows = [
    { id: 'a', lane: 'host', kind: 'response_started', clock: { t_ms: 1 }, ref: { response_id: 'A', host_life: 0 } },
    { id: 'b', lane: 'host', kind: 'response_started', clock: { t_ms: 2 }, ref: { response_id: 'B', host_life: 0 } },
  ]
  const met = replyMetrics({ rows: ['a', 'b'] }, new Map(rows.map((x) => [x.id, x])))
  assert.match(met.why, /2 replies share this episode/)
  assert.equal(metricLine(met), '')
})

// ---- ids are qualified by producer lifetime ----

const timedSpans = (m) => m.metrics.spans.filter((s) => s.ms != null)
const repliesNamed = (v, R) => v.replies.filter((m) => m.metrics.reply === R || m.rows.some((id) => v.rows.find((x) => x.id === id)?.ref?.response_id === R))

test('one reply id in two rth-host lifetimes (q1 then q2) is not "100 ms completed" from the first', () => {
  const r = rig('g', { started: false })
  r.host('started', { pid: 1, t_ms: 0 })
  r.host('response_started', { response_id: 'R', request_id: 'q1', t_ms: 100 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 200 })
  r.host('started', { pid: 2, t_ms: 0 })
  r.host('response_started', { response_id: 'R', request_id: 'q2', t_ms: 300 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 600 })
  const v = r.view()
  const named = repliesNamed(v, 'R')
  assert.equal(named.length, 1, 'one moment holds both lifetimes')
  for (const m of named) {
    const starts = m.rows.map((id) => v.rows.find((x) => x.id === id)).filter((x) => x?.kind === 'response_started')
    if (starts.length > 1) {
      assert.match(m.metrics.why, /different host processes.*ambiguous/)
      assert.equal(m.metrics.status, null)
      assert.deepEqual(timedSpans(m), [])
    }
    assert.ok(!timedSpans(m).some((s) => s.ms === 100 && starts.length > 1), 'never the first lifetime\'s 100 ms for a moment holding both')
  }
  assert.equal(v.summary.timed, 0)
})

test('one reply id started in two agent boots is ambiguous', () => {
  const r = rig('g')
  r.agent('response_started', { response_id: 'R', request_id: 'q1' })
  r.agent('agent_done', { response_id: 'R', status: 'completed' })
  r.boot = 'h'
  r.agent('response_started', { response_id: 'R', request_id: 'q2' })
  r.agent('agent_done', { response_id: 'R', status: 'completed' })
  const v = r.view()
  const both = repliesNamed(v, 'R').filter((m) => m.rows.filter((id) => v.rows.find((x) => x.id === id)?.kind === 'response_started').length > 1)
  assert.equal(both.length, 1, 'the two boots land in one moment')
  assert.match(both[0].metrics.why, /different agent boots.*ambiguous/)
  assert.equal(both[0].metrics.status, null)
})

test('rth-host and the agent binding one reply id to different requests is ambiguous', () => {
  const r = rig()
  r.host('command_written', { command_id: 'q1' })
  r.agent('command_admitted', { command_id: 'q1', generation: 1, status: 'admitted' })
  r.host('response_started', { response_id: 'R', request_id: 'q1' })
  r.agent('response_started', { response_id: 'R', request_id: 'q2' })
  r.agent('agent_delta', { response_id: 'R', text: 'Oi' })
  r.agent('agent_done', { response_id: 'R', status: 'completed' })
  r.host('response_done', { response_id: 'R', status: 'completed' })
  const [m] = r.view().replies
  assert.match(m.metrics.why, /different requests.*ambiguous/)
  assert.deepEqual(timedSpans(m), [])
})

test('a request id typed in two host lifetimes is ambiguous; the valid same-reply case still measures', () => {
  const r = rig()
  r.host('command_written', { command_id: 'Q' })
  r.host('started', { pid: 9, t_ms: 0 })
  r.host('command_written', { command_id: 'Q' })
  r.host('response_started', { response_id: 'R', request_id: 'Q' })
  r.host('response_done', { response_id: 'R', status: 'completed' })
  const v = r.view()
  const m = repliesNamed(v, 'R')[0]
  assert.equal(m.rows.filter((id) => v.rows.find((x) => x.id === id)?.kind === 'command_written').length, 2)
  assert.match(m.metrics.why, /request Q was typed more than once/)
  assert.deepEqual(timedSpans(m), [])
  const ok = rig()
  typedTurn(ok, 'dsh-1', 'R1')
  const [good] = ok.view().replies
  assert.equal(good.metrics.why, undefined)
  assert.ok(span(good.metrics, 'reply finished').ms > 0, 'host and agent observing the same reply still measure')
  assert.ok(span(good.metrics, 'first text received').ms > 0)
})

test('the model is observed context by order, never a per-reply binding or a price input', () => {
  const src = modelFor.toString() + String(replyMetrics)
  assert.ok(!/PRICES/.test(src), 'no price is looked up from the observed model')
})

// ---- absent timestamps are missing, never zero; lifetimes need a started record ----

const hostReply = (r, a, b) => {
  r.host('response_started', { response_id: 'R', request_id: 'mic-1', t_ms: a })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: b })
  return span(r.view().replies[0].metrics, 'reply duration')
}

test('a missing start, end, or both is unavailable, never 500 ms or 0 ms', () => {
  assert.equal(hostReply(rig(), null, 500).ms, null)
  assert.match(hostReply(rig(), null, 500).why, /start time was not reported/)
  assert.match(hostReply(rig(), 100, null).why, /end time was not reported/)
  const both = hostReply(rig(), null, null)
  assert.equal(both.ms, null)
  assert.match(both.why, /start and end times were not reported/)
  const v = rig()
  hostReply(v, null, null)
  assert.equal(v.view().summary.timed, 0, 'never enters the average')
})

test('malformed times (string, NaN, Infinity) are unavailable on both clocks; a genuine zero is measured', () => {
  for (const bad of ['100', NaN, Infinity, undefined, {}]) {
    const rows = [
      { id: 'a', lane: 'host', kind: 'response_started', clock: { t_ms: bad }, ref: { response_id: 'R', host_life: 'host-1' } },
      { id: 'b', lane: 'host', kind: 'response_done', clock: { t_ms: 600 }, ref: { response_id: 'R', host_life: 'host-1', status: 'completed' } },
      { id: 'c', lane: 'agentd', kind: 'command_admitted', clock: { t_ns: 1e6 }, ref: { command_id: 'Q', status: 'admitted', producer: 'g' } },
      { id: 'd', lane: 'agentd', kind: 'response_started', clock: { t_ns: 2e6 }, ref: { response_id: 'R', request_id: 'Q', producer: 'g' } },
      { id: 'e', lane: 'agentd', kind: 'agent_delta', clock: { t_ns: bad }, ref: { response_id: 'R', producer: 'g' } },
    ]
    const met = replyMetrics({ rows: rows.map((x) => x.id) }, new Map(rows.map((x) => [x.id, x])))
    assert.equal(span(met, 'reply duration').ms, null, `host start ${String(bad)}`)
    assert.equal(span(met, 'first text received').ms, null, `agent end ${String(bad)}`)
  }
  assert.equal(hostReply(rig(), 0, 250).ms, 250, 'zero is a real time since the host process started')
})

test('no started record observed: host spans are unavailable (process not established), never lifetime 0', () => {
  const r = rig('g', { started: false })
  const d = hostReply(r, 100, 400)
  assert.equal(d.ms, null)
  assert.match(d.why, /host process not established/)
  assert.ok(r.view().rows.filter((x) => x.lane === 'host').every((x) => x.ref.host_life === null))
})

test('a replaced journal with no started record and increasing timestamps is not continuity', () => {
  const r = rig()
  r.host('command_written', { command_id: 'Q', t_ms: 1000 })
  r.file = '/x/rth-host.replaced.jsonl'
  r.host('response_started', { response_id: 'R', request_id: 'Q', t_ms: 1200 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 1500 })
  const m = r.view().replies[0]
  assert.equal(span(m.metrics, 'reply duration').ms, null)
  assert.match(span(m.metrics, 'reply duration').why, /not established|another rth-host process/)
  assert.equal(span(m.metrics, 'reply finished').ms, null)
  // the same file read again from a lower offset (rotated or rewritten): also unknown until started
  const s = rig()
  s.host('response_started', { response_id: 'S', request_id: 'mic-1', t_ms: 2000 })
  s.host('response_done', { response_id: 'S', status: 'completed', t_ms: 2400, offset: 1 })
  assert.match(span(s.view().replies[0].metrics, 'reply duration').why, /not established|different rth-host|another rth-host/)
})

test('a clock reset without a started record invalidates continuity but makes no new process; a later started does', () => {
  const r = rig()
  r.host('response_started', { response_id: 'A', request_id: 'mic-1', t_ms: 5000 })
  r.host('response_done', { response_id: 'A', status: 'completed', t_ms: 50 })
  r.host('response_started', { response_id: 'B', request_id: 'mic-2', t_ms: 80 })
  r.host('response_done', { response_id: 'B', status: 'completed', t_ms: 90 })
  r.host('started', { pid: 77, t_ms: 0 })
  r.host('response_started', { response_id: 'C', request_id: 'mic-3', t_ms: 10 })
  r.host('response_done', { response_id: 'C', status: 'completed', t_ms: 40 })
  const v = r.view()
  const by = (R) => span(v.replies.find((m) => m.metrics.reply === R).metrics, 'reply duration')
  assert.equal(by('A').ms, null)
  assert.equal(by('B').ms, null)
  assert.match(by('B').why, /not established/)
  assert.equal(by('C').ms, 30)
})

// ---- whole chains, never a fallback endpoint ----

test('host A binds R->Q, host B types Q and ends R: never "finished in 0.70 s"', () => {
  const r = rig('g', { started: false })
  r.host('started', { pid: 1, t_ms: 0 })
  r.host('response_started', { response_id: 'R', request_id: 'Q', t_ms: 100 })
  r.host('started', { pid: 2, t_ms: 0 })
  r.host('command_written', { command_id: 'Q', t_ms: 200 })
  r.host('response_done', { response_id: 'R', status: 'completed', t_ms: 900 })
  const v = r.view()
  const [m] = repliesNamed(v, 'R')
  assert.deepEqual(timedSpans(m), [], 'no span is measured from rows of two host processes')
  assert.equal(m.metrics.status, null, "host B's terminal is not the status of host A's reply")
  assert.match(span(m.metrics, 'reply finished').why, /another rth-host process/)
  assert.match(span(m.metrics, 'reply duration').why, /another rth-host process/)
  assert.equal(metricLine(m.metrics), '')
  assert.equal(v.summary.timed, 0)
})

test('agent boot g binds R->Q, boot h admits Q and sends the first text: first text unavailable', () => {
  const r = rig('g')
  r.agent('response_started', { response_id: 'R', request_id: 'Q' })
  r.boot = 'h'
  r.agent('command_admitted', { command_id: 'Q', generation: 1, status: 'admitted' })
  r.agent('agent_delta', { response_id: 'R', text: 'Oi' })
  r.agent('agent_done', { response_id: 'R', status: 'completed' })
  const [m] = r.view().replies
  assert.deepEqual(timedSpans(m), [])
  assert.equal(m.metrics.status, null, "boot h's terminal is not the status of boot g's reply")
  assert.match(span(m.metrics, 'first text received').why, /another agent boot/)
})

test('an agent text with no agent start of its own is not a first text; the same-lifetime chains still measure', () => {
  const r = rig()
  r.host('command_written', { command_id: 'Q' })
  r.agent('command_admitted', { command_id: 'Q', generation: 1, status: 'admitted' })
  r.host('response_started', { response_id: 'R', request_id: 'Q' })
  r.agent('agent_delta', { response_id: 'R', text: 'Oi' })
  r.host('response_done', { response_id: 'R', status: 'completed' })
  const [m] = r.view().replies
  assert.match(span(m.metrics, 'first text received').why, /agent's own start for this reply/)
  assert.ok(span(m.metrics, 'reply finished').ms > 0, 'the host chain (one process) still measures')
  const ok = rig()
  typedTurn(ok, 'dsh-1', 'R1', { play: true })
  const [good] = ok.view().replies
  for (const name of ['reply finished', 'reply duration', 'playback started', 'first text received']) assert.ok(span(good.metrics, name).ms > 0, name)
  assert.equal(good.metrics.status, 'completed')
})

// ---- the real tail -> Mind boundary; a non-append change is never continuity ----

const J = (...recs) => recs.map((r) => JSON.stringify(r) + '\n').join('')
function journal(initial) {
  const dir = mkdtempSync(join(tmpdir(), 'mind-journal-'))
  const path = join(dir, 'rth-host.jsonl')
  writeFile(path, initial)
  const mind = new MindStream({ limit: 500 })
  const notes = []
  const tail = followHostJournal(path, mind, { onNote: (t) => notes.push(t) }).start()
  tail.watcher?.close() // deterministic: the test calls read() as the watcher callback would
  const j = {
    dir, path, mind, notes, tail,
    read: () => { tail.read(false, false); tail.watcher?.close() },
    replace: (text) => { const tmp = join(dir, 'next.jsonl'); writeFile(tmp, text); renameSync(tmp, path); j.read() },
    measured: (R) => { const m = j.reply(R); return m ? span(m, 'reply duration')?.ms ?? null : null },
    reply: (R) => {
      const snap = mind.snapshot()
      const byId = new Map(snap.rows.map((x) => [x.id, x]))
      const eps = Object.fromEntries(snap.episodes.map((e) => [e.id, e]))
      return moments(snap.rows, eps).filter((m) => m.type === 'reply').map((m) => replyMetrics(m, byId)).find((x) => x.reply === R)
    },
  }
  return j
}
const pad = (text, size) => { const short = size - Buffer.byteLength(text); assert.ok(short >= 0); return text.replace(/\}\n$/, `,"pad":"${'x'.repeat(Math.max(0, short - 9))}"}\n`) }
const A = J({ kind: 'started', pid: 1, t_ms: 0 }, { kind: 'response_started', response_id: 'R', request_id: 'mic-1', t_ms: 100 })

test("a rename with a larger replacement: the new start is read, the old reply is not measured across it", () => {
  const j = journal(A)
  j.replace(pad(J({ kind: 'started', pid: 2, t_ms: 0 }), Buffer.byteLength(A)) + J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 900 }))
  assert.ok(j.notes.includes('replaced'))
  const m = j.reply('R')
  assert.equal(span(m, 'reply duration').ms, null, 'never 800 ms from two processes')
  assert.equal(m.status, null)
  assert.ok(j.mind.snapshot().rows.some((r) => r.kind === 'started' && /pid2/.test(r.ref.host_life)), "the new process's start is not skipped")
  j.tail.stop()
})

test('an equal-size replacement followed by an append, and an in-place equal-size rewrite: both discontinuities', () => {
  const j = journal(A)
  j.replace(pad(J({ kind: 'note', t_ms: 300 }), Buffer.byteLength(A)))
  assert.ok(j.notes.includes('replaced'))
  appendFileSync(j.path, J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 900 }))
  j.read()
  const m = j.reply('R')
  assert.equal(span(m, 'reply duration').ms, null)
  assert.match(span(m, 'reply duration').why, /another rth-host process|not established/)
  const k = journal(A)
  writeFile(k.path, pad(J({ kind: 'note', t_ms: 300 }), Buffer.byteLength(A))) // same inode, same size, other bytes
  appendFileSync(k.path, J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 900 }))
  k.read()
  assert.ok(k.notes.includes('rewritten'))
  assert.equal(span(k.reply('R'), 'reply duration').ms, null)
  j.tail.stop(); k.tail.stop()
})

test('after a replacement, a newly observed started record establishes the next lifetime, which measures', () => {
  const j = journal(A)
  j.replace(J({ kind: 'started', pid: 2, t_ms: 0 }, { kind: 'response_started', response_id: 'S', request_id: 'mic-2', t_ms: 50 }))
  appendFileSync(j.path, J({ kind: 'response_done', response_id: 'S', status: 'completed', t_ms: 80 }))
  j.read()
  assert.equal(span(j.reply('S'), 'reply duration').ms, 30)
  j.tail.stop()
})

test('a shrink and a read loss (unlinked, then a new file without started) invalidate the lifetime', () => {
  const j = journal(A)
  truncateSync(j.path, 10)
  writeFile(j.path, J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 900 }))
  j.read()
  assert.ok(j.notes.some((t) => t === 'shrank' || t === 'rewritten'))
  assert.equal(j.measured('R'), null)
  const k = journal(A)
  unlinkSync(k.path)
  k.read()
  assert.ok(k.notes.includes('missing'))
  writeFile(k.path, A.replace(/\{"kind":"started"[^\n]*\n/, '') + J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 900 }))
  k.read()
  assert.equal(k.measured('R'), null, 'no started in the new file: not established')
  j.tail.stop(); k.tail.stop()
})

test('plain appends keep the lifetime (the bounded read-only tail still follows)', () => {
  const j = journal(A)
  appendFileSync(j.path, J({ kind: 'response_done', response_id: 'R', status: 'completed', t_ms: 400 }))
  j.read()
  assert.deepEqual(j.notes.filter((t) => t !== 'watch_error'), [])
  assert.equal(span(j.reply('R'), 'reply duration').ms, 300)
  j.tail.stop()
})

// ---- the caption makes no claim the activity result does not ----

const DURATION = /\b\d+(\.\d+)? s\b|\b\d+ ms\b/
test("a reused-id sequence (one boot, R twice at 1-4 s): ambiguous, and the caption shows no duration", () => {
  const r = rig('g')
  r.agent('response_started', { response_id: 'R', request_id: 'q1', t_ns: 1e9 })
  r.agent('agent_done', { response_id: 'R', status: 'completed', t_ns: 2e9 })
  r.agent('response_started', { response_id: 'R', request_id: 'q2', t_ns: 3e9 })
  r.agent('agent_done', { response_id: 'R', status: 'completed', t_ns: 4e9 })
  const [m] = r.view().replies
  assert.match(m.metrics.why, /started 2 times by the agent.*ambiguous/)
  assert.equal(metricLine(m.metrics), '')
  const cap = replyCaption(m, Date.now())
  assert.doesNotMatch(cap, DURATION, `caption "${cap}"`)
  if (m.duration_ms != null) assert.equal(m.duration_ms, 3000, 'the legacy first-to-last interval still exists as a diagnostic; it is not the caption')
})

test('an unknown host lifetime with a raw playback row: playback not reported, and the caption never says played', () => {
  const r = rig('g', { started: false })
  r.host('response_started', { response_id: 'R', request_id: 'mic-1' })
  r.host('playback', { response_id: 'R', phase: 'started', played_samples: 512 })
  r.host('playback', { response_id: 'R', phase: 'finished', played_samples: 24000 })
  r.host('response_done', { response_id: 'R', status: 'completed' })
  const [m] = r.view().replies
  assert.equal(m.metrics.playback, 'not reported')
  const cap = replyCaption(m, Date.now())
  assert.doesNotMatch(cap, /played|playback|heard/, `caption "${cap}"`)
  assert.doesNotMatch(cap, DURATION, `caption "${cap}"`)
})

test('missing or evicted first-event evidence: the caption shows no substitute duration or playback', () => {
  const rows = [
    { id: 'a', lane: 'agentd', kind: 'command_admitted', clock: { t_ns: 1e9 }, ref: { command_id: 'q', status: 'admitted', producer: 'g' } },
    { id: 'b', lane: 'agentd', kind: 'response_started', clock: { t_ns: 1.1e9 }, ref: { response_id: 'R', request_id: 'q', producer: 'g' } },
    { id: 'c', lane: 'agentd', kind: 'agent_delta', clock: { t_ns: 1.5e9 }, ref: { response_id: 'R', producer: 'g', continuation: true } },
    { id: 'd', lane: 'agentd', kind: 'agent_done', clock: { t_ns: 2.4e9 }, ref: { response_id: 'R', producer: 'g', status: 'completed' } },
  ]
  const metrics = replyMetrics({ rows: rows.map((x) => x.id) }, new Map(rows.map((x) => [x.id, x])))
  const m = { type: 'reply', status: 'completed', open: false, uncertain: false, spoken: true, toTyped: true, duration_ms: 1400, metrics, rows: rows.map((x) => x.id), at: 0 }
  assert.match(span(metrics, 'first text received').why, /evicted/)
  const cap = replyCaption(m, 10)
  assert.equal(cap, 'to a typed message', 'ordinary non-metric words stay; no 1.4 s, no played')
  const missing = replyCaption({ ...m, metrics: undefined }, 10)
  assert.equal(missing, 'to a typed message', 'no activity result at all: still no timing or playback claim')
})

test('a valid same-lifetime reply still captions its qualified timing and host-report playback', () => {
  const r = rig()
  typedTurn(r, 'q1', 'A', { play: true })
  r.host('playback', { response_id: 'A', phase: 'finished', played_samples: 9000 })
  const [m] = r.view().replies
  const cap = replyCaption(m, Date.now())
  assert.match(cap, /^to a typed message · finished in [\d.]+ s · first text [\d.]+ s · playback reported$/, cap)
  assert.doesNotMatch(cap, /\bplayed\b/)
})
