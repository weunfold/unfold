// Eviction, identity across restarts, redaction of coalesced text on live SSE, and the audit-read
// selection race. Each scenario is a synthetic reproduction, made deterministic.
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import * as plugin from '../index.mjs'
import { loadDefinitions } from '../src/definitions.mjs'
import * as display from '../src/display.mjs'
import { MindStream } from '../src/mind.mjs'
import { SuggestionLog } from '../src/suggestions.mjs'

const catalog = loadCatalog()
let seq = 0
const env = (kind, f = {}, producer = 'guest-test') => ({ v: 2, id: `${producer}:${++seq}`, producer, seq, t_ns: seq * 1e6, source: 'guest', kind, ...f })
const settle = () => new Promise((r) => setImmediate(r))
const SECRET = 'SYNTHETIC_TEST_ONLY_0123456789'

// ── eviction ────────────────────────────────────────────────────────────────────────────────

test('a late completion for an evicted reply does not throw; it starts a truthful partial episode', () => {
  const m = new MindStream({ limit: 3 })
  const removed = []
  m.onChange((type, p) => type === 'episode-removed' && removed.push(p.id))
  const start = m.agentd(env('response_started', { response_id: 'r-old', request_id: 'mic-1' }))
  for (let i = 0; i < 4; i++) m.agentd(env('log', { level: 'info', msg: 'filler' }))
  assert.deepEqual(removed, [start.episode], 'the client is told the emptied episode is gone')
  const late = m.agentd(env('agent_done', { response_id: 'r-old', status: 'completed' }))
  assert.ok(late.episode, 'the late completion has an episode')
  assert.notEqual(late.episode, start.episode)
  const ep = m.snapshot().episodes.find((e) => e.id === late.episode)
  assert.equal(ep.status, 'completed')
  assert.match(ep.partial, /earlier rows of this episode were evicted/)
})

test('a late coalesced chunk for an evicted reply is kept as a continuation, not lost', () => {
  const window = new EventWindow(catalog, { limit: 100 })
  const m = new MindStream({ limit: 3 })
  window.onChange((type, p) => {
    if (type === 'added') m.agentd(p, { via: 'rth-events observer feed' })
    if (type === 'updated') m.agentdPatch(p)
  })
  window.push(env('agent_delta', { response_id: 'r1', text: 'first part, ' }))
  for (let i = 0; i < 4; i++) window.push(env('log', { level: 'info', msg: 'filler' }))
  assert.equal(m.rows.some((r) => r.kind === 'agent_delta'), false, 'the delta row was evicted')
  window.push(env('agent_delta', { response_id: 'r1', text: 'late part' }))
  const cont = m.rows.find((r) => r.kind === 'agent_delta')
  assert.ok(cont, 'the late chunk is shown')
  assert.match(cont.sentence, /late part/)
  assert.match(cont.sentence, /continued: the start of this text was evicted/)
  window.push(env('agent_delta', { response_id: 'r1', text: ' and more' }))
  assert.equal(m.rows.filter((r) => r.kind === 'agent_delta').length, 1, 'later chunks grow the continuation row')
  // The journal lane coalesces by itself: a late chunk after its row was evicted is also a continuation.
  const j = new MindStream({ limit: 2 })
  const o = { via: 'agentd journal file', file: '/j', offset: 0 }
  j.agentd(env('agent_delta', { response_id: 'rj', text: 'a' }), o)
  j.agentd(env('log', { level: 'info', msg: 'x' }), o)
  j.agentd(env('log', { level: 'info', msg: 'y' }), o)
  const jc = j.agentd(env('agent_delta', { response_id: 'rj', text: 'b' }), o)
  assert.match(jc.sentence, /continued/)
})

