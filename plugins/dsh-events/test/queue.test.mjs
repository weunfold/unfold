// The presence tab's state word and RTS queue (src/queue.mjs), driven through the real presence
// reducer (src/presence.mjs) with catalog events: slots open and settle by their own ids, never by
// time or a reconnect; no fake progress; a cancel only where a real one exists.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.mjs'
import { LEASE, initialPresence, presence, view } from '../src/presence.mjs'
import { MAX_SLOTS, SETTLE_MS, WORDS, langOf, openItems, slotLine, stateOf, stepQueue, unresolvedIn } from '../src/queue.mjs'

const catalog = loadCatalog()
let seq = 0
const env = (kind, fields = {}) => ({ v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq, source: 'guest', kind, ...fields })
const ev = (kind, fields) => ({ type: 'event', d: catalog.describe(env(kind, fields)) })
const awake = { read: true, actual: 'active' }
const asleep = { read: true, actual: 'off' }

function rig() {
  let now = 0
  let s = initialPresence(0)
  s = presence(s, { type: 'link', state: 'attached', detail: 'observer' }, 0)
  s = presence(s, { type: 'host', status: { host: { mic: { state: 'unmuted' } }, commands: { unfinished: 0 } } }, 10)
  let q = { slots: [] }
  const beat = () => { s = presence(s, ev('heartbeat', { uptime_ms: now, attachments: 1 }), now) }
  const step = (input, dt = 100) => {
    now += dt
    if (input) s = presence(s, input, now)
    beat()
    const v = view(s, now)
    q = stepQueue(q, v, now)
    return { v, q }
  }
  return { step, get q() { return q }, get now() { return now }, view: () => view(s, now), tick: (dt) => { now += dt; beat(); q = stepQueue(q, view(s, now), now); return q }, link: (st) => { now += 50; s = presence(s, { type: 'link', state: st, detail: 'observer' }, now); q = stepQueue(q, view(s, now), now) } }
}

test('the word and the slots follow the reducer: a request, then its reply, then both end by their own ids', () => {
  const r = rig()
  let { v, q } = r.step(null)
  assert.equal(stateOf(v, awake, q), 'quiet')
  assert.equal(stateOf(v, asleep, q), 'asleep')
  assert.equal(q.slots.length, 0, 'nothing running: no slots, no placeholders')
  ;({ v, q } = r.step(ev('command_admitted', { command_id: 'dsh-1', generation: 1, status: 'admitted' })))
  assert.deepEqual(q.slots.map((x) => [x.kind, x.phase]), [['request', 'running']])
  assert.equal(stateOf(v, awake, q), 'working')
  assert.match(slotLine(q.slots[0], r.now), /^A request is open · \d+ s$/)
  ;({ v, q } = r.step(ev('response_started', { response_id: 'r1', request_id: 'dsh-1' })))
  assert.equal(stateOf(v, awake, q), 'replying')
  assert.ok(q.slots.some((x) => x.kind === 'reply' && x.phase === 'running'))
  const since = q.slots.find((x) => x.kind === 'reply').since
  ;({ v, q } = r.step(null, 5000))
  assert.equal(q.slots.find((x) => x.kind === 'reply').since, since, 'the slot keeps the time this tab first saw it')
  assert.match(slotLine(q.slots.find((x) => x.kind === 'reply'), r.now), /^Replying · 5 s$/)
  ;({ v, q } = r.step(ev('agent_done', { response_id: 'r1', status: 'completed' })))
  assert.ok(q.slots.every((x) => x.phase === 'ended'), 'both ended: the reducer closed them by id')
  assert.equal(stateOf(v, awake, q), 'quiet')
  assert.equal(r.tick(SETTLE_MS + 10).slots.length, 0, 'they fade after the settle')
})

test('no fake progress: slots carry no percentage, the line only the elapsed time', () => {
  const r = rig()
  const { q } = r.step(ev('response_started', { response_id: 'r9', request_id: null }))
  for (const s of q.slots) {
    assert.ok(!('progress' in s) && !('percent' in s) && !('pct' in s))
    assert.doesNotMatch(slotLine(s, r.now + 3000), /%|\d+\s*\/\s*\d+/)
  }
})

test('a reply open when the link drops: unsure while out of touch, then a kept "?" slot, never ended or quiet', () => {
  const r = rig()
  r.step(ev('response_started', { response_id: 'ra', request_id: null }))
  r.link('waiting')
  assert.ok(r.q.slots[0].unsure, 'out of touch: the running slot is unsure, not settled')
  assert.equal(r.q.slots[0].phase, 'running')
  r.link('attached')
  const { v, q } = r.step({ type: 'host', status: { host: { mic: { state: 'unmuted' } }, commands: { unfinished: 0 } } })
  assert.equal(openItems(v).length, 0, 'the reducer forgot it on reattach')
  assert.deepEqual(q.slots.map((x) => x.phase), ['lost'], 'kept as unknown, not ended')
  assert.match(slotLine(q.slots[0], r.now), /not sure how it ended/)
  assert.equal(stateOf(v, awake, q), 'unknown', 'not "quiet" while an outcome is unknown')
  assert.ok(unresolvedIn(q))
  assert.equal(r.tick(10 * 60_000).slots[0]?.phase, 'lost', 'time alone does not settle it')
})

