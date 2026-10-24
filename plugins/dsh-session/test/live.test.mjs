// The live "listening…" view: a pure reducer over speech facts, with an injected clock. Replayed
// here on the shape of a recorded voice call (its kinds and times, synthetic text), host onsets
// included.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { LiveView, LIVE_TIMING } from '../src/live.mjs'

function rig(timing) {
  let t = 0
  const views = []
  const live = new LiveView({ now: () => t, timing, onChange: (v) => views.push(v) })
  return { live, views, at: (ms) => { t = ms; live.tick() }, get t() { return t } }
}

test('a clean onset (speaker silent) starts listening at once, with the level at onset; the words replace it; the reply ends it', () => {
  const r = rig()
  r.live.input({ type: 'onset', audible: false, level_dbfs: -18.7 })
  assert.deepEqual(r.live.view(), { state: 'listening', since: 0, level: { dbfs: -18.7, basis: 'level at speech onset (rth-host)' }, text: null })
  r.at(240)
  r.live.input({ type: 'speech-start' })
  assert.equal(r.live.view().state, 'listening', 'the provider confirms, nothing changes')
  r.at(7700)
  r.live.input({ type: 'heard', text: 'Synthetic utterance 1.' })
  assert.deepEqual([r.live.view().state, r.live.view().text], ['heard', 'Synthetic utterance 1.'])
  r.live.input({ type: 'reply' })
  assert.equal(r.live.view().state, 'idle', 'the words are in the chat now')
})

test('an onset while the speaker plays (possible echo) does not start listening; the provider signal does', () => {
  const r = rig()
  r.live.input({ type: 'onset', audible: true, level_dbfs: -31.8 })
  assert.equal(r.live.view().state, 'idle')
  r.live.input({ type: 'speech-start' })
  assert.deepEqual(r.live.view(), { state: 'listening', since: 0, level: null, text: null })
})

test('nothing transcribed: it goes away cleanly (after the end of speech, or an onset nobody confirmed)', () => {
  const r = rig()
  r.live.input({ type: 'speech-start' })
  r.at(2000)
  r.live.input({ type: 'speech-end' })
  r.at(2000 + LIVE_TIMING.endGraceMs - 1)
  assert.equal(r.live.view().state, 'listening')
  r.at(2000 + LIVE_TIMING.endGraceMs)
  assert.equal(r.live.view().state, 'idle')
  r.live.input({ type: 'onset', audible: false, level_dbfs: -14 })
  r.at(5000 + LIVE_TIMING.onsetOnlyMs)
  assert.equal(r.live.view().state, 'idle', 'an onset the provider never confirmed fades')
  r.live.input({ type: 'speech-start' })
  r.at(10_000 + LIVE_TIMING.maxMs)
  assert.equal(r.live.view().state, 'idle', 'never stuck on')
})

test('words no reply takes: shown as heard until the chat shows them on their own', () => {
  const r = rig()
  r.live.input({ type: 'speech-start' })
  r.live.input({ type: 'heard', text: 'Synthetic aside.' })
  r.live.input({ type: 'note' })
  assert.equal(r.live.view().state, 'idle')
  r.live.input({ type: 'speech-start' })
  r.live.input({ type: 'heard', text: 'More.' })
  r.at(LIVE_TIMING.heardMs)
  assert.equal(r.live.view().state, 'idle', 'bounded even if no note is seen')
})

test('a new utterance while the previous words wait: listening again, the old words cleared', () => {
  const r = rig()
  r.live.input({ type: 'speech-start' })
  r.live.input({ type: 'heard', text: 'First.' })
  r.at(500)
  r.live.input({ type: 'speech-start' })
  assert.deepEqual([r.live.view().state, r.live.view().text, r.live.view().since], ['listening', null, 500])
})

test('a recorded call\'s shape: listening appears at each clean onset, before the provider and the words, and never stays on', () => {
  const r = rig()
  const fx = readFileSync(new URL('fixtures/voice-call-shape.jsonl', import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))
  const shown = []
  for (const x of fx) {
    r.at(x.t_ms)
    const e = x.env
    const before = r.live.view().state
    if (x.lane === 'host' && e.kind === 'local_vad_onset') r.live.input({ type: 'onset', audible: e.audible, level_dbfs: e.level_dbfs })
    else if (e.kind === 'provider_signal' && e.signal === 'voice_activity_start') r.live.input({ type: 'speech-start' })
    else if (e.kind === 'provider_signal' && e.signal === 'voice_activity_end') r.live.input({ type: 'speech-end' })
    else if (e.kind === 'input_transcript') r.live.input({ type: 'heard', text: e.text })
    else if (e.kind === 'response_started') r.live.input({ type: 'reply' })
    if (before === 'idle' && r.live.view().state === 'listening') shown.push({ at: x.t_ms, by: x.lane === 'host' ? 'onset' : 'provider' })
  }
  r.at(fx.at(-1).t_ms + LIVE_TIMING.maxMs)
  assert.equal(r.live.view().state, 'idle')
  const byOnset = shown.filter((s) => s.by === 'onset').length
  assert.ok(shown.length >= 20, `listening shown for every utterance (${shown.length})`)
  assert.ok(byOnset >= 10, `most start at the host onset, ahead of the provider (${byOnset})`)
  // In the recorded call the words arrived 2.0–7.8 s after speech began: the bubble covered that wait.
  // Each utterance enters `heard` once (a deadline update is published too, so count entries).
  assert.equal(r.views.filter((v, i) => v.state === 'heard' && r.views[i - 1]?.state !== 'heard').length, 20)
  assert.ok(r.views.every((v) => v.state === 'idle' ? v.ttl_ms === null : v.ttl_ms > 0 && v.ttl_ms <= 30_000), 'every published view carries its lifetime')
})
