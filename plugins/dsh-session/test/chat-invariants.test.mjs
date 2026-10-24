// Chat invariants: the live view's freshness, explicit identity before adjacency, and an
// unconfirmed stop that stays unconfirmed. (A local onset while muted is in
// voice-projection.test.mjs: it needs the plugin harness.) Each negative case has a positive
// control.
import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { VoiceLedger } from '../src/voice.mjs'
import { RthBridge } from '../src/bridge.mjs'
import { LiveView } from '../src/live.mjs'
import { voiceFinalText } from '../index.mjs'

// ── the client hook (the exact source of useLive, run with deterministic stand-ins) ──

function hookRig({ withTimers = true } = {}) {
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  const hook = src.slice(src.indexOf('function useLive(on'), src.indexOf('/** The listening bubble ('))
  let value = null
  let cleanup = null
  let deps = null
  let now = 0
  const timers = new Map()
  let nextId = 1
  const sources = []
  class ES {
    constructor(url) { this.url = url; this.listeners = {}; this.closed = false; sources.push(this) }
    addEventListener(k, fn) { (this.listeners[k] ??= []).push(fn) }
    close() { this.closed = true }
    emit(k, data) { for (const fn of this.listeners[k] ?? []) fn({ data: JSON.stringify(data) }); this[`on${k}`]?.({}) }
  }
  const ctx = {
    LIVE: 'fake', EventSource: ES, Date: { now: () => now }, JSON, Math, Number,
    useState: (init) => [value ?? init, (v) => { value = typeof v === 'function' ? v(value) : v }],
    useEffect: (fn, d) => { if (deps && d && d.every((x, i) => x === deps[i])) return; cleanup?.(); deps = d; cleanup = fn() ?? null },
    ...(withTimers ? {
      setTimeout: (f, ms) => { const id = nextId++; timers.set(id, { f, at: now + ms }); return id },
      clearTimeout: (id) => timers.delete(id),
    } : {}),
  }
  vm.createContext(ctx)
  vm.runInContext(`${hook}\nthis.useLive = useLive`, ctx)
  return {
    render: (on) => ctx.useLive(on),
    get value() { return value },
    get es() { return sources.at(-1) },
    sources,
    advance: (ms) => {
      now += ms
      for (const [id, t] of [...timers].sort((a, b) => a[1].at - b[1].at)) if (t.at <= now) { timers.delete(id); t.f() }
    },
    timers,
    unmount: () => { cleanup?.(); cleanup = null; deps = null },
  }
}
const listening = (ttl = 8000) => ({ state: 'listening', since: 0, level: null, text: null, ttl_ms: ttl })

test('hook: a stream error drops the listening value at once', () => {
  for (const withTimers of [true, false]) {
    const h = hookRig({ withTimers })
    h.render(true)
    h.es.emit('snapshot', listening())
    assert.equal(h.value.state, 'listening')
    h.es.emit('error', {})
    assert.ok(!h.value || h.value.state === 'idle', `retained ${JSON.stringify(h.value)} (timers: ${withTimers})`)
    h.unmount()
  }
})

test('hook: speech expires locally at its deadline, with no idle event from the host', () => {
  const h = hookRig()
  h.render(true)
  h.es.emit('snapshot', listening(3000))
  h.advance(2999)
  assert.equal(h.value.state, 'listening')
  h.advance(1)
  assert.ok(!h.value || h.value.state === 'idle', 'the browser ends it itself')
  h.unmount()
})

test('hook: a silently stalled stream (no frame, no beat) drops it within the lease; beats keep the connection, not the speech', () => {
  const h = hookRig()
  h.render(true)
  h.es.emit('snapshot', listening(60_000))
  for (let i = 0; i < 3; i++) { h.advance(5000); h.es.emit('beat', { at: i }) }
  assert.equal(h.value.state, 'listening', 'beats keep a live connection')
  h.advance(30_000)
  assert.ok(!h.value || h.value.state === 'idle', 'silence beyond the lease drops it')
  // beats never extend speech past its own deadline
  const g = hookRig()
  g.render(true)
  g.es.emit('snapshot', listening(4000))
  for (let i = 0; i < 4; i++) { g.advance(1500); g.es.emit('beat', { at: i }) }
  assert.ok(!g.value || g.value.state === 'idle', 'a beat is not speech')
  h.unmount(); g.unmount()
})