test('every server index stays bounded with the row window over many episodes', () => {
  const limit = 10
  const c = new MindStream({ limit })
  let removed = 0
  c.onChange((type) => type === 'episode-removed' && removed++)
  for (let i = 0; i < 2000; i++) {
    const cmd = `cmd-${i}`
    c.agentd(env('command_admitted', { command_id: cmd, generation: i, status: 'admitted' }))
    const d = c.agentd(env('decision', { decision_id: `d-${i}`, trigger_event: `host:${cmd}`, candidates: ['respond'], selected: 'respond', policy: 'rule', latency_ns: 1, episode_id: 'x', training_eligibility: 'unknown' }))
    c.agentd(env('response_started', { cause: d.ids.event, response_id: `r-${i}`, request_id: cmd }))
    c.agentd(env('response_started', { response_id: `orphan-${i}`, request_id: `q-${i}` })) // a request id never typed
    c.agentd(env('agent_done', { cause: `missing:${i}`, response_id: `bounded-${i}`, status: 'completed' })) // an unseen cause
  }
  const sizes = {
    rows: c.rows.length,
    episodes: c.episodes.size,
    uf: c.uf.size,
    keyEpisode: c.keyEpisode.size,
    byEvent: c.byEvent.size,
    children: c.children.size,
    pendingCause: c.pendingCause.size,
    requestRefs: c.requestRefs.size,
    typed: c.typed.size,
    open: c.open.size,
    tombstones: c.tombstones.size,
  }
  assert.equal(sizes.rows, limit)
  for (const [k, v] of Object.entries(sizes)) assert.ok(v <= 8 * limit, `${k} = ${v} grows past 8 x limit`)
  assert.equal(removed + c.episodes.size, c.episodeSeq, 'every episode is either retained or announced as removed')
})

// ── identity across restarts ────────────────────────────────────────────────────────────────

