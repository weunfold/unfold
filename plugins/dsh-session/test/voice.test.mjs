// Spoken turns in the bridge: synthetic event streams built from the catalog kinds
// (decision with policy provider:gemini-auto-vad@v1, response_started, agent_delta, agent_done,
// provider_signal, input_transcript with the agent's utterance fields), and two recorded journals
// (test/fixtures). Projection only: no test may see a host submit for a spoken turn.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { RthBridge } from '../src/bridge.mjs'
import { VoiceLedger } from '../src/voice.mjs'
import { finalText, heardNoteText, voiceUserText } from '../index.mjs'

const AUTO_VAD = 'provider:gemini-auto-vad@v1'
const fast = { stopMs: 200, reaskMs: [20, 40, 80], reconcileAttempts: 3, voiceGapWaitMs: 60, voiceIdleMs: 2000 }

function fakeHost({ replies } = {}) {
  const calls = []
  const host = {
    calls,
    async submit(command_id, text) {
      calls.push({ op: 'submit', command_id, text })
      return { status: 200, body: { command: { command_id, status: 'dispatched', generation: 1, response_id: `resp-${command_id}` } } }
    },
    async cancel() {
      calls.push({ op: 'cancel' })
      return { status: 200, body: { ok: true } }
    },
  }
  if (replies) {
    host.reply = async (responseId) => {
      calls.push({ op: 'reply', responseId })
      const r = replies[responseId]
      return r ? { status: 200, body: { reply: r } } : { status: 404, body: { error: 'no such route' } }
    }
  }
  return host
}

// Clean bubbles: until a backend shows it can pair, transcripts are grouped by adjacency
// (src/voice.mjs). `pairing: true` turns adjacency off: the strict id contract; the default is
// Gemini's shape. `voice-live` (the listening view) is not a bubble.
function setup({ host = fakeHost(), timing = fast, pairing = false } = {}) {
  const b = new RthBridge({ host, observer: { start() {}, stop() {} }, timing })
  if (pairing) b.ledger.adjacency = false
  b.onLink('attached')
  const seen = []
  b.onChange((kind, data) => { if (kind.startsWith('voice') && kind !== 'voice-live') seen.push({ kind, ...data }) })
  return { b, host, seen, opened: () => seen.filter((e) => e.kind === 'voice-opened'), notes: () => seen.filter((e) => e.kind === 'voice-heard') }
}

/** Guest envelopes as agentd journals them: `<producer>:<seq>` ids, seq growing. */
function guest(producer = 'guest-b1') {
  let seq = 0
  return (kind, fields = {}) => ({ v: 2, id: `${producer}:${++seq}`, producer, seq, t_ns: seq, source: 'guest', kind, ...fields })
}

/**
 * One provider-opened turn as the agent emits it: the Decision (cause = the last admitted mic
 * frame), response_started caused by the Decision envelope, deltas, then the terminal.
 * `ids` adds utterance_id to the Decision and response (a backend that can prove the link).
 */
function spokenTurn(g, { n, reply = ['Hello ', 'there.'], status = 'completed', utterance = null, signal = 'turn_complete' }) {
  const d = g('decision', { decision_id: `b1-d${n}`, trigger_event: `host:${n}`, selected: 'respond', candidates: ['respond', 'wait'], policy: AUTO_VAD, ...(utterance ? { utterance_id: utterance } : {}) })
  const rid = `b1-v${n}`
  const evs = [d, g('response_started', { cause: d.id, response_id: rid, request_id: 'b1-mic-1', ...(utterance ? { utterance_id: utterance } : {}) })]
  for (const text of reply) evs.push(g('agent_delta', { cause: d.id, response_id: rid, text }))
  evs.push(g('agent_done', { cause: d.id, response_id: rid, status }))
  if (signal) evs.push(g('provider_signal', { cause: d.id, response_id: rid, signal }))
  return { rid, decisionId: `b1-d${n}`, evs }
}

const heard = (g, u, parts, { final = true, association = 'unknown', response_id } = {}) =>
  parts.map((text, i) => g('input_transcript', { text, utterance_id: u, utterance_seq: i, ...(final && i === parts.length - 1 ? { final: true } : {}), association, ...(response_id ? { response_id } : {}) }))

async function collect(gen) {
  const evs = []
  for await (const ev of gen) evs.push(ev)
  return evs
}

const submits = (host) => host.calls.filter((c) => c.op === 'submit')
const fixture = (name) => readFileSync(new URL(`fixtures/${name}`, import.meta.url), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))