test('hook: session away clears it; back starts empty until a fresh snapshot; a reconnect recovers from fresh evidence', () => {
  const h = hookRig()
  h.render(true)
  h.es.emit('snapshot', listening())
  h.render(false)
  assert.ok(!h.value, 'away: nothing kept')
  assert.equal(h.sources[0].closed, true)
  h.render(true)
  assert.ok(!h.value, 'back: not revived from the old value')
  h.es.emit('snapshot', { state: 'heard', since: 5, level: null, text: 'Fresh words.', ttl_ms: 4000 })
  assert.equal(h.value.text, 'Fresh words.', 'positive control: fresh evidence shows')
  h.es.emit('error', {})
  assert.ok(!h.value || h.value.state === 'idle')
  h.es.emit('snapshot', listening(2000))
  assert.equal(h.value.state, 'listening', 'recovery after a reconnect, from the new snapshot')
  assert.equal(h.sources.filter((s) => !s.closed).length, 1, 'one subscription')
  h.unmount()
  assert.equal(h.timers.size, 0, 'no timer left behind')
})

// ── duplicates never renew or resurrect speech ──

function liveBridge() {
  const b = new RthBridge({ host: {}, observer: { start() {}, stop() {} } })
  const live = new LiveView()
  b.onChange((k, d) => { if (k === 'voice-live') live.input(d) })
  return { b, live, done: () => { b.stop(); live.stop() } }
}

test('a replayed transcript after its reply does not bring the words back; new words do', () => {
  const { b, live, done } = liveBridge()
  const e = { kind: 'input_transcript', id: 'g:1', text: 'Already shown.', utterance_id: 'u1', utterance_seq: 0, association: 'unknown' }
  b.ingest(e)
  b.ingest({ kind: 'decision', id: 'g:2', decision_id: 'd1', policy: 'provider:gemini-auto-vad@v1' })
  b.ingest({ kind: 'response_started', id: 'g:3', response_id: 'r1', request_id: 'mic', cause: 'g:2' })
  assert.equal(live.view().state, 'idle')
  b.ingest(e)
  b.ingest({ ...e, id: 'g:9' }) // the same part under a new envelope id (a reconnect replay)
  assert.equal(live.view().state, 'idle', JSON.stringify(live.view()))
  b.ingest({ kind: 'input_transcript', id: 'g:10', text: 'New words.', utterance_id: 'u1', utterance_seq: 1, association: 'unknown' })
  assert.deepEqual([live.view().state, live.view().text], ['heard', 'New words.'], 'positive control')
  done()
})

test('a replayed speech-start signal does not restart listening', () => {
  const { b, live, done } = liveBridge()
  const s = { kind: 'provider_signal', id: 'g:1', signal: 'voice_activity_start', response_id: null }
  b.ingest(s)
  assert.equal(live.view().state, 'listening')
  b.ingest({ kind: 'decision', id: 'g:2', decision_id: 'd1', policy: 'provider:gemini-auto-vad@v1' })
  b.ingest({ kind: 'response_started', id: 'g:3', response_id: 'r1', request_id: 'mic', cause: 'g:2' })
  assert.equal(live.view().state, 'idle')
  b.ingest(s)
  assert.equal(live.view().state, 'idle', 'the same envelope again is not new speech')
  b.ingest({ ...s, id: 'g:4' })
  assert.equal(live.view().state, 'listening', 'positive control: a new signal')
  done()
})

test('the published view carries its remaining lifetime (ttl_ms), bounded by the cap', () => {
  let t = 0
  const live = new LiveView({ now: () => t })
  live.input({ type: 'speech-start' })
  assert.ok(live.published().ttl_ms > 0 && live.published().ttl_ms <= 30_000)
  t = 29_000
  assert.equal(live.published().ttl_ms, 1000)
  live.input({ type: 'reply' })
  assert.equal(live.published().state, 'idle')
  live.stop()
})

// ── the first explicit identity is resolved before adjacency ──

