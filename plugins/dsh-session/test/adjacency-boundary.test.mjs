// Adjacency boundaries (src/voice.mjs): Gemini's adapter keeps ONE utterance_id for a
// whole call and never sends `final`, so the provider's voice_activity_start is the only boundary
// between two utterances that no reply separates. Without it they would merge again.
import test from 'node:test'
import assert from 'node:assert/strict'
import { RthBridge } from '../src/bridge.mjs'

function setup() {
  const b = new RthBridge({ host: { calls: [] }, observer: { start() {}, stop() {} }, timing: { voiceIdleMs: 2000, voice: { adjacentHoldMs: 60_000 } } })
  b.onLink('attached')
  const seen = []
  b.onChange((kind, data) => { if (kind === 'voice-heard' || kind === 'voice-opened') seen.push([kind, data.text ?? data.heard]) })
  let seq = 0
  const g = (kind, f = {}) => ({ v: 2, id: `guest-x:${++seq}`, producer: 'guest-x', seq, source: 'guest', kind, ...f })
  return { b, seen, g }
}

test('two utterances under one utterance_id, no reply between: voice_activity_start keeps them two bubbles', () => {
  const { b, seen, g } = setup()
  b.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  b.ingest(g('input_transcript', { text: 'First words.', utterance_id: 'x-u1', utterance_seq: 0, association: 'unknown' }))
  b.ingest(g('provider_signal', { signal: 'voice_activity_end', response_id: null }))
  b.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  b.ingest(g('input_transcript', { text: 'Second words.', utterance_id: 'x-u1', utterance_seq: 1, association: 'unknown' }))
  const d = g('decision', { decision_id: 'x-d1', policy: 'provider:gemini-auto-vad@v1', selected: 'respond' })
  b.ingest(d)
  b.ingest(g('response_started', { cause: d.id, response_id: 'x-v1', request_id: 'x-mic' }))
  assert.deepEqual(seen, [['voice-heard', 'First words.'], ['voice-opened', 'Second words.']])
  b.stop()
})

// "Stop." continues the utterance id whose incomplete words the first reply took, so it is shown on
// its own (a note), never a reply's bubble. The words are preserved.
test('a barge-in: speech that starts during a reply is never the current one\'s; it is shown on its own', () => {
  const { b, seen, g } = setup()
  b.ingest(g('input_transcript', { text: 'Count to ten.', utterance_id: 'x-u1', utterance_seq: 0, association: 'unknown' }))
  const d1 = g('decision', { decision_id: 'x-d1', policy: 'provider:gemini-auto-vad@v1', selected: 'respond' })
  b.ingest(d1)
  b.ingest(g('response_started', { cause: d1.id, response_id: 'x-v1', request_id: 'x-mic' }))
  b.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: 'x-v1' }))
  b.ingest(g('input_transcript', { text: 'Stop.', utterance_id: 'x-u1', utterance_seq: 1, association: 'unknown' }))
  b.ingest(g('agent_done', { cause: d1.id, response_id: 'x-v1', status: 'cancelled' }))
  const d2 = g('decision', { decision_id: 'x-d2', policy: 'provider:gemini-auto-vad@v1', selected: 'respond' })
  b.ingest(d2)
  b.ingest(g('response_started', { cause: d2.id, response_id: 'x-v2', request_id: 'x-mic' }))
  assert.deepEqual(seen, [['voice-opened', 'Count to ten.'], ['voice-heard', 'Stop.'], ['voice-opened', null]])
  b.stop()
})

test('the live view gets the words also when a part closes its utterance (final: true), and for a backend that can pair', () => {
  for (const pairing of [false, true]) {
    const { b, g } = setup()
    if (pairing) b.ledger.adjacency = false
    const live = []
    b.onChange((kind, data) => { if (kind === 'voice-live' && data.type === 'heard') live.push(data.text) })
    b.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
    b.ingest(g('input_transcript', { text: 'Closed words.', utterance_id: 'x-u7', utterance_seq: 0, final: true, association: 'unknown' }))
    assert.deepEqual(live, ['Closed words.'], pairing ? 'id contract' : 'adjacency')
    b.stop()
  }
})