test('a spoken turn (Gemini shape): the words heard before it are the reply\'s, by adjacency; no note, no command', async () => {
  const { b, host, opened, notes } = setup()
  const g = guest()
  for (const e of heard(g, 'b1-u1', ['What is the capital of France?'])) b.ingest(e)
  const turn = spokenTurn(g, { n: 1, reply: ['The capital of ', 'France is Paris.'] })
  turn.evs.forEach((e) => b.ingest(e))
  assert.equal(notes().length, 0, 'the words are not a separate note')
  assert.equal(opened().length, 1, 'one spoken reply')
  assert.equal(opened()[0].heard, 'What is the capital of France?', 'its queued text is the words')
  assert.equal(opened()[0].decisionId, 'b1-d1')
  const evs = await collect(b.runVoice(turn.rid))
  const final = evs.at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'The capital of France is Paris.')
  assert.equal(final.voice.decisionId, 'b1-d1')
  assert.deepEqual(evs.filter((e) => e.type === 'delta').map((e) => e.text), ['The capital of ', 'France is Paris.'])
  const h = b.voiceCommit(final.voice)
  assert.deepEqual({ text: h.text, adjacent: h.adjacent }, { text: 'What is the capital of France?', adjacent: true })
  assert.equal(voiceUserText(h), 'What is the capital of France?', 'the words only')
  assert.equal(voiceUserText(null), null, 'nothing heard: no user bubble')
  assert.equal(heardNoteText({ text: '  Okay. ' }), 'Okay.')
  assert.equal(heardNoteText({ text: '' }), null)
  assert.equal(host.calls.length, 0, 'no command, no re-ask, no cancel')
})

test('arrival order: words heard before a reply are its own; earlier unanswered words are shown first', () => {
  const { b, seen } = setup()
  const g = guest()
  for (const e of heard(g, 'b1-u0', ['unanswered'])) b.ingest(e)
  b.ingest(g('provider_signal', { signal: 'voice_activity_start', response_id: null }))
  for (const e of heard(g, 'b1-u1', ['hi'])) b.ingest(e)
  spokenTurn(g, { n: 1 }).evs.forEach((e) => b.ingest(e))
  assert.deepEqual(seen.map((e) => [e.kind, e.text ?? e.heard]), [['voice-heard', 'unanswered'], ['voice-opened', 'hi']])
})

test('arrival order, a backend that can pair: the heard note is offered before the reply that follows it', () => {
  const { b, seen } = setup({ pairing: true })
  const g = guest()
  for (const e of heard(g, 'b1-u1', ['hi'])) b.ingest(e)
  spokenTurn(g, { n: 1 }).evs.forEach((e) => b.ingest(e))
  assert.deepEqual(seen.map((e) => e.kind), ['voice-heard', 'voice-opened'])
})

test('a voice turn from a backend that names it: 1 user voice bubble (the transcript) and 1 reply', async () => {
  const { b, host, opened, notes } = setup()
  const g = guest()
  // Until the backend has named an utterance on a reply once, nothing waits for a pairing (a
  // final transcript is shown at once, in arrival order). A first turn shows it can.
  spokenTurn(g, { n: 0, utterance: 'b1-u0', reply: ['Ready.'] }).evs.forEach((e) => b.ingest(e))
  await collect(b.runVoice('b1-v0'))
  for (const e of heard(g, 'b1-u1', ['What is ', 'two plus two?'])) b.ingest(e)
  const turn = spokenTurn(g, { n: 1, utterance: 'b1-u1', reply: ['Four.'] })
  turn.evs.forEach((e) => b.ingest(e))
  assert.equal(opened().length, 2)
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  const h = b.voiceCommit(final.voice)
  assert.deepEqual({ text: h.text, final: h.final, utteranceId: h.utteranceId }, { text: 'What is two plus two?', final: true, utteranceId: 'b1-u1' })
  assert.equal(voiceUserText(h), 'What is two plus two?')
  assert.equal(final.text, 'Four.')
  await new Promise((r) => setTimeout(r, 30))
  assert.equal(notes().length, 0, 'a paired transcript is not also a note')
  assert.equal(submits(host).length, 0)
})

test('incremental transcripts coalesce by (utterance_id, utterance_seq); replays and reorders do not duplicate', async () => {
  const { b, notes } = setup({ timing: { ...fast, voice: { holdMs: 40, adjacentHoldMs: 40 } } })
  const g = guest()
  const parts = heard(g, 'b1-u7', ['Please ', 'count ', 'to three.'])
  // out of order, with a replayed part (same utterance_seq, new envelope) and a replayed envelope
  b.ingest(parts[1])
  b.ingest(parts[0])
  b.ingest(parts[1])
  b.ingest({ ...parts[0], id: 'guest-b1:999', seq: 999, text: 'PLEASE ' })
  b.ingest(parts[2])
  b.ingest(parts[2])
  await new Promise((r) => setTimeout(r, 80))
  assert.equal(notes().length, 1, 'one note for the whole utterance')
  assert.equal(notes()[0].text, 'Please count to three.')
  assert.equal(notes()[0].final, true)
  assert.ok(b.ledger.stats.duplicates >= 3)
})