test('the first explicitly linked reply takes ITS utterance, not the latest one; the other is not lost', () => {
  const notes = []
  const l = new VoiceLedger({ onNote: (n) => notes.push(n), timing: { adjacentHoldMs: 60_000, pairGraceMs: 10, holdMs: 20 } })
  l.transcript({ id: 'e1', text: 'The question for reply one.', utterance_id: 'u1', utterance_seq: 0, final: true, association: 'unknown' })
  l.transcript({ id: 'e2', text: 'An unrelated aside.', utterance_id: 'u2', utterance_seq: 0, final: true, association: 'unknown' })
  l.pin({ responseId: 'r1', utteranceId: 'u1' })
  const r = l.commit({ responseId: 'r1' })
  assert.equal(r?.text, 'The question for reply one.')
  assert.equal(r?.adjacent, undefined, 'by id, not by adjacency')
  assert.ok(!notes.some((n) => n.text === 'The question for reply one.'), 'never also a note')
  // u2 is kept: a later reply naming it gets it, once
  l.pin({ responseId: 'r2', utteranceId: 'u2' })
  assert.equal(l.commit({ responseId: 'r2' })?.text, 'An unrelated aside.')
  l.stop()
})

test('a first linked reply with no matching words stays unpaired; it never consumes another utterance', async () => {
  const notes = []
  const l = new VoiceLedger({ onNote: (n) => notes.push(n), timing: { adjacentHoldMs: 60_000, pairGraceMs: 10, holdMs: 20 } })
  l.transcript({ id: 'e1', text: 'Words for nobody.', utterance_id: 'u7', utterance_seq: 0, final: true, association: 'unknown' })
  l.pin({ responseId: 'r1', utteranceId: 'u9' })
  assert.equal(l.commit({ responseId: 'r1' }), null)
  await new Promise((r) => setTimeout(r, 60))
  assert.deepEqual(notes.map((n) => n.text), ['Words for nobody.'], 'shown on its own, once')
  l.stop()
})

test('an explicit provider association after unknown parts: buffered words keep their text, no duplicate', async () => {
  const notes = []
  const l = new VoiceLedger({ onNote: (n) => notes.push(n), timing: { adjacentHoldMs: 60_000, pairGraceMs: 10, holdMs: 20 } })
  l.transcript({ id: 'e1', text: 'Earlier unknown words.', utterance_id: 'u1', utterance_seq: 0, final: true, association: 'unknown' })
  l.transcript({ id: 'e2', text: 'Named words.', utterance_id: 'u2', utterance_seq: 0, final: true, association: 'provider', response_id: 'r2' })
  l.pin({ responseId: 'r2', utteranceId: null })
  assert.equal(l.commit({ responseId: 'r2' })?.text, 'Named words.')
  await new Promise((r) => setTimeout(r, 60))
  assert.deepEqual(notes.map((n) => n.text), ['Earlier unknown words.'], 'the earlier words are shown once, on their own')
  l.stop()
})

test('control: the ordinary Gemini path (no id ever) still pairs by adjacency', () => {
  const l = new VoiceLedger({ timing: { adjacentHoldMs: 60_000 } })
  l.transcript({ id: 'e1', text: 'Plain words.', utterance_id: 'g-u1', utterance_seq: 0, association: 'unknown' })
  l.pin({ responseId: 'r1', utteranceId: null })
  assert.deepEqual([l.commit({ responseId: 'r1' })?.text, l.commit({ responseId: 'r1' })], ['Plain words.', null])
  l.stop()
})

// ── an unconfirmed stop is never shown as a confirmed one ──

test('a stop with no host acknowledgement keeps its uncertainty; a confirmed cancel is "interrupted"', async () => {
  const b = new RthBridge({ host: { cancel: async () => ({ status: 503, body: { error: 'unavailable' } }) }, observer: { start() {}, stop() {} }, timing: { stopMs: 5 } })
  b.ingest({ kind: 'decision', id: 'g:2', decision_id: 'd1', policy: 'provider:gemini-auto-vad@v1' })
  b.ingest({ kind: 'response_started', id: 'g:3', response_id: 'r1', request_id: 'mic', cause: 'g:2' })
  let final
  for await (const x of b.runVoice('r1', AbortSignal.abort())) if (x.type === 'final') final = x
  assert.equal(final.status, 'stop_unconfirmed')
  assert.match(voiceFinalText(final), /unconfirmed/)
  b.stop()
  assert.equal(voiceFinalText({ status: 'stop_unconfirmed', text: '' }), '*stop unconfirmed*')
  assert.equal(voiceFinalText({ status: 'stop_unconfirmed', text: 'Half a sentence' }), 'Half a sentence *— stop unconfirmed*')
  assert.equal(voiceFinalText({ status: 'cancelled', text: 'Half a sentence' }), 'Half a sentence *— interrupted*', 'a confirmed terminal')
  assert.doesNotMatch(voiceFinalText({ status: 'stop_unconfirmed', text: 'x' }), /stopped\*/)
})
