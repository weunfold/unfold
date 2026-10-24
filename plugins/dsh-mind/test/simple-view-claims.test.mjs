// Three claims the simple view must never make, each with its neighbouring cases.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import * as M from '../src/moments.mjs'

const { moments, nowLine } = M
const row = (id, kind, sentence, episode, at, extra = {}) => ({ id, kind, sentence, episode, observed_at: at, lane: 'agentd', status: 'observed', ...extra })
const fresh = { stream: { state: 'live', label: 'Mind stream: live (pushed)' }, agent: { state: 'live', label: '' } }

test('a decision to look something up is never shown as a lookup that ran, whatever the reply', () => {
  assert.equal(typeof M.captionOf, 'function', 'the caption is a pure, tested function of the moment')
  for (const status of ['completed', 'failed', 'stopped', 'unknown', 'pending']) {
    for (const text of ['Memory unavailable.', 'Você anotou três coisas.', null]) {
      const rows = [
        row('d', 'decision', 'Decided to look_up (of respond, look_up) by rule', 'lookup', 1000, { status: 'completed' }),
        row('s', 'response_started', 'Started reply r1', 'lookup', 1100, { status: 'pending' }),
        ...(text ? [row('t', 'agent_delta', `Wrote reply text for r1: “${text}”`, 'lookup', 1200)] : []),
      ]
      const ms = moments(rows, { lookup: { id: 'lookup', status } })
      const reply = ms.find((m) => m.type === 'reply')
      assert.doesNotMatch(M.captionOf(reply, 1300), /looked|look(ed)? (something )?up|found|remembered/i, `${status}/${text}`)
      for (const m of ms) assert.doesNotMatch(m.text ?? '', /looked something up|found/i, 'the decision line keeps the decision wording')
      assert.equal(ms.find((m) => m.key.startsWith('d:'))?.text, 'Chose to look something up first')
    }
  }
})

test('heard moments are grouped by Mind episodes (explicit ids) only, never by text or time', () => {
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /startsWith\(prev\.text\)|mergePartials/, 'no text-similarity join in the client')
  const eps = { utteranceA: { id: 'utteranceA', status: 'unknown' }, utteranceB: { id: 'utteranceB', status: 'completed' } }
  // Distinct episodes, prefix text, 1 s apart.
  const ab = moments([
    row('a', 'input_transcript', 'Provider transcribed “Hi” (not final; speaker not identified)', 'utteranceA', 2000),
    row('b', 'input_transcript', 'Provider transcribed “Hi again” (final; speaker not identified)', 'utteranceB', 3000),
  ], eps)
  assert.deepEqual(ab.map((m) => [m.episode, m.text, m.final, m.rows]), [['utteranceA', 'Hi', false, ['a']], ['utteranceB', 'Hi again', true, ['b']]])
  // Identical text in distinct episodes stays two moments.
  const same = moments([
    row('a', 'input_transcript', 'Provider transcribed “Oi” (final; speaker not identified)', 'utteranceA', 2000),
    row('b', 'input_transcript', 'Provider transcribed “Oi” (final; speaker not identified)', 'utteranceB', 2100),
  ], eps)
  assert.equal(same.length, 2)
  // No episode (no shared id): each transcript is its own moment.
  const loose = moments([
    row('a', 'input_transcript', 'Provider transcribed “Me” (not final; speaker not identified)', null, 2000),
    row('b', 'input_transcript', 'Provider transcribed “Me lembra” (not final; speaker not identified)', null, 2200),
  ], {})
  assert.deepEqual(loose.map((m) => m.rows), [['a'], ['b']])
  // A genuine same-id utterance: one moment with the latest text, even when parts arrive reordered.
  const one = moments([
    row('b', 'input_transcript', 'Provider transcribed “Me lembra de ligar” (final; speaker not identified)', 'u1', 2400),
    row('a', 'input_transcript', 'Provider transcribed “Me lembra” (not final; speaker not identified)', 'u1', 2000),
  ], { u1: { id: 'u1', status: 'observed' } })
  assert.deepEqual(one.map((m) => [m.text, m.final, m.rows.length]), [['Me lembra de ligar', true, 2]])
})

test('time alone never turns an unresolved reply or decision into "Quiet"', () => {
  const open = moments([row('s2', 'response_started', 'Started reply r2', 'slow', 1000, { status: 'pending' })], { slow: { id: 'slow', status: 'pending' } })
  assert.equal(nowLine(open, fresh, 2000).text, 'Replying…')
  for (const at of [92_000, 600_000, 86_400_000]) {
    const h = nowLine(open, fresh, at)
    assert.notEqual(h.state, 'idle', `at ${at} ms`)
    assert.notEqual(h.text, 'Quiet')
    assert.equal(h.state, 'unknown')
    assert.equal(h.stale, true)
    assert.match(M.captionOf(open[0], at), /no end seen/)
  }
  // A real outcome lets it go quiet.
  const done = moments([
    row('s2', 'response_started', 'Started reply r2', 'slow', 1000, { status: 'pending' }),
    row('e2', 'agent_done', 'Reply r2 ended: completed', 'slow', 5000, { status: 'completed' }),
  ], { slow: { id: 'slow', status: 'completed' } })
  assert.equal(nowLine(done, fresh, 600_000).text, 'Quiet')
  // A stop settles only through the reply's own end row, not the episode status alone.
  const stopped = moments([row('s3', 'response_started', 'Started reply r3', 'x', 1000, { status: 'pending' }), row('c3', 'agent_done', 'Reply r3 ended: stopped', 'x', 1500, { status: 'stopped' })], { x: { id: 'x', status: 'stopped' } })
  assert.equal(nowLine(stopped, fresh, 600_000).text, 'Quiet')
  const statusOnly = moments([row('s4', 'response_started', 'Started reply r4', 'y', 1000, { status: 'pending' })], { y: { id: 'y', status: 'stopped' } })
  assert.notEqual(nowLine(statusOnly, fresh, 600_000).text, 'Quiet', 'an episode status without the reply\'s own end does not settle it')
  // A decision with no reply: busy at first, then "no reply seen since", never quiet.
  const d = moments([row('d', 'decision', 'Decided to look_up (of respond, look_up) by rule', 'dd', 1000, { status: 'completed' })], { dd: { id: 'dd', status: 'completed' } })
  assert.equal(nowLine(d, fresh, 2000).state, 'busy')
  const later = nowLine(d, fresh, 1000 + 61_000)
  assert.equal(later.state, 'unknown')
  assert.match(later.text, /no reply seen since/)
  // A decision to wait is not unresolved work.
  const w = moments([row('w', 'decision', 'Decided to wait (of respond, wait) by rule', 'ww', 1000, { status: 'completed' })], { ww: { id: 'ww', status: 'completed' } })
  assert.equal(nowLine(w, fresh, 1000 + 61_000).text, 'Quiet')
})