test('a transcript is final only when a part said final: true; otherwise the note is partial (id contract)', async () => {
  const { b, notes } = setup({ timing: { ...fast, voice: { holdMs: 30 } }, pairing: true })
  const g = guest()
  for (const e of heard(g, 'b1-u2', ['Name one ', 'planet'], { final: false })) b.ingest(e)
  assert.equal(notes().length, 0, 'not final: waits for more parts')
  await new Promise((r) => setTimeout(r, 60))
  assert.equal(notes().length, 1)
  assert.equal(notes()[0].final, false)
  assert.equal(heardNoteText(notes()[0]), 'Name one planet', 'partial or not, the chat shows the words only')
})

test('audio arriving before the final transcript changes nothing (audio is optional metadata; a backend that can pair)', async () => {
  const { b, opened, notes } = setup({ pairing: true })
  const g = guest()
  const [p0, p1] = heard(g, 'b1-u3', ['How far ', 'is the moon?'])
  b.ingest(p0)
  const turn = spokenTurn(g, { n: 3, reply: ['About 384,000 km.'] })
  b.ingest(turn.evs[0])
  b.ingest(turn.evs[1])
  b.ingest(g('agent_audio', { cause: turn.evs[0].id, response_id: turn.rid, data: '', sample_rate: 24000 }))
  b.ingest(g('media_stored', { response_id: turn.rid, ref: { sha256: 'ab' } }))
  b.ingest(p1)
  turn.evs.slice(2).forEach((e) => b.ingest(e))
  assert.equal(notes().length, 1)
  assert.equal(notes()[0].text, 'How far is the moon?')
  assert.equal(opened().length, 1)
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  assert.equal(final.text, 'About 384,000 km.')
  assert.equal(b.voiceCommit(final.voice), null, 'the part that arrived during the reply is not relabeled as its prompt')
})

test('a delayed prior transcript between reply A and reply B is never shown as B\'s prompt (id contract)', async () => {
  const { b, seen } = setup({ pairing: true })
  const g = guest()
  const a = spokenTurn(g, { n: 1, reply: ['Paris.'] })
  a.evs.forEach((e) => b.ingest(e))
  // A's question is transcribed only now, after A completed and before B's first output.
  for (const e of heard(g, 'b1-u1', ['What is the capital of France?'])) b.ingest(e)
  const bt = spokenTurn(g, { n: 2, reply: ['Mars.'] })
  bt.evs.forEach((e) => b.ingest(e))
  assert.deepEqual(seen.map((e) => e.kind), ['voice-opened', 'voice-heard', 'voice-opened'])
  const fa = (await collect(b.runVoice(a.rid))).at(-1)
  const fb = (await collect(b.runVoice(bt.rid))).at(-1)
  assert.equal(b.voiceCommit(fa.voice), null)
  assert.equal(b.voiceCommit(fb.voice), null, 'B has no paired prompt')
  assert.equal(fb.text, 'Mars.')
  assert.equal(seen[1].association, 'unknown')
})

test('delayed transcription for a backend that names its reply: a separate late note, the reply bubble untouched', async () => {
  const { b, notes } = setup({ timing: { ...fast, voice: { pairGraceMs: 10, holdMs: 40 } } })
  const g = guest()
  const turn = spokenTurn(g, { n: 4, utterance: 'b1-u4', reply: ['Yes.'] })
  turn.evs.forEach((e) => b.ingest(e))
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  assert.equal(b.voiceCommit(final.voice), null, 'nothing heard yet when the bubble is written')
  for (const e of heard(g, 'b1-u4', ['Is it ', 'raining?'])) b.ingest(e)
  await new Promise((r) => setTimeout(r, 40))
  assert.equal(notes().length, 1)
  assert.equal(notes()[0].late, true)
  assert.equal(notes()[0].responseId, turn.rid)
  assert.equal(heardNoteText(notes()[0]), 'Is it raining?', 'late, but the chat shows the words only (the link is in notes()[0].responseId)')
})

test('a missing transcript: the reply stands alone with its own marker', async () => {
  const { b, notes } = setup()
  const g = guest()
  const turn = spokenTurn(g, { n: 5, reply: ['Sure.'] })
  turn.evs.forEach((e) => b.ingest(e))
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(b.voiceCommit(final.voice), null)
  assert.equal(notes().length, 0)
})