test('a new Mind lifetime never reuses an episode or row id; old suggestions and audit ids do not attach to new events', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-r2-'))
  try {
    const a = new MindStream()
    const b = new MindStream()
    assert.notEqual(a.life, b.life)
    const ra = a.agentd(env('response_started', { response_id: 'r-A', request_id: 'mic-1' }))
    new SuggestionLog(dir).append({ key: 'reviewer-identity-A', scope: { episode: ra.episode, title: 'reply A' }, text: 'Synthetic suggestion about A' })
    const rb = b.agentd(env('response_started', { response_id: 'r-B', request_id: 'mic-2' }))
    assert.notEqual(ra.episode, rb.episode)
    assert.notEqual(ra.id, rb.id)
    assert.deepEqual(new SuggestionLog(dir).list({ episode: rb.episode }), [], 'no old suggestion under the new episode')
    assert.equal(b.detail(ra.id, loadDefinitions()), null, 'an old audit reference does not resolve to a new, unrelated event')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

async function loaded(dir, config = {}) {
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const window = new EventWindow(catalog, { limit: 200 })
  ctx.provide('rthEvents', { window, catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: dir, ...config })
  await fiber
  await settle()
  const route = (path) => routes.find((r) => r.path === path)
  const call = async (path, init) => {
    const res = await route(path.split('?')[0]).fetch(new Request(`http://h${path}`, init))
    return { status: res.status, body: await res.json() }
  }
  return { ctx, fiber, window, route, call }
}

test('a saved proposal follows its episode through a same-lifetime merge, and records its provenance', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-r2m-'))
  try {
    const p = await loaded(dir)
    p.window.push(env('decision', { decision_id: 'dm', trigger_event: 'x', candidates: ['respond'], selected: 'respond', policy: 'rule', latency_ns: 1, episode_id: 'e', training_eligibility: 'unknown' }))
    p.window.push(env('response_started', { response_id: 'rm', request_id: 'mic-9' }))
    const before = (await p.call('/api/rth-mind')).body.episodes
    assert.equal(before.length, 2)
    const target = before.find((e) => e.title.includes('reply rm'))
    const post = await p.call('/api/rth-mind/suggestions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ key: 'merge-0001', actor: 'reviewer', scope: { episode: target.id, title: target.title }, text: 'About the reply' }) })
    assert.equal(post.status, 201)
    assert.equal(post.body.record.context.mind_lifetime.length > 0, true, 'the server records which Mind lifetime the scope belongs to')
    assert.equal(post.body.record.context.episode_retained, true)
    assert.ok(post.body.record.context.event_ids.length > 0, 'and the event ids the episode held, as provenance')
    // An explicit id joins the two episodes: the older one keeps its id.
    p.window.push(env('command_admitted', { command_id: 'cm', generation: 1, status: 'completed', decision_id: 'dm', response_id: 'rm' }))
    const after = (await p.call('/api/rth-mind')).body.episodes
    assert.equal(after.length, 1)
    const merged = after[0]
    assert.ok(merged.aliases.includes(target.id) || merged.id === target.id)
    const list = (await p.call(`/api/rth-mind/suggestions?episode=${encodeURIComponent(merged.id)}`)).body.suggestions
    assert.deepEqual(list.map((s) => s.key), ['merge-0001'], 'the proposal is still listed with its actual target')
    await p.fiber.dispose()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

// ── redaction of coalesced text ─────────────────────────────────────────────────────────────

test('a credential in a later delta, or split across deltas, never reaches any emitted payload, snapshot or audit', () => {
  for (const parts of [['A synthetic credential: ', `Bearer ${SECRET}`], ['key: Bearer SYNTHETIC_TE', 'ST_ONLY_0123456789 and the rest']]) {
    const window = new EventWindow(catalog, { limit: 100 })
    const mind = new MindStream()
    const emitted = []
    mind.onChange((type, payload) => emitted.push(JSON.stringify({ type, payload })))
    window.onChange((type, p) => {
      if (type === 'added') mind.agentd(p, { via: 'rth-events observer feed' })
      if (type === 'updated') mind.agentdPatch(p)
    })
    for (const text of parts) window.push(env('agent_delta', { response_id: 'r1', text }))
    window.push(env('agent_done', { response_id: 'r1', status: 'completed' }))
    const row = mind.rows.find((r) => r.kind === 'agent_delta')
    const all = [...emitted, JSON.stringify(mind.snapshot()), ...mind.rows.map((r) => JSON.stringify(mind.detail(r.id, loadDefinitions())))].join('\n')
    assert.ok(!all.includes(SECRET), `the complete credential leaked for ${JSON.stringify(parts)}`)
    assert.match(row.sentence, /Bearer \[redacted\]/)
  }
})

test('the plugin redacts every SSE payload at the boundary', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-r3-'))
  try {
    const p = await loaded(dir)
    const ac = new AbortController()
    const res = await p.route('/api/rth-mind/stream').fetch(new Request('http://h/api/rth-mind/stream', { signal: ac.signal }))
    const reader = res.body.getReader()
    let text = ''
    p.window.push(env('agent_delta', { response_id: 'r9', text: 'credential: ' }))
    p.window.push(env('agent_delta', { response_id: 'r9', text: `Bearer ${SECRET}` }))
    for (const fields of [{ msg: `Authorization: Bearer ${SECRET}` }]) p.window.push(env('log', { level: 'info', ...fields }))
    const until = Date.now() + 2000
    while (!/agent_delta[\s\S]*row-updated[\s\S]*log/.test(text) && Date.now() < until) {
      const { value, done } = await Promise.race([reader.read(), new Promise((r) => setTimeout(() => r({ value: null }), 200))])
      if (done) break
      if (value) text += Buffer.from(value).toString('utf8')
    }
    ac.abort()
    assert.match(text, /event: row-updated/)
    assert.ok(!text.includes(SECRET), 'no SSE frame carries the credential')
    await p.fiber.dispose()
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

// ── the audit read selection race ───────────────────────────────────────────────────────────

function deferred() {
  let resolve
  const promise = new Promise((r) => (resolve = r))
  return { promise, resolve }
}

test('a late audit read for a previous selection never replaces the current one; a read crossing a restart is refused', async () => {
  assert.equal(typeof display.auditLoader, 'function', 'the audit read is bound to its selection by src/display.mjs auditLoader')
  const pending = new Map()
  const aborted = []
  const loader = display.auditLoader((id, signal) => {
    const d = deferred()
    pending.set(id, d)
    signal.addEventListener('abort', () => aborted.push(id))
    return d.promise
  })
  const states = []
  const on = (s) => states.push(s)
  loader.select('L1.mA', on)
  loader.select('L1.mB', on)
  assert.deepEqual(aborted, ['L1.mA'], 'the superseded read is aborted')
  pending.get('L1.mB').resolve({ ok: true, body: { row: { id: 'L1.mB', sentence: 'B' }, evidence: { row: { id: 'L1.mB' } } } })
  await settle()
  pending.get('L1.mA').resolve({ ok: true, body: { row: { id: 'L1.mA', sentence: 'A' }, evidence: { row: { id: 'L1.mA' } } } })
  await settle()
  const last = states.at(-1)
  assert.equal(last.state, 'ready')
  assert.equal(last.id, 'L1.mB')
  assert.equal(last.data.row.sentence, 'B', 'the late A never overwrites B')
  assert.ok(!states.some((s) => s.state === 'ready' && s.id === 'L1.mA'))
  // An answer for a different row (e.g. the server restarted and the id now means something else) is refused.
  loader.select('L1.mC', on)
  pending.get('L1.mC').resolve({ ok: true, body: { row: { id: 'L2.mC', sentence: 'unrelated' } } })
  await settle()
  assert.equal(states.at(-1).state, 'error')
  assert.match(states.at(-1).error, /not L1\.mC/)
})
