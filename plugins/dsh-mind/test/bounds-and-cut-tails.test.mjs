// Bounds and cut tails: one long coalesced row and one long-lived episode stay bounded, and a
// redacted credential is never shown again when rth-events' 240-character text tail cuts away its context.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import { loadDefinitions } from '../src/definitions.mjs'
import { MindStream } from '../src/mind.mjs'

const catalog = loadCatalog()
let seq = 0
const env = (kind, f = {}) => ({ v: 2, id: `guest-review:${++seq}`, producer: 'guest-review', seq, t_ns: seq * 1e6, source: 'guest', kind, ...f })
const TOKEN = 'SYNTHETIC_REVIEW_TOKEN_1234567890'

/** The real path: a real EventWindow, its changes into a MindStream; every emitted payload kept. */
function rig({ limit = 10, windowLimit = 2000 } = {}) {
  const window = new EventWindow(catalog, { limit: windowLimit })
  const mind = new MindStream({ limit })
  const emitted = []
  mind.onChange((type, payload) => emitted.push({ type, payload }))
  const wire = (m) => window.onChange((t, p) => {
    if (t === 'added') m.agentd(p, { via: 'rth-events observer feed' })
    if (t === 'updated') m.agentdPatch(p)
  })
  wire(mind)
  return { window, mind, emitted, wire }
}
const everything = (r) => [...r.emitted.map((e) => JSON.stringify(e)), JSON.stringify(r.mind.snapshot()), ...r.mind.rows.map((x) => JSON.stringify(r.mind.detail(x.id, loadDefinitions())))].join('\n')

// ── bounds ──────────────────────────────────────────────────────────────────────────────────

test('one long coalesced reply keeps a bounded sample of its chunk ids, and says how many it omitted', () => {
  const r = rig()
  for (let i = 0; i < 2000; i++) r.window.push(env('agent_delta', { response_id: 'coalesced', text: 'x' }))
  const row = r.mind.rows[0]
  assert.equal(r.mind.rows.length, 1)
  assert.ok(row.eventIds.length <= 8, `eventIds ${row.eventIds.length}`)
  assert.ok(r.mind.byEvent.size <= 8, `byEvent ${r.mind.byEvent.size}`)
  const d = r.mind.detail(row.id, loadDefinitions())
  assert.equal(d.ids.chunk_ids.kept + d.ids.chunk_ids.omitted, 2000, 'kept plus omitted is every chunk')
  assert.equal(d.ids.chunk_ids.first, `guest-review:${seq - 1999}`)
  assert.equal(d.ids.chunk_ids.last, `guest-review:${seq}`)
  assert.equal(r.mind.byEvent.get(`guest-review:${seq}`), row.id, 'the latest chunk still resolves')
})

test('a long-lived episode keeps union-find keys bounded by its retained rows, and still joins later events', () => {
  const m = new MindStream({ limit: 10 })
  m.agentd(env('response_started', { response_id: 'long', request_id: 'mic-1' }))
  for (let i = 0; i < 2000; i++) m.agentd(env('provider_signal', { response_id: 'long', signal: 'generation_complete' }))
  const ep = [...m.episodes.values()][0]
  assert.equal(m.rows.length, 10)
  assert.equal(m.episodes.size, 1)
  assert.ok(m.uf.size <= 4 * 10, `uf ${m.uf.size}`)
  assert.ok(ep.keys.size <= 4 * 10, `episode keys ${ep.keys.size}`)
  assert.ok(m.keyEpisode.size <= 4 * 10, `keyEpisode ${m.keyEpisode.size}`)
  // The shared key survives: the reply's completion still joins the same episode.
  const done = m.agentd(env('agent_done', { response_id: 'long', status: 'completed' }))
  assert.equal(done.episode, ep.id)
  assert.equal(m.snapshot().episodes.find((e) => e.id === ep.id).status, 'completed')
})

// ── cut tails ───────────────────────────────────────────────────────────────────────────────