test('reconnect mid-voice-reply: the recorded reply is taken when the owner has one (GET only, never submit)', async () => {
  const host = fakeHost({ replies: { 'b1-v6': { response_id: 'b1-v6', status: 'completed', text: 'One, two, three, four.', text_complete: true, truncated: false, source: 'observed', observed: 'complete' } } })
  const { b } = setup({ host })
  const g = guest()
  const turn = spokenTurn(g, { n: 6, reply: ['One, ', 'two, ', 'three, ', 'four.'] })
  const [d, rs, d1, , , d4, done] = turn.evs
  ;[d, rs, d1].forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached') // "two, three, " happened while we were away
  b.ingest(d4)
  b.ingest(done)
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'One, two, three, four.')
  assert.equal(final.recorded, true)
  assert.deepEqual(host.calls.map((c) => c.op), ['reply'])
})

test('reconnect mid-voice-reply with no recorded reply available (the owner answers 404): shown possibly incomplete', async () => {
  const host = fakeHost({ replies: {} })
  const { b } = setup({ host })
  const g = guest()
  const turn = spokenTurn(g, { n: 7, reply: ['One, ', 'two, ', 'three.'] })
  const [d, rs, d1, , d3, done] = turn.evs
  ;[d, rs, d1].forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached')
  b.ingest(d3)
  b.ingest(done)
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'One, three.')
  assert.equal(finalText(final), 'One, three. *— may be incomplete*', 'a short mark in chat')
  assert.match(final.detail, /the live stream had a gap; the reply ended \(completed\)/, 'the reason is kept on the record')
  assert.equal(submits(host).length, 0)
})

test('reconnect mid-voice-reply and the end is never seen: incomplete after the bounded wait', async () => {
  const { b, host } = setup()
  const g = guest()
  const turn = spokenTurn(g, { n: 8, reply: ['Counting: ', 'one'] })
  turn.evs.slice(0, 3).forEach((e) => b.ingest(e))
  const t0 = Date.now()
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached')
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.match(final.detail, /end of this spoken reply was not seen/)
  assert.ok(Date.now() - t0 < 1000)
  assert.equal(host.calls.length, 0)
})

test('a reply already seen whole before a reconnect stays completed', async () => {
  const { b } = setup()
  const g = guest()
  const turn = spokenTurn(g, { n: 9, reply: ['Done.'] })
  turn.evs.forEach((e) => b.ingest(e))
  b.onLink('waiting')
  b.onLink('attached')
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'Done.')
})

test('an interrupted voice reply (barge-in) is shown honestly as cut off, with the generated text', async () => {
  const { b, host } = setup()
  const g = guest()
  const turn = spokenTurn(g, { n: 10, reply: ['1, 2, 3, ', '4, 5,'], status: 'cancelled', signal: null })
  turn.evs.forEach((e) => b.ingest(e))
  const final = (await collect(b.runVoice(turn.rid))).at(-1)
  assert.equal(final.status, 'cancelled')
  assert.equal(final.text, '1, 2, 3, 4, 5,')
  assert.equal(finalText(final), '1, 2, 3, 4, 5, *— interrupted*', 'a subtle mark, not a paragraph')
  // the provider's interrupted signal, when it arrives first, is named
  const t2 = spokenTurn(g, { n: 11, reply: ['Sure, '], status: 'cancelled', signal: null })
  t2.evs.slice(0, 3).forEach((e) => b.ingest(e))
  b.ingest(g('provider_signal', { response_id: t2.rid, signal: 'interrupted' }))
  b.ingest(t2.evs[3])
  const f2 = (await collect(b.runVoice(t2.rid))).at(-1)
  assert.deepEqual(f2.voice.signals, ['interrupted'], 'the provider\'s interrupted signal is kept on the record')
  assert.equal(finalText(f2), 'Sure, *— interrupted*')
  assert.equal(host.calls.length, 0)
})

test('a stop pressed during a spoken reply asks the owner to cancel; no backend end -> stop_unconfirmed', async () => {
  const { b, host } = setup()
  const g = guest()
  const turn = spokenTurn(g, { n: 12 })
  turn.evs.slice(0, 3).forEach((e) => b.ingest(e))
  const ac = new AbortController()
  const run = collect(b.runVoice(turn.rid, ac.signal))
  setTimeout(() => ac.abort(), 5)
  const final = (await run).at(-1)
  assert.equal(final.status, 'stop_unconfirmed')
  assert.deepEqual(host.calls.map((c) => c.op), ['cancel'])
})

