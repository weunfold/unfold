// Mind's simple view (src/moments.mjs): plain moments from Mind's sentences, with the
// honesty rules kept (a transcript is not "you said", nothing open is shown live when the panel
// is not live, a decision is not "thinking").
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { decidedTo, gapsIn, moments, nowLine, quietLine, quoted, usageOf } from '../src/moments.mjs'

let n = 0
const row = (kind, sentence, extra = {}) => ({ id: `m${++n}`, lane: 'agentd', kind, sentence, status: 'observed', observed_at: 1000 + n * 100, episode: null, ...extra })
const live = { stream: { state: 'live', label: 'Mind stream: live (pushed)' }, agent: { state: 'live', label: '' } }

test('quoted text comes only from the Mind sentence', () => {
  assert.equal(quoted('Provider transcribed “Oi RTH” (final; speaker not identified)'), 'Oi RTH')
  assert.equal(quoted('Agent note (info): “a “nested” one”'), 'a “nested” one')
  assert.equal(quoted('no quote here'), null)
  assert.equal(decidedTo('Decided to look_up (of respond, look_up) by rule'), 'look_up')
})

test('a conversation: heard on one side, the reply on the other, small things as quiet lines', () => {
  const ep = { id: 'ep1', status: 'completed', duration_ms: 2300 }
  const heard = row('input_transcript', 'Provider transcribed “Que horas é?” (final; speaker not identified)', { episode: 'ep1' })
  const vad = row('local_vad_onset', 'Heard a possible start of talking on mic-1 (speech candidate …). Not proof anyone spoke.', { lane: 'host' })
  const d = row('decision', 'Decided to respond (of respond, wait) by rule:always-respond@v0', { episode: 'ep2', status: 'completed' })
  const st = row('response_started', 'Started reply r1 to request unknown', { episode: 'ep2', status: 'pending' })
  const delta = row('agent_delta', 'Wrote reply text for r1: “Às dez.” (2 parts)', { episode: 'ep2' })
  const play = row('playback', 'Playback finished for r1: …', { episode: 'ep2', lane: 'host', status: 'completed' })
  const note = row('log', 'Agent note (info): “history saved”')
  const ms = moments([heard, vad, d, st, delta, play, note], { ep1: ep, ep2: { id: 'ep2', status: 'completed', duration_ms: 900 } })
  assert.deepEqual(ms.map((m) => [m.type, m.text]), [['heard', 'Que horas é?'], ['reply', 'Às dez.']], 'an info note is not a line in the conversation')
  assert.equal(ms[0].final, true)
  assert.equal(ms[1].spoken, true, 'played: the host reported handing the audio to the device')
  assert.ok(!ms.some((m) => /you said|the owner|spoke/i.test(m.text)), 'a transcript or a speech candidate is never "you said"')
})

test('not live: nothing open is shown as happening, and the top line says so', () => {
  const st = row('response_started', 'Started reply r9 to request unknown', { episode: 'ep9', status: 'pending' })
  const eps = { ep9: { id: 'ep9', status: 'pending' } }
  const on = moments([st], eps, { live: true })
  assert.equal(on[0].open, true)
  assert.equal(nowLine(on, live, st.observed_at + 10).text, 'Replying…')
  const off = moments([st], eps, { live: false })
  assert.equal(off[0].open, false)
  assert.equal(off[0].status, 'unknown')
  assert.equal(nowLine(off, { stream: { state: 'stale', label: 'Mind stream: no word for 20 s (stale; nothing below is live)' }, agent: { state: 'unknown' } }, 0).text, 'Out of touch, reconnecting')
  assert.match(nowLine(off, { stream: live.stream, agent: { state: 'stale' } }, 0).text, /^Not sure/)
})

test('a decision with no reply yet says what was decided, never thinking', () => {
  const d = row('decision', 'Decided to look_up (of respond, look_up, wait) by rule', { episode: 'epd', status: 'completed' })
  const ms = moments([d], { epd: { id: 'epd', status: 'completed' } })
  assert.equal(ms[0].text, 'Chose to look something up first')
  const top = nowLine(ms, live, d.observed_at + 1000)
  assert.equal(top.text, 'Chose to look something up first, no reply yet')
  assert.doesNotMatch(top.text, /think/i)
  const later = nowLine(ms, live, d.observed_at + 61_000)
  assert.equal(later.state, 'unknown', 'it stops looking busy, but time alone never makes it quiet')
  assert.match(later.text, /no reply seen since/)
})

test('quiet lines: raw logs never reach the conversation; gaps are counted, not lined; repeats fold', () => {
  assert.equal(quietLine({ status: 'gap', kind: 'seq_gap', sentence: '' }), null)
  assert.equal(gapsIn([{ status: 'gap', kind: 'seq_gap' }, { status: 'gap', kind: 'seq_gap' }, { status: 'gap', kind: 'link_lost' }]), 2)
  assert.equal(quietLine(row('mute_applied', 'Microphone muted at the host (generation 2, 0 partial samples discarded)', { lane: 'host' })).text, 'Microphone muted')
  assert.equal(quietLine(row('mute_applied', 'Microphone unmuted at the host (generation 3, …)', { lane: 'host' })).text, 'Microphone unmuted')
  assert.equal(quietLine(row('heartbeat', 'Agent is up')), null, 'heartbeats are not moments')
  const usage = row('log', 'Agent note (info): “gemini usage gen=255 total_tokens=629”')
  assert.equal(quietLine(usage), null, 'a usage line is metrics, never a quoted thought (the owner\'s screenshot)')
  assert.deepEqual(usageOf(usage), { gen: 255, total: 629 })
  assert.equal(quietLine(row('log', 'Agent note (info): “provider session healthy”')), null)
  assert.equal(quietLine(row('log', 'Agent note (warn): “socket slow”')).text, 'Unfold reported a warning')
  const a = row('log', 'Agent note (error): “boom”')
  const b = row('log', 'Agent note (error): “boom”')
  const ms = moments([a, b], {})
  assert.equal(ms.length, 1)
  assert.equal(ms[0].text, 'Unfold reported an error')
  assert.equal(ms[0].count, 2)
  for (const m of moments([usage, row('log', 'Agent note (info): “x”')], {})) assert.doesNotMatch(m.text, /gemini usage|total_tokens/)

})

test('a host journal discontinuity is one quiet line; an agent journal note or other gap stays under More', () => {
  const mark = (kind, sentence) => ({ lane: 'mind', status: 'gap', kind, sentence })
  for (const t of ['replaced', 'shrank', 'rewritten', 'missing']) assert.match(quietLine(mark(`journal_${t}`, 'Host journal: x')).text, /host log changed files: its timings before and after aren't compared/)
  assert.equal(quietLine(mark('journal_replaced', 'Agent journal: x')), null)
  assert.equal(quietLine(mark('journal_truncated_history', 'Host journal: x')), null)
})