test('prefix cut: a credential redacted earlier is never shown when the tail cuts away its "Bearer"', () => {
  const r = rig()
  r.window.push(env('agent_delta', { response_id: 'redact', text: `Bearer ${TOKEN}` }))
  assert.match(r.mind.rows[0].sentence, /Bearer \[redacted\]/)
  r.window.push(env('agent_delta', { response_id: 'redact', text: ' '.repeat(240 - TOKEN.length) }))
  assert.ok(!everything(r).includes(TOKEN), 'the whole credential reappeared after the tail shift')
})

test('a cut inside a longer context ("Authorization: Bea|rer TOKEN") does not reveal it either', () => {
  const r = rig()
  r.window.push(env('agent_delta', { response_id: 'ctx', text: `Authorization: Bearer ${TOKEN} ok` }))
  // Cut so the tail starts inside "Bearer": the "Bea" goes, "rer TOKEN ok" stays in the tail.
  const keep = `rer ${TOKEN} ok`.length
  r.window.push(env('agent_delta', { response_id: 'ctx', text: '.'.repeat(240 - keep) }))
  assert.ok(!everything(r).includes(TOKEN))
})

test('controls: preloaded and continued tails that start after a cut are hidden at the cut; uncut text is untouched', () => {
  // Preload: the window already holds a cut tail starting at the token when Mind loads.
  const w = new EventWindow(catalog, { limit: 100 })
  w.push(env('agent_delta', { response_id: 'pre', text: `Bearer ${TOKEN}` }))
  w.push(env('agent_delta', { response_id: 'pre', text: ' '.repeat(240 - TOKEN.length) }))
  const pre = new MindStream({ limit: 10 })
  for (const row of w.rows) pre.agentd(row, { via: 'rth-events observer feed', history: true, preload: true })
  assert.ok(!JSON.stringify(pre.snapshot()).includes(TOKEN), 'a preloaded cut tail starts with the token')
  // Continuation: the row was evicted, and the next patch's tail starts with the token.
  const r = rig({ limit: 2 })
  r.window.push(env('agent_delta', { response_id: 'cont', text: `Bearer ${TOKEN}` }))
  r.window.push(env('log', { level: 'info', msg: 'a' }))
  r.window.push(env('log', { level: 'info', msg: 'b' }))
  r.window.push(env('agent_delta', { response_id: 'cont', text: ' '.repeat(240 - TOKEN.length) }))
  assert.ok(r.mind.rows.some((x) => x.continuation), 'the late patch became a continuation row')
  assert.ok(!everything(r).includes(TOKEN), 'a continuation tail starts with the token')
  // Control: ordinary text that was never cut is shown as it is.
  const c = rig()
  c.window.push(env('agent_delta', { response_id: 'plain', text: 'Oi! Tudo bem ' }))
  c.window.push(env('agent_delta', { response_id: 'plain', text: 'por aqui.' }))
  assert.match(c.mind.rows[0].sentence, /“Oi! Tudo bem por aqui\.” \(2 parts\)/)
})

test('a credential never seen whole, cut out of a multi-word context inside one long chunk, is hidden as credential-shaped', () => {
  const r = rig()
  const token = 'SYNTHETIC_RESIDUAL_TOKEN_123456'
  r.window.push(env('agent_delta', { response_id: 'long', text: 'hi ' }))
  const tail = `rer ${token} ` + '.'.repeat(240 - `rer ${token} `.length)
  r.window.push(env('agent_delta', { response_id: 'long', text: 'p'.repeat(300) + 'Authorization: Bea' + tail }))
  assert.ok(!everything(r).includes(token), 'a credential-shaped run right after a cut is shown')
  // Control: ordinary long words after a cut stay readable.
  const c = rig()
  c.window.push(env('agent_delta', { response_id: 'words', text: 'start ' }))
  c.window.push(env('agent_delta', { response_id: 'words', text: 'x'.repeat(300) + ' internationalization is a long ordinary word and stays' }))
  assert.match(c.mind.rows[0].sentence, /internationalization is a long ordinary word and stays/)
})