test('interleaved typed and voice turns: the typed one submits once, the spoken ones never', async () => {
  const { b, host, opened } = setup()
  const g = guest()
  const typed = collect(b.run('dsh-m1', 'hi'))
  await new Promise((r) => setTimeout(r, 5))
  // the typed reply starts, a spoken turn interleaves, then both finish
  b.ingest(g('response_started', { response_id: 'resp-dsh-m1', request_id: 'dsh-m1' }))
  b.ingest(g('agent_delta', { response_id: 'resp-dsh-m1', text: 'hi! ' }))
  const v = spokenTurn(g, { n: 13, reply: ['Spoken answer.'] })
  v.evs.slice(0, 3).forEach((e) => b.ingest(e))
  b.ingest(g('agent_delta', { response_id: 'resp-dsh-m1', text: "I'm alive." }))
  b.ingest(g('agent_done', { response_id: 'resp-dsh-m1', status: 'completed' }))
  v.evs.slice(3).forEach((e) => b.ingest(e))
  const tf = (await typed).at(-1)
  const vf = (await collect(b.runVoice(v.rid))).at(-1)
  assert.equal(tf.text, "hi! I'm alive.")
  assert.equal(vf.text, 'Spoken answer.')
  assert.equal(opened().length, 1, 'the typed response is not a spoken turn')
  assert.deepEqual(submits(host).map((c) => c.command_id), ['dsh-m1'])
})

test('replay: the same events again open nothing new and do not double the text', async () => {
  const { b, opened, notes } = setup()
  const g = guest()
  const evs = [...heard(g, 'b1-u14', ['Again?']), ...spokenTurn(g, { n: 14, reply: ['Once.'] }).evs]
  evs.forEach((e) => b.ingest(e))
  evs.slice(0, 4).forEach((e) => b.ingest(e)) // replayed before it is followed: deltas too
  const final = (await collect(b.runVoice('b1-v14'))).at(-1)
  evs.forEach((e) => b.ingest(e)) // and after
  assert.equal(opened().length, 1)
  assert.equal(notes().length, 0, 'the words went with the reply, once')
  assert.equal(opened()[0].heard, 'Again?')
  assert.equal(final.text, 'Once.')
  const again = (await collect(b.runVoice('b1-v14'))).at(-1)
  assert.equal(again.status, 'unknown', 'a second follow of the same reply never shows it twice')
})

test('a later spoken reply while an earlier one shows no end: the earlier is incomplete, never completed', async () => {
  const { b } = setup()
  const g = guest()
  const a = spokenTurn(g, { n: 15, reply: ['First '] })
  a.evs.slice(0, 3).forEach((e) => b.ingest(e))
  const bt = spokenTurn(g, { n: 16, reply: ['Second.'] })
  bt.evs.forEach((e) => b.ingest(e))
  const fa = (await collect(b.runVoice(a.rid))).at(-1)
  assert.equal(fa.status, 'incomplete')
  assert.match(fa.detail, /later spoken reply started/)
  assert.equal((await collect(b.runVoice(bt.rid))).at(-1).status, 'completed')
})

test('bounded memory (adjacency): closed segments are capped and the oldest shown, never lost', async () => {
  const { b, notes } = setup({ timing: { ...fast, voice: { adjacentHoldMs: 60_000 } } })
  const g = guest()
  for (let i = 0; i < 100; i++) b.ingest(g('input_transcript', { text: `u${i}`, utterance_id: `b1-u${1000 + i}`, utterance_seq: 0, association: 'unknown' }))
  const kept = b.ledger.closed.length + (b.ledger.segment ? 1 : 0)
  assert.ok(b.ledger.closed.length <= 32)
  assert.equal(notes().length, 100 - kept, 'the oldest are shown, not lost')
  assert.deepEqual(notes().slice(0, 2).map((n) => n.text), ['u0', 'u1'], 'in order')
  b.ingest(g('input_transcript', { text: 'x'.repeat(10_000), utterance_id: 'b1-big', utterance_seq: 0, final: true }))
  const big = b.ledger.closed.at(-1)
  assert.ok(big.chars <= 4000 && big.truncated, 'a part beyond the size bound is cut, and marked')
  b.stop()
})