test('an end for another id (or with no observed start) does not close a different open reply', () => {
  const r = rig()
  r.step(ev('response_started', { response_id: 'a', request_id: null }))
  r.step(ev('response_started', { response_id: 'b', request_id: null }))
  const { q } = r.step(ev('agent_done', { response_id: 'c', status: 'completed' }))
  assert.deepEqual(q.slots.filter((x) => x.phase === 'running').map((x) => x.ref).sort(), ['a', 'b'])
  const q2 = r.step(ev('agent_done', { response_id: 'b', status: 'completed' })).q
  assert.deepEqual(q2.slots.filter((x) => x.phase === 'running').map((x) => x.ref), ['a'])
})


test('words: page language, no thinking, and "listening" is never claimed (no speech-candidate source)', () => {
  assert.equal(langOf('pt-BR'), 'pt')
  assert.equal(langOf('en'), 'en')
  assert.equal(WORDS.pt.writing, 'Escrevendo')
  for (const w of [...Object.values(WORDS.en), ...Object.values(WORDS.pt)]) assert.doesNotMatch(w, /think|pensa/i)
  const r = rig()
  const states = new Set()
  for (const input of [null, ev('command_admitted', { command_id: 'x', generation: 1, status: 'admitted' }), ev('input_transcript', { text: 'oi', final: true }), ev('response_started', { response_id: 'y', request_id: 'x' })]) {
    const { v, q } = r.step(input)
    states.add(stateOf(v, awake, q))
  }
  assert.ok(!states.has('listening'))
  assert.ok(!states.has('writing'), 'an open response is "replying": response_started carries no text evidence')
  assert.ok(states.has('replying'))
  assert.equal(MAX_SLOTS, 5)
  assert.equal(LEASE.heartbeatMs > 0, true)
})

// ---- No per-reply cancel exists; a host aggregate is not an identified item ----

test('no slot offers a cancel (the only stop route is global: api/rth-session/stop -> /v1/cancel {})', async () => {
  const mod = await import('../src/queue.mjs')
  assert.equal(mod.cancelFor, undefined, 'no per-slot cancel helper')
  const { readFileSync } = await import('node:fs')
  const src = readFileSync(new URL('../client/index.jsx', import.meta.url), 'utf8')
  assert.doesNotMatch(src, /Stop this reply|cancelFor/, 'no per-slot stop in the presence client')
})

test('a host aggregate (unfinished=2, unidentified) never shows as a slot that ends when local entries appear', () => {
  let now = 0
  let s = initialPresence(0)
  s = presence(s, { type: 'link', state: 'attached', detail: 'observer' }, 0)
  s = presence(s, { type: 'host', status: { host: { mic: { state: 'unmuted' } }, commands: { unfinished: 2 } } }, 10)
  const beat = () => { now += 100; s = presence(s, ev('heartbeat', { uptime_ms: now, attachments: 1 }), now) }
  beat()
  let q = stepQueue({ slots: [] }, view(s, now), now)
  assert.equal(stateOf(view(s, now), awake, q), 'working', 'the aggregate still says work is open')
  assert.ok(!q.slots.some((x) => x.id === 'h:host-record'), 'no slot for an unidentified aggregate')
  now += 100
  s = presence(s, ev('response_started', { response_id: 'rn', request_id: null }), now)
  beat()
  q = stepQueue(q, view(s, now), now)
  assert.ok(!q.slots.some((x) => x.phase === 'ended'), 'nothing "ended": the aggregate leaving the label is not terminal evidence')
  assert.deepEqual(q.slots.map((x) => [x.kind, x.phase]), [['reply', 'running']])
  assert.equal(stateOf(view(s, now), awake, q), 'replying')
})

test('associated start: command_admitted(c1) then response_started(r1, request_id=c1) -> lines are neutral, never "no reply yet"', () => {
  const r = rig()
  r.step(ev('command_admitted', { command_id: 'c1', generation: 1, status: 'admitted' }))
  const { q } = r.step(ev('response_started', { response_id: 'r1', request_id: 'c1' }))
  const lines = q.slots.map((x) => slotLine(x, r.now))
  assert.deepEqual(lines.map((l) => l.replace(/ · \d+ s$/, '')), ['A request is open', 'Replying'])
  assert.ok(!lines.some((l) => /no reply yet/.test(l)))
})