test('bounded memory: open replies, utterances and seen ids are capped (id contract)', async () => {
  const { b, notes } = setup({ pairing: true })
  const g = guest()
  for (let n = 100; n < 140; n++) spokenTurn(g, { n }).evs.slice(0, 2).forEach((e) => b.ingest(e))
  assert.ok(b.voiceTracks.size <= b.timing.voiceOpen)
  for (let i = 0; i < 100; i++) b.ingest(g('input_transcript', { text: `u${i}`, utterance_id: `b1-u${1000 + i}`, utterance_seq: 0, association: 'unknown' }))
  assert.ok(b.ledger.utterances.size <= 32)
  assert.equal(notes().length, 100 - b.ledger.utterances.size, 'evicted utterances are shown, not lost')
  b.ingest(g('input_transcript', { text: 'x'.repeat(10_000), utterance_id: 'b1-big', utterance_seq: 0, final: true }))
  assert.ok(notes().at(-1).text.length <= 4000)
  assert.ok(b.voiceDecisions.size <= 64 && b.voiceSeen.set.size <= 256)
  b.stop()
})

test('recorded journal (Gemini proof, old shape): the spoken turn takes the words heard before it; the typed turn is not spoken', async () => {
  const { b, host, opened, notes } = setup()
  const evs = fixture('gemini-proof-voice-then-typed.jsonl')
  evs.forEach((e) => b.ingest(e))
  assert.equal(notes().length, 0)
  assert.equal(opened().length, 1, 'only the provider-opened response; the typed command\'s response is not a spoken turn')
  assert.equal(opened()[0].heard, 'What is the capital of France?')
  assert.equal(opened()[0].decisionId, '1a15d14d1b4-225-d167')
  const final = (await collect(b.runVoice(opened()[0].responseId))).at(-1)
  assert.equal(final.text, 'The capital of France is Paris.')
  assert.equal(b.voiceCommit(final.voice).text, 'What is the capital of France?')
  assert.equal(b.voiceStats.unattributed, 1, 'the typed reply, not ours here, is left out')
  assert.equal(host.calls.length, 0)
})

// The same recorded arrivals. This old-shape journal has no utterance ids and no final flags, so
// after the first reply took words the ledger cannot tell a late part from the next utterance: the
// later questions are shown on their own (notes), never a reply's bubble. Every word is preserved.
test('recorded journal (bench voice run, old shape): 4 replies with a barge-in; the first takes its words, the later questions are notes', async () => {
  const { b, host, opened, notes } = setup()
  const evs = fixture('bench-voice-4-turns-barge-in.jsonl')
  evs.forEach((e) => b.ingest(e))
  assert.equal(opened().length, 4)
  assert.deepEqual(opened().map((o) => o.heard), ['What is the capital of France?', null, null, null])
  assert.deepEqual(notes().map((n) => n.text), ['Name one planet in our solar system.', 'Please count slowly from 1 to 30.', 'Stop. What is 2 + 2?'])
  const finals = []
  for (const o of opened()) finals.push((await collect(b.runVoice(o.responseId))).at(-1))
  assert.deepEqual(finals.map((f) => f.status), ['completed', 'completed', 'cancelled', 'completed'])
  assert.match(finalText(finals[2]), /^1, 2, 3, 4, 5,.* 24, \*— interrupted\*$/s)
  assert.equal(finals[3].text, 'Two plus two equals four.')
  assert.deepEqual(finals.map((f) => b.voiceCommit(f.voice)?.text ?? null), opened().map((o) => o.heard))
  assert.equal(host.calls.length, 0)
})

// ── what was shown keeps up with what is heard (injected clock) ──

function clocked(timing) {
  let t = 0
  const notes = []
  const l = new VoiceLedger({ now: () => t, timing, onNote: (n) => notes.push(n) })
  l.adjacency = false // the id contract
  let id = 0
  const part = (u, seq, text, final) => l.transcript({ id: `g:${++id}`, kind: 'input_transcript', text, utterance_id: u, utterance_seq: seq, ...(final ? { final: true } : {}), association: 'unknown' })
  return { l, notes, part, at: (ms) => { t = ms; l.sweep() } }
}

test('a partial note already shown, then the rest and the end flag: an update note, nothing lost', () => {
  const { l, notes, part, at } = clocked({ holdMs: 10, retainMs: 100 })
  part('u1', 0, 'Name one ', false)
  at(11)
  assert.deepEqual(notes.map((n) => [n.text, n.final, !!n.update]), [['Name one ', false, false]])
  at(12)
  part('u1', 1, 'planet', true)
  at(200)
  assert.deepEqual(notes.map((n) => [n.text, n.final, !!n.update, n.association]), [['Name one ', false, false, 'unknown'], ['Name one planet', true, true, 'unknown']])
  assert.ok(notes.every((n) => !n.responseId), 'no invented reply link')
  assert.equal(l.stats.updates_dropped, 0)
  assert.equal(l.utterances.size, 0, 'forgotten after retention')
  l.stop()
})

test('the final part before the first one: it waits for the gap, and the end flag alone is not "complete"', () => {
  const { notes, part, at, l } = clocked({ holdMs: 10, retainMs: 100 })
  part('u2', 1, 'planet', true)
  assert.equal(notes.length, 0, 'a gapped transcript is not shown at once, even with final')
  at(3)
  part('u2', 0, 'Name one ', false)
  assert.deepEqual(notes.map((n) => [n.text, n.final, n.missing]), [['Name one planet', true, 0]])
  // the first part never arrives: shown with the gap marked, never final
  part('u3', 2, 'today?', true)
  at(20)
  assert.deepEqual(notes.at(-1), { key: 'u-u3', text: ' [missing part] today?', final: false, missing: 2, utteranceId: 'u3', truncated: false, association: 'unknown', update: false })
  assert.equal(heardNoteText(notes.at(-1)), '[missing part] today?')
  // a gap filled after it was shown: an update note
  part('u3', 0, 'Is it ', false)
  part('u3', 1, 'raining ', false)
  assert.equal(notes.at(-1).text, 'Is it raining today?')
  assert.equal(notes.at(-1).update, true)
  assert.equal(notes.at(-1).final, true)
  l.stop()
})

test('a part arriving after its utterance was forgotten is a continuation, not missing parts', () => {
  const { notes, part, at, l } = clocked({ holdMs: 10, retainMs: 50 })
  part('u4', 0, 'Hello', true)
  at(100)
  part('u4', 1, ' again', true)
  assert.deepEqual(notes.map((n) => [n.text, n.missing, !!n.update]), [['Hello', 0, false], [' again', 0, true]])
  l.stop()
})

test('update notes are bounded per utterance; the rest is counted', () => {
  const { notes, part, at, l } = clocked({ holdMs: 1, retainMs: 1000 })
  for (let i = 0; i < 20; i++) {
    part('u5', i, `w${i} `, false)
    at(i * 10 + 5)
  }
  assert.ok(notes.length <= 9, `${notes.length} notes`)
  at(5000)
  assert.ok(l.stats.updates_dropped >= 1)
  l.stop()
})

// ── the recorded reply replaces ours only when its text is complete ──

async function gappedWith(record) {
  const host = fakeHost({ replies: record ? { 'b1-v50': record } : {} })
  const { b } = setup({ host })
  const g = guest()
  const turn = spokenTurn(g, { n: 50, reply: ['One, ', 'two, ', 'three.'] })
  const [d, rs, d1, , d3, done] = turn.evs
  ;[d, rs, d1].forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached') // "two, " was missed
  b.ingest(d3)
  b.ingest(done)
  return { final: (await run).at(-1), host }
}

test('reconcile: a complete recorded reply (text_complete) replaces the gapped text', async () => {
  const { final, host } = await gappedWith({ response_id: 'b1-v50', status: 'completed', text: 'One, two, three.', text_complete: true, truncated: false, source: 'observed', observed: 'complete' })
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'One, two, three.')
  assert.equal(final.recorded, true)
  assert.deepEqual(host.calls.map((c) => c.op), ['reply'])
})

test('reconcile: a suffix-only record keeps our text and the incomplete marker', async () => {
  const { final, host } = await gappedWith({ response_id: 'b1-v50', status: 'completed', text: 'three.', text_complete: false, truncated: false, source: 'observed', observed: 'suffix' })
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'One, three.', 'never replaced by the suffix')
  assert.equal(finalText(final), 'One, three. *— may be incomplete*')
  assert.match(final.detail, /owner's record \(completed, observed suffix from observed\) does not hold the whole text/, 'the reason is kept on the record')
  assert.equal(submits(host).length, 0)
})

test('reconcile: a terminal-only record (empty text) keeps our text and the incomplete marker', async () => {
  const { final } = await gappedWith({ response_id: 'b1-v50', status: 'completed', text: null, text_complete: false, truncated: false, source: 'observed', observed: 'terminal_only' })
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'One, three.', 'never replaced by a null or empty text')
  assert.match(final.detail, /observed terminal_only/)
})

test('reconcile: a record without text_complete counts as incomplete', async () => {
  const { final } = await gappedWith({ response_id: 'b1-v50', status: 'completed', text: 'One, two, three.' })
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'One, three.')
})

// ── the owner's reply cache can be a moment behind the observer ──

test('reconcile: with the live end seen, a 404 is asked again briefly and the cached reply is taken', async () => {
  const calls = []
  let answers = 0
  const host = {
    calls,
    async submit() { calls.push({ op: 'submit' }); return { status: 500, body: {} } },
    async cancel() { calls.push({ op: 'cancel' }); return { status: 200, body: {} } },
    async reply(rid) {
      calls.push({ op: 'reply', rid })
      // not cached on the first two asks, then whole
      return ++answers < 3 ? { status: 404, body: { error: 'unknown or evicted' } } : { status: 200, body: { reply: { response_id: rid, status: 'completed', text: 'One, two, three.', text_complete: true, source: 'observed', observed: 'complete' } } }
    },
  }
  const { b } = setup({ host, timing: { ...fast, notFoundRetryMs: [10, 20, 30] } })
  const g = guest()
  const turn = spokenTurn(g, { n: 70, reply: ['One, ', 'two, ', 'three.'] })
  const [d, rs, d1, , d3, done] = turn.evs
  ;[d, rs, d1].forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached')
  b.ingest(d3)
  b.ingest(done) // the live end is seen
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'One, two, three.')
  assert.deepEqual(calls.map((c) => c.op), ['reply', 'reply', 'reply'])
  assert.equal(b.voiceStats.not_found_retries, 2)
})

test('reconcile: with the live end seen, a 404 that persists ends incomplete after the bounded retries', async () => {
  const host = fakeHost({ replies: {} })
  const { b } = setup({ host, timing: { ...fast, notFoundRetryMs: [10, 20, 30] } })
  const g = guest()
  const turn = spokenTurn(g, { n: 71, reply: ['One, ', 'two.'] })
  const [d, rs, d1, , done] = turn.evs
  ;[d, rs, d1].forEach((e) => b.ingest(e))
  const t0 = Date.now()
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached')
  b.ingest(done)
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.equal(host.calls.filter((c) => c.op === 'reply').length, 4, '1 ask + 3 bounded retries')
  assert.ok(Date.now() - t0 < 1000)
  assert.equal(submits(host).length, 0)
})

test('reconcile: without a live end, a 404 stays final (no retry)', async () => {
  const host = fakeHost({ replies: {} })
  const { b } = setup({ host, timing: { ...fast, notFoundRetryMs: [10, 20, 30] } })
  const g = guest()
  const turn = spokenTurn(g, { n: 72, reply: ['Counting: ', 'one'] })
  turn.evs.slice(0, 3).forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached') // the end is never seen: the bounded gap wait, then one ask
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.match(final.detail, /end of this spoken reply was not seen/)
  assert.equal(host.calls.filter((c) => c.op === 'reply').length, 1)
})

test('reconcile: unload during the 404 retries ends the reply at once, never completed', async () => {
  const host = fakeHost({ replies: {} })
  const { b } = setup({ host, timing: { ...fast, notFoundRetryMs: [5000, 5000, 5000] } })
  const g = guest()
  const turn = spokenTurn(g, { n: 73, reply: ['Hello.'] })
  const [d, rs, , done] = turn.evs
  ;[d, rs].forEach((e) => b.ingest(e))
  const run = collect(b.runVoice(turn.rid))
  b.onLink('waiting')
  b.onLink('attached')
  b.ingest(done)
  await new Promise((r) => setTimeout(r, 30))
  const t0 = Date.now()
  b.stop()
  const final = (await run).at(-1)
  assert.equal(final.status, 'unknown')
  assert.ok(Date.now() - t0 < 200, `took ${Date.now() - t0} ms`)
})

test('a spoken turn followed while the observer is still away waits for the end; reconciled after the reattach', async () => {
  const host = fakeHost({ replies: { 'b1-v80': { response_id: 'b1-v80', status: 'completed', text: 'One, two, three.', text_complete: true, source: 'observed', observed: 'complete' } } })
  const { b } = setup({ host, timing: { ...fast, voiceGapWaitMs: 2000, notFoundRetryMs: [10, 20, 30] } })
  const g = guest()
  const turn = spokenTurn(g, { n: 80, reply: ['One, ', 'two, ', 'three.'] })
  const [d, rs, , , , done] = turn.evs
  ;[d, rs].forEach((e) => b.ingest(e))
  b.onLink('waiting') // the observer is away when the DSH turn starts
  // Monotonic clock, 1 ms tolerance: a 60 ms timer may fire a millisecond early against the wall clock.
  const t0 = performance.now()
  const run = collect(b.runVoice(turn.rid))
  await new Promise((r) => setTimeout(r, 60))
  assert.equal(host.calls.length, 0, 'no reconciliation against a reply that may still be running')
  b.onLink('attached')
  b.ingest(done)
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'One, two, three.')
  assert.ok(performance.now() - t0 >= 59, `waited ${performance.now() - t0} ms`)
})
