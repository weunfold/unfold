// A standalone sk- credential, with no
// key, assignment or scheme around it, must never reach a Mind row, snapshot, audit record or SSE
// payload, in host text or agent text, whole, nested, split across deltas or cut by a tail.
// Synthetic values only: CANARY is the receipt's canary; the others are made up for the variants.
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { loadCatalog } from './support/catalog.js'
import { EventWindow, redactText as windowRedactText } from './support/window.js'
import * as plugin from '../index.js'
import { loadDefinitions } from '../src/definitions.js'
import { MindStream } from '../src/mind.js'
import { REDACTED, redact, redactText, secretsIn } from '../src/redact.js'
import { sentence } from '../src/sentences.js'

const catalog = loadCatalog()
const BOOT = 'b0'
const P = `guest-${BOOT}`
const settle = () => new Promise((r) => setImmediate(r))
// A bounded wait on a ref'd timer: a stream that never ends fails here with one line, rather than
// leaving the run with nothing to wait on.
const within = (promise, ms, what) => {
  let timer
  const limit = new Promise((_, reject) => (timer = setTimeout(() => reject(new Error(`${what}: not in ${ms} ms`)), ms)))
  return Promise.race([promise, limit]).finally(() => clearTimeout(timer))
}
const CANARY = 'sk-proofCANARY0123456789abcdefABCDEF'
// Other members of the family (synthetic): legacy, project, service-account, admin, Anthropic-style.
const VARIANTS = [
  'sk-Synthetic0Legacy1Key2Value3Here4Abcdef5Ghijkl6',
  'sk-proj-Synth3tic_Proj-Key4Value5abcDEF6ghiJKL7mno',
  'sk-svcacct-Synth3ticSvcKey4Value5abcDEF6ghi',
  'sk-admin-Synth3ticAdminKey4Value5abcDEF6',
  'sk-ant-api03-Synth3tic_AntKey-4Value5abcDEF6ghiJKL7mnoPQR8AA',
  'sk-ant-admin01-Synth3ticAdminKey4Value5abcDEF6',
  'sk-ant-oat01-Synth3ticOatKey4Value5abcDEF6ghi',
]
const fragmentsOf = (s, n = 5) => Array.from({ length: s.length - n + 1 }, (_, i) => s.slice(i, i + n))
// Mind's random lifetime nonce and the evidence sha256 values are hex, and the canary has a hex
// stretch: tests fix the nonce (`fixed`) and blank 64-hex digests, so a match is a real leak.
const DIGEST = /\b[0-9a-f]{64}\b/g
const fragmentIn = (s, text) => fragmentsOf(s).find((f) => text.replace(DIGEST, '<sha256>').includes(f)) ?? null
const fixed = (mind) => ((mind.life = 'lifeZ'), mind)
// A failure's own evidence, without the secret: every run of characters covered by the whole
// canary or a 5-char fragment of it reads "[canary]", so a report never carries its bytes. The
// text is redacted first, so an excerpt cannot print another secret's bytes either.
const maskCanary = (raw) => {
  const s = redactText(raw).replace(DIGEST, '<sha256>')
  const hidden = Array.from({ length: s.length }, () => false)
  for (const f of [CANARY, ...fragmentsOf(CANARY)]) {
    for (let at = s.indexOf(f); at >= 0; at = s.indexOf(f, at + 1)) hidden.fill(true, at, at + f.length)
  }
  return [...s].map((c, i) => (!hidden[i] ? c : hidden[i - 1] ? '' : '[canary]')).join('')
}
// Where a served body carries `frag`: the JSON path of the first string or number holding it
// (a snapshot body is one JSON value, SSE frames one per `data:` line), its type, and a masked
// excerpt around it. Never throws: a diagnostic must not replace the failure it explains.
const whereIn = (text, frag) => {
  try {
    const hit = (v, path) => {
      if (typeof v === 'string' || typeof v === 'number') {
        const s = String(v).replace(DIGEST, '<sha256>')
        const at = s.indexOf(frag)
        return at < 0 ? null : `${path} (${typeof v}): ${maskCanary(s.slice(Math.max(0, at - 40), at + frag.length + 40))}`
      }
      if (v && typeof v === 'object') {
        for (const [k, x] of Object.entries(v)) {
          const found = hit(x, Array.isArray(v) ? `${path}[${k}]` : `${path}.${k}`)
          if (found) return found
        }
      }
      return null
    }
    const values = text.trimStart().startsWith('{') ? [['$', JSON.parse(text)]] : text.split('\n').filter((l) => l.startsWith('data: ')).map((l, i) => [`frame[${i}]`, JSON.parse(l.slice(6))])
    for (const [path, v] of values) {
      const found = hit(v, path)
      if (found) return found
    }
    return 'in no JSON string or number (outside the parsed values)'
  } catch (e) {
    return `not located (${e.message})`
  }
}

// ── the pattern contract ───────────────────────────────────────────────────────────────────

test('the bare canary in plain prose is redacted and its value is recognised', () => {
  assert.equal(redactText(`my key is ${CANARY} ok`), `my key is ${REDACTED} ok`)
  assert.deepEqual(secretsIn(`my key is ${CANARY} ok`), [CANARY])
  for (const v of VARIANTS) {
    assert.equal(redactText(`use ${v}.`), `use ${REDACTED}.`, v)
    assert.deepEqual(secretsIn(`use ${v}.`), [v], v)
  }
})

test('boundaries — quotes, JSON escapes, punctuation and line starts are starts; mid-word is not', () => {
  for (const [input, want] of [
    [`"${CANARY}"`, `"${REDACTED}"`],
    [`'${CANARY}'`, `'${REDACTED}'`],
    [`(${CANARY})`, `(${REDACTED})`],
    [`<${CANARY}>`, `<${REDACTED}>`],
    [`${CANARY}`, REDACTED],
    [`line1\n${CANARY}`, `line1\n${REDACTED}`],
    [`{"msg":"line1\\n${CANARY}\\tend"}`, `{"msg":"line1\\n${REDACTED}\\tend"}`], // a serialized JSON escape before it
    [`url=https://x/${CANARY}/y`, `url=https://x/${REDACTED}/y`],
    [`k:${CANARY},next`, `k:${REDACTED},next`],
  ]) assert.equal(redactText(input), want, input)
  assert.deepEqual(secretsIn(`{"msg":"line1\\n${CANARY}"}`), [CANARY])
})

test('whole-value priority, escaped quotes and key-aware masking keep their behaviour', () => {
  // A credential key's value stays ONE unit (one marker, not a marker inside a marker).
  assert.equal(redactText(`api_key: "${CANARY}"`), `api_key: "${REDACTED}"`)
  assert.equal(redactText(`OPENAI_API_KEY=${CANARY}`), `OPENAI_API_KEY=${REDACTED}`)
  assert.equal(redactText(`Authorization: Bearer ${CANARY}`), `Authorization: Bearer ${REDACTED}`)
  assert.equal(redactText(`{"api_key": "pre\\"${CANARY}\\" tail", "ok": 1}`), `{"api_key": "${REDACTED}", "ok": 1}`)
  assert.deepEqual(redact({ nested: { note: `see ${CANARY}`, list: [CANARY, 'plain'] }, token: 'x' }), { nested: { note: `see ${REDACTED}`, list: [REDACTED, 'plain'] }, token: REDACTED })
  // Existing families, unchanged.
  assert.equal(redactText('Bearer abcdef0123456789secret'), `Bearer ${REDACTED}`)
  assert.equal(redactText('Basic dXNlcjpwYXNz'), `Basic ${REDACTED}`)
  assert.equal(redactText('key AIzaSyD-THIS-IS-A-FAKE-KEY-0123456789'), `key ${REDACTED}`)
  assert.equal(redactText('GEMINI_API_KEY="a b c"'), `GEMINI_API_KEY=${REDACTED}`)
})

test('controls: hashes, ids, hyphenated prose and lookalikes are kept', () => {
  for (const keep of [
    'sha256 8bc340e750b82c176990fe60d623d024f0e59da7f21d6cb8dc33de7e1472110d',
    'artifact_id 3f2a9c1e-7b4d-4e2a-9c1f-0a1b2c3d4e5f boot b0 host-1-pid42 unfold-de3aecdb-m17',
    'risk-Assessment0123456789abcdefABCDEF and desk-Booking0123456789abcdefABC', // not an sk- start
    'task-sk-Abc0123456789defghijklmno', // "-" before sk- is not a start
    'we use sk-learn and sk-learn-pipeline-v2 here', // no upper case: prose, kept
    'SK-UPPER0123456789ABCDEFGHIJ', // the prefix is lower case
    'sk-Short0123abc is short', // under 20 characters (text follows: an open run at the end is the streaming rule)
    ['sk', 'live', 'Synth3ticStripeKey0123456789abc'].join('_'), // another family, out of scope (built here, so no key-shaped literal is committed)
    'the sk- prefix, sk-, and ask-me-anything',
  ]) {
    assert.equal(redactText(keep), keep, keep)
    assert.deepEqual(secretsIn(keep), [], keep)
  }
})

test('streaming: a text that ENDS inside an sk- run is hidden until the run closes', () => {
  for (let cut = 4; cut <= CANARY.length; cut++) {
    const shown = redactText(`my key is ${CANARY.slice(0, cut)}`)
    assert.equal(shown, `my key is ${REDACTED}`, `cut ${cut}`)
  }
  assert.equal(redactText('the prefix sk-'), 'the prefix sk-', 'a bare prefix with no body is kept')
  // secretsIn remembers only a CLOSED key: a 20+ prefix still open at the end is not a value yet
  assert.deepEqual(secretsIn('my key is sk-proofCANARY012345678'), [])
  assert.deepEqual(secretsIn(`my key is ${CANARY}`), [], 'a key ending the text is masked, and remembered once text follows it')
  assert.deepEqual(secretsIn(`my key is ${CANARY} `), [CANARY])
  assert.equal(redactText('we use sk-learn'), `we use ${REDACTED}`, 'documented cost: an open run at the very end is hidden')
  assert.equal(redactText('we use sk-learn now'), 'we use sk-learn now', 'and shown once text follows it')
})

// ── MindStream: host text, agent text, nested/log fields, split deltas, tails ────────────────

/** The real path: envelopes through a real EventWindow, its changes into a MindStream; every
 * Mind change payload is captured as emitted (before the hub's own final redaction). */
function rig() {
  const window = new EventWindow(catalog, { limit: 500 })
  const mind = fixed(new MindStream({ now: () => 1_000_000 }))
  const windowSeen = []
  const mindSeen = []
  window.onChange((change, payload) => {
    windowSeen.push(JSON.stringify(payload))
    if (change === 'added') mind.agentd(payload, { via: 'unfold-events observer feed' })
    else if (change === 'updated') mind.agentdPatch(payload)
    else if (change === 'link') mind.setLink(payload.state, payload.detail)
  })
  mind.onChange((change, payload) => mindSeen.push(JSON.stringify(payload)))
  let seq = 0
  const send = (kind, fields = {}) => {
    seq++
    const env = { v: 2, id: `${P}:${seq}`, producer: P, seq, t_ns: seq * 1_000_000, source: 'guest', kind, ...fields }
    window.push(env)
    return env
  }
  window.emit('link', { state: 'attached', detail: null })
  send('hello', { protocol: 2, agent: 'a', version: 'v', boot_id: BOOT, utc_ms_at_origin: 1, journal: '/j' })
  return { window, mind, send, windowSeen, mindSeen }
}

const everything = (mind, ...more) => {
  const defs = loadDefinitions()
  return [JSON.stringify(mind.snapshot()), ...mind.rows.map((x) => JSON.stringify(mind.detail(x.id, defs))), ...more.flat()].join('\n')
}

test('repro: host started, response_started, reply_text with the bare canary — the snapshot never carries it', () => {
  const mind = fixed(new MindStream({ now: () => 1 }))
  const seen = []
  mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  const o = (offset) => ({ via: 'host journal file', file: '/x', offset })
  mind.host({ kind: 'started', pid: 1, t_ms: 0 }, o(0))
  mind.host({ kind: 'response_started', response_id: 'r1', t_ms: 1 }, o(10))
  mind.host({ kind: 'reply_text', response_id: 'r1', text: `the canary ${CANARY} is here`, t_ms: 2 }, o(20))
  const all = everything(mind, seen)
  assert.equal(fragmentIn(CANARY, all), null, 'host text')
  assert.match(all, /\[redacted\]/)
})

test('the canary in agent text, log messages and nested fields never reaches a row, payload or snapshot', () => {
  const r = rig()
  r.send('log', { level: 'info', msg: `connect with ${CANARY} now` })
  r.send('some_future_kind', { nested: { note: `see ${CANARY}`, list: [{ value: CANARY }, 'plain'] } })
  r.send('response_started', { response_id: 'r1', request_id: 'c1' })
  r.send('agent_delta', { response_id: 'r1', text: `here it is ${CANARY} done` })
  r.send('agent_done', { response_id: 'r1', status: 'completed' })
  const all = everything(r.mind, r.windowSeen, r.mindSeen, JSON.stringify(r.window.rows))
  assert.equal(fragmentIn(CANARY, all), null)
  assert.ok(all.includes('plain') && all.includes(BOOT), 'ordinary values and ids are kept')
})

test('split deltas: no emitted window or Mind payload carries a fragment, at any split and delta size', () => {
  const full = `my key is ${CANARY} and that is all`
  for (const size of [1, 2, 5, 11]) {
    for (let at = 0; at < full.length; at++) {
      const r = rig()
      r.send('response_started', { response_id: 'r1', request_id: 'c1' })
      // the first delta ends at `at`, the rest arrives in deltas of `size`
      const parts = [full.slice(0, at || 1)]
      for (let i = at || 1; i < full.length; i += size) parts.push(full.slice(i, i + size))
      for (const t of parts) r.send('agent_delta', { response_id: 'r1', text: t })
      const all = everything(r.mind, r.windowSeen, r.mindSeen, JSON.stringify(r.window.rows))
      assert.equal(fragmentIn(CANARY, all), null, `first delta ${at}, then ${size}: ${fragmentIn(CANARY, all)}`)
    }
  }
})

test('split deltas on the direct journal path (raw envelopes into MindStream, its own join)', () => {
  const full = `my key is ${CANARY} and that is all`
  for (const size of [1, 3, 7]) {
    const mind = fixed(new MindStream({ now: () => 1 }))
    const seen = []
    mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
    let seq = 0
    const env = (kind, f) => ({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq, source: 'guest', kind, ...f })
    mind.agentd(env('response_started', { response_id: 'rJ' }), { via: 'agentd journal', file: '/j', offset: 0 })
    for (let i = 0; i < full.length; i += size) mind.agentd(env('agent_delta', { response_id: 'rJ', text: full.slice(i, i + size) }), { via: 'agentd journal', file: '/j', offset: i + 1 })
    const all = everything(mind, seen)
    assert.equal(fragmentIn(CANARY, all), null, `deltas of ${size}: ${fragmentIn(CANARY, all)}`)
  }
  // secretsIn: a value already masked in this row is remembered, so a later copy the pattern does
  // not start on (glued to a word, documented boundary) is masked too, in Mind's own join.
  const mind = fixed(new MindStream({ now: () => 1 }))
  const seen = []
  mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  let seq = 0
  const env = (kind, f) => ({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq, source: 'guest', kind, ...f })
  mind.agentd(env('response_started', { response_id: 'rM' }), { via: 'agentd journal', file: '/j', offset: 0 })
  mind.agentd(env('agent_delta', { response_id: 'rM', text: `my key is ${CANARY} and ` }), { via: 'agentd journal', file: '/j', offset: 1 })
  mind.agentd(env('agent_delta', { response_id: 'rM', text: `glued${CANARY} end` }), { via: 'agentd journal', file: '/j', offset: 2 })
  assert.equal(fragmentIn(CANARY, everything(mind, seen)), null, 'remembered value')
  assert.ok(mind.rows.some((r) => r.text === `my key is ${REDACTED} and glued${REDACTED} end`))
})

test('tail boundaries: the 240-char window tail and the 4096-char private keep never show part of it', () => {
  // The window's 240-char tail cut lands at every position through the canary.
  for (let after = 200; after <= 250; after += 1) {
    const r = rig()
    r.send('response_started', { response_id: 'r1', request_id: 'c1' })
    const full = `${'lorem ipsum '.repeat(30)}${CANARY} ${'z'.repeat(after)}`
    for (let i = 0; i < full.length; i += 37) r.send('agent_delta', { response_id: 'r1', text: full.slice(i, i + 37) })
    const all = everything(r.mind, r.windowSeen, r.mindSeen, JSON.stringify(r.window.rows))
    assert.equal(fragmentIn(CANARY, all), null, `tail after ${after}`)
  }
  // Mind's own 4096-char keep on the direct journal path cuts through the canary.
  for (const pad of [4016, 4020, 4030, 4040, 4048]) { // the 4096 cut lands inside the first canary
    const mind = fixed(new MindStream({ now: () => 1 }))
    const seen = []
    mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
    let seq = 0
    const env = (kind, f) => ({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq, source: 'guest', kind, ...f })
    mind.agentd(env('response_started', { response_id: 'rK' }), { via: 'agentd journal', file: '/j', offset: 0 })
    mind.agentd(env('agent_delta', { response_id: 'rK', text: `${CANARY} ` }), { via: 'agentd journal', file: '/j', offset: 1 })
    mind.agentd(env('agent_delta', { response_id: 'rK', text: 'w'.repeat(pad) }), { via: 'agentd journal', file: '/j', offset: 2 })
    mind.agentd(env('agent_delta', { response_id: 'rK', text: ` then ${CANARY} again` }), { via: 'agentd journal', file: '/j', offset: 3 })
    const all = everything(mind, seen)
    assert.equal(fragmentIn(CANARY, all), null, `keep pad ${pad}`)
  }
})

// ── the served surfaces: /api/unfold-mind and its SSE stream ───────────────────────────────────

test('canary on the real /api/unfold-mind snapshot route and SSE stream: no fragment in any frame', async (t) => {
  // Every served time is this test's own: the snapshot carries epoch-ms stamps (`at`, a row's
  // observed_at, an episode's opened_at/last_at, the link's since) and a five-character window of
  // the canary's digit run can coincide with the digits of the wall clock of the moment the test
  // ran (#456: `45678`). Those are not canary text, so the clock is fixed here as the lifetime
  // nonce is (`fixed`): the same bytes are still searched, and the result no longer depends on
  // when the suite runs. Route registration, the Context, its mind and the data dir are built
  // fresh here, and the plugin keeps no route or mind state at module level, so no other test
  // in this file can be seen either.
  t.mock.method(Date, 'now', () => 1_000_000)
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-sk-'))
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const window = new EventWindow(catalog, { limit: 50 })
  let seq = 0
  const push = (kind, fields) => window.push({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  ctx.provide('unfoldEvents', { window, catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: dir })
  try {
    await fiber
    await settle()
    fixed(ctx.get('unfoldMind').mind)
    const abort = new AbortController()
    const res = await routes.find((r) => r.path === '/api/unfold-mind/stream').fetch(new Request('http://h/api/unfold-mind/stream', { signal: abort.signal }))
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let frames = ''
    const pump = (async () => {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) return
        frames += decoder.decode(value)
      }
    })()
    push('response_started', { response_id: 'r1', request_id: 'c1' })
    const full = `my key is ${CANARY} and more`
    for (let i = 0; i < full.length; i += 4) push('agent_delta', { response_id: 'r1', text: full.slice(i, i + 4) })
    push('log', { level: 'info', msg: `log ${CANARY}` })
    for (let i = 0; i < 5; i++) await settle()
    const snap = await (await routes.find((r) => r.path === '/api/unfold-mind').fetch(new Request('http://h/api/unfold-mind'))).text()
    abort.abort()
    await within(pump.catch(() => {}), 2000, 'the stream ends on abort')
    assert.ok(frames.includes('event: '), 'the stream carried frames')
    for (const [what, text] of [['snapshot route', snap], ['SSE frames', frames]]) {
      const frag = fragmentIn(CANARY, text)
      assert.equal(frag, null, frag && `${what}: ${frag} at ${whereIn(text, frag)}`)
    }
  } finally {
    await fiber.dispose()
    rmSync(dir, { recursive: true, force: true })
  }
})

// A quoted field is clipped for its sentence (log at 140, typed text at 90). A clip landing inside a
// key used to leave its first 15 body characters, too short for the sk- rule, so the fragment
// survived serve-time redaction. Each field plants the key across its own clip boundary.
test('a field clipped for its sentence never shows part of a key the clip cut, in the sentence or on the route', async () => {
  const log = `${'a'.repeat(120)} ${CANARY}` // 140 keeps 139: the space, "sk-" and 15 body chars
  const typed = `${'a'.repeat(70)} ${CANARY}` // 90 keeps 89: the same 15 body chars
  for (const [kind, fields] of [['log', { level: 'info', msg: log }], ['user_text', { request_id: 'c1', text: typed }]]) {
    const said = sentence({ lane: 'agentd', kind, fields }).sentence
    assert.equal(fragmentIn(CANARY, said), null, `${kind} sentence: ${maskCanary(said)}`)
  }
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-clip-'))
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const window = new EventWindow(catalog, { limit: 50 })
  let seq = 0
  const push = (kind, fields) => window.push({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  ctx.provide('unfoldEvents', { window, catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: dir })
  try {
    await fiber
    await settle()
    fixed(ctx.get('unfoldMind').mind)
    push('log', { level: 'info', msg: log })
    push('user_text', { request_id: 'c1', text: typed })
    for (let i = 0; i < 5; i++) await settle()
    const snap = await (await routes.find((r) => r.path === '/api/unfold-mind').fetch(new Request('http://h/api/unfold-mind'))).text()
    assert.ok(snap.includes('Agent note (info)') && snap.includes('Received a typed message'), 'both rows were served')
    // Mind's wall clock is real here: its ms readings are JSON numbers, never a quoted field.
    const body = snap.replace(/(?<=":)\d{10,}(?=[,}\]])/g, '0')
    const frag = fragmentIn(CANARY, body)
    assert.equal(frag, null, frag && `snapshot route: ${frag} at ${whereIn(body, frag)}`)
  } finally {
    await fiber.dispose()
    rmSync(dir, { recursive: true, force: true })
  }
})

// The mid-token clip rule (src/sentences.js quote()): a clip that cuts a [A-Za-z0-9_-] run drops
// the whole run, so no part of a word the text rules never saw whole is shown.
test('a clip that cuts a token drops the whole token; a clip at a word boundary keeps the word', () => {
  const said = (msg) => sentence({ lane: 'agentd', kind: 'log', fields: { level: 'info', msg } }).sentence // n = 140
  const typed = (text) => sentence({ lane: 'agentd', kind: 'user_text', fields: { request_id: 'c1', text } }).sentence // n = 90
  // A field that is one long token renders an empty clip.
  assert.ok(said('x'.repeat(200)).endsWith(': “…”'), said('x'.repeat(200)))
  assert.ok(typed('y'.repeat(200)).endsWith(': “…”'), typed('y'.repeat(200)))
  // A benign sk- run at the boundary: 139 kept characters end in "sk-l"; the run goes, "see" stays.
  const benign = `${'b'.repeat(130)} see sk-learn-pipeline-v2 tail`
  assert.ok(said(benign).endsWith(`${'b'.repeat(130)} see…”`), said(benign))
  assert.ok(!said(benign).includes('sk-l'))
  // A key inside a longer word is not a start to the sk- rule ("task-sk-…"), so only this rule
  // keeps its cut run out of the sentence.
  const inWord = `${'c'.repeat(110)} task-${CANARY}`
  assert.equal(fragmentIn(CANARY, said(inWord)), null, maskCanary(said(inWord)))
  assert.ok(said(inWord).endsWith(`${'c'.repeat(110)}…”`), maskCanary(said(inWord)))
  // A clip that lands on a space keeps the whole word before it.
  assert.ok(said(`${'d'.repeat(139)} tail`).endsWith(`${'d'.repeat(139)}…”`))
})

// The clip guard lives in quote(), so every raw-field caller inherits it: log keeps 139 characters,
// typed text, the provider transcript and reply text keep 89. The key straddles the clip at its
// first, middle and last character, the positions the old order turned into a short fragment.
test('every raw-field caller keeps a key out of its sentence when the clip lands inside it', () => {
  const callers = [
    ['log', 140, (text) => sentence({ lane: 'agentd', kind: 'log', fields: { level: 'info', msg: text } }).sentence],
    ['user_text', 90, (text) => sentence({ lane: 'agentd', kind: 'user_text', fields: { request_id: 'c1', text } }).sentence],
    ['input_transcript', 90, (text) => sentence({ lane: 'agentd', kind: 'input_transcript', fields: { text } }).sentence],
    ['host input_transcript', 90, (text) => sentence({ lane: 'host', kind: 'input_transcript', fields: { text } }).sentence],
    ['reply_text', 90, (text) => sentence({ lane: 'host', kind: 'reply_text', fields: { response_id: 'r1', text } }).sentence],
  ]
  for (const [name, n, said] of callers) {
    for (const r of [0, Math.floor(CANARY.length / 2), CANARY.length - 1]) {
      const pad = n - 2 - r // the key starts at pad + 1, so its r-th character is the first dropped
      const out = said(`${'a'.repeat(pad)} ${CANARY} tail`)
      assert.equal(fragmentIn(CANARY, out), null, `${name}, clip inside the key at ${r}: ${maskCanary(out)}`)
    }
  }
})

// Every caller that quotes a raw field into its sentence.
const RAW_CALLERS = [
  ['log', (text) => sentence({ lane: 'agentd', kind: 'log', fields: { level: 'info', msg: text } }).sentence],
  ['user_text', (text) => sentence({ lane: 'agentd', kind: 'user_text', fields: { request_id: 'c1', text } }).sentence],
  ['command_submitted', (text) => sentence({ lane: 'agentd', kind: 'command_submitted', fields: { command_id: 'k1', text } }).sentence],
  ['input_transcript', (text) => sentence({ lane: 'agentd', kind: 'input_transcript', fields: { text } }).sentence],
  ['host input_transcript', (text) => sentence({ lane: 'host', kind: 'input_transcript', fields: { text } }).sentence],
  ['reply_text', (text) => sentence({ lane: 'host', kind: 'reply_text', fields: { response_id: 'r1', text } }).sentence],
  ['agent_delta', (text) => sentence({ lane: 'agentd', kind: 'agent_delta', fields: { response_id: 'r1', text } }).sentence],
]

// A YAML block scalar (`password: |`, `apiKey: >`, `db_password: |-`, then the value on the lines
// under it) is hidden whole by a line scanner (#346, #347). quote() collapses whitespace for the
// sentence, so it redacts the raw field first: collapsed, the name rule would take the `|` or `>`
// as the value and quote the real value line in clear. The key line's own line end (\n, \r\n or a
// lone \r, each one line end; an LF or CR key's \r\n too) ends a block line; any other \r or \n
// is a stray inside the value. A line ends the block, shown, only when its first other character
// comes after spaces alone, no more of them than its key's indent; any other line is value (blank,
// deeper, or a tab, a stray or a character that prints nothing before its first other character).
// The key line is untrusted text: a quoted key, an anchor and a tag, a comment after the
// indicator and an explicit key are key lines too. One table, every repro of the review rounds,
// each row through the raw text (mind's redaction, the served field, the window's copy) and every
// raw-field caller (its sentence and its served form).
const SECRET = ['velvet', 'lantern', 'harbor'].join('') // plain words built at run time: findable, not key-shaped
const MORE = ['orchard', 'meadow', 'pebble'].join('')
// A failure prints its text with every 5-char fragment of either value masked.
const masked = (t) => [SECRET, MORE].flatMap((v) => [v, ...fragmentsOf(v)]).reduce((m, f) => m.split(f).join('[secret]'), t)
const EOLS = [['LF', '\n'], ['CRLF', '\r\n'], ['CR', '\r']]
// Each key line end with a stray line end it does not use.
const STRAYS = [['LF', '\n', 'CR', '\r'], ['CRLF', '\r\n', 'CR', '\r'], ['CRLF', '\r\n', 'LF', '\n'], ['CR', '\r', 'LF', '\n']]
// A row: its raw text; `after`, the text after the block, shown; `exact`, the whole redaction; `lf`,
// the same text with LF line ends, whose redaction and sentences it must equal but for them.
const YAML_ROWS = [
  // Rounds 1-2: a block with LF, CRLF or lone CR line ends is the LF block, its own line ends kept.
  ...[
    ['password: |', (n) => `config:${n}  password: |${n}    ${SECRET}${n}  next: 1`],
    ['apiKey: >', (n) => `creds:${n}  apiKey: >${n}    ${SECRET}${n}    ${MORE}${n}`],
    ['db_password: |-', (n) => `db_password: |-${n}  ${SECRET}${n}  ${MORE}`],
  ].flatMap(([label, block]) => EOLS.map(([eol, n]) => ({ name: `${label}, a ${eol} block`, raw: block(n), lf: block('\n'), n }))),
  // Round 3: a lone CR inside an LF block is value (the reviewer's repro).
  { name: 'a lone CR inside an LF block', raw: `password: |\n  ${SECRET}\r${MORE}\nnext: 1`, after: '\nnext: 1' },
  // Round 4: each key line end with each stray it does not use, mid-line, at a line start, on a
  // blank line; the unindented line after the block is shown.
  ...STRAYS.flatMap(([keyEol, n, strayName, s]) =>
    [['password: |', ''], ['apiKey: >', '  '], ['db_password: |-', '']].flatMap(([key, i]) =>
      [
        ['mid-line', `${i}${key}${n}${i}  ${SECRET}${s}${MORE}${n}${i}next: shown`],
        ['at a line start', `${i}${key}${n}${i}  ${SECRET}${n}${s}${i}  ${MORE}${n}${i}next: shown`],
        ['on a blank line', `${i}${key}${n}${i}  ${SECRET}${n}${s}${n}${i}  ${MORE}${n}${i}next: shown`],
      ].map(([where, raw]) => ({ name: `${key}, ${keyEol} key, a stray ${strayName} ${where}`, raw, after: `${n}${i}next: shown` })),
    ),
  ),
  // Round 4: the key's own line end comes first (an LF or CR key's \r\n): the line has ended.
  ...[['LF', '\n'], ['CR', '\r']].map(([keyEol, n]) => ({ name: `${keyEol} key, then \\r\\n`, raw: `password: |${n}  ${SECRET}\r\nnext: shown`, after: 'next: shown' })),
  // Round 5: a stray after part of a nested key's indent (the reviewer's repros, then 5 indents
  // around a stray for each of the 4 key/stray pairs).
  { name: 'nested key, a space then a stray CR then 3 spaces', raw: `  password: |\n \r   ${SECRET}\nnext: 1`, after: '\nnext: 1' },
  { name: 'nested key, its indent then a stray CR then 3 spaces, at the end', raw: `  password: |\n  \r   ${SECRET}` },
  ...STRAYS.flatMap(([keyEol, n, strayName, s]) =>
    [[' ', '   '], ['  ', '   '], [' ', ''], [' ', ' '], ['\t', '  ']].map(([pre, post]) => ({
      name: `nested key, ${keyEol} key, ${JSON.stringify(pre)} then a stray ${strayName} then ${JSON.stringify(post)}`,
      raw: `  password: |${n}${pre}${s}${post}${SECRET}${n}  next: 1`,
      after: `${n}  next: 1`,
    })),
  ),
  // The value ends at a less-indented key, its blank lines with it.
  ...EOLS.map(([eol, n]) => ({
    name: `the value ends at a less-indented key, ${eol}`,
    raw: `config:${n}  password: |${n}    ${SECRET}${n}${n}    ${MORE}${n}next: shown`,
    exact: `config:${n}  password: |${n}    ${REDACTED}${n}next: shown`,
  })),
  // Round 6: the key line is untrusted text, not validated YAML (the reviewer's repros): a quoted
  // key, an anchor and a tag in either order, a comment after the indicator, an explicit key.
  ...[
    ['a double-quoted key', `"password": |\n  ${SECRET}`],
    ['a single-quoted key', `'password': |\n  ${SECRET}`],
    ['an indented quoted key', `  "password": |\n    ${SECRET}`],
    ['a quoted key in a list', `- "password": |\n    ${SECRET}`],
    ['a quoted key with an anchor', `"password": &a |\n  ${SECRET}`],
    ['an anchored key', `password: &a |\n  ${SECRET}`],
    ['a tag', `password: !!str |\n  ${SECRET}`],
    ['an anchor then a tag', `password: &a !!str |\n  ${SECRET}`],
    ['a tag then an anchor', `password: !!str &a |\n  ${SECRET}`],
    ['a comment after the indicator', `password: | # note\n  ${SECRET}`],
    ['a comment with no space', `password: |#note\n  ${SECRET}`],
    ['an explicit key', `? password\n: |\n  ${SECRET}`],
    // Round 6 (explicit key with a gap): comment and blank lines between `? key` and `: |`.
    ['an explicit key, a comment line before its colon', `? password\n# note\n: |\n  ${SECRET}`],
    ['an explicit key, a blank line before its colon', `? password\n\n: |\n  ${SECRET}`],
    ['an explicit key, an indented comment before its colon', `? password\n  # note\n: |\n  ${SECRET}`],
  ].map(([name, raw]) => ({ name, raw })),
  // A tab in a line's indent is value, never a terminator, and a NUL or another
  // character that prints nothing on the key line does not stop the key from matching.
  { name: 'a tab as the indent under a nested key', raw: `  password: |\n\t${SECRET}\n  next: 1`, after: '\n  next: 1' },
  { name: 'a space then a tab under a nested key', raw: `  password: |\n \t${SECRET}\n  next: 1`, after: '\n  next: 1' },
  { name: 'a tab-indented line after the value', raw: `  password: |\n    ${SECRET}\n\t${MORE}\n  next: 1`, after: '\n  next: 1' },
  { name: 'a tab-indented key, a space-indented value', raw: `\tpassword: |\n    ${SECRET}\nnext: 1`, after: '\nnext: 1' },
  { name: 'a NUL before the colon', raw: `password\0: |\n  ${SECRET}` },
  { name: 'a NUL after a quoted key', raw: `"password"\0: |\n  ${SECRET}` },
  { name: 'a NUL between the colon and the indicator', raw: `password:\0|\n  ${SECRET}` },
  { name: 'a NUL after the indicator', raw: `password: |\0\n  ${SECRET}` },
  { name: 'a DEL after the indicator', raw: `password: |\x7f\n  ${SECRET}` },
  { name: 'a zero-width space before the colon', raw: `password​: |\n  ${SECRET}` },
  { name: 'a BOM after the colon', raw: `password:﻿|\n  ${SECRET}` },
  { name: 'a NUL before a value line', raw: `  password: |\n\0   ${SECRET}\n  next: 1`, after: '\n  next: 1' },
  { name: 'a NUL before the key', raw: `\0password: |\n  ${SECRET}` },
  // Kept from rounds 1-5 (pinned): every indicator, the spacing around the colon, the name forms.
  ...[
    ['|+', `password: |+\n  ${SECRET}`],
    ['>', `password: >\n  ${SECRET}`],
    ['>-', `password: >-\n  ${SECRET}`],
    ['>+', `password: >+\n  ${SECRET}`],
    ['|2', `password: |2\n  ${SECRET}`],
    ['|2-', `password: |2-\n  ${SECRET}`],
    ['|-2', `password: |-2\n  ${SECRET}`],
    ['a space before the colon', `password : |\n  ${SECRET}`],
    ['a tab before the indicator', `password:\t|\n  ${SECRET}`],
    ['UPPER', `PASSWORD: |\n  ${SECRET}`],
    ['DB_Password', `DB_Password: |\n  ${SECRET}`],
    ['api-key', `api-key: |\n  ${SECRET}`],
    ['db.password', `db.password: |\n  ${SECRET}`],
    ['db/password', `db/password: |\n  ${SECRET}`],
    ['--- before the key', `---\npassword: |\n  ${SECRET}`],
    ['two spaces around the colon', `password  :  |\n  ${SECRET}`],
  ].map(([name, raw]) => ({ name: `pinned: ${name}`, raw })),
]

for (const row of YAML_ROWS) {
  test(`YAML block: ${row.name}`, () => {
    const { raw, after, exact, lf, n } = row
    for (const [form, rt] of [['redactText', redactText], ['redact', (t) => redact({ text: t }).text], ['window', windowRedactText]]) {
      const out = rt(raw)
      assert.ok(out.includes(REDACTED), `${form} redacts the block ${JSON.stringify(masked(out))}`)
      for (const value of [SECRET, MORE]) assert.equal(fragmentIn(value, out), null, `${form} ${JSON.stringify(masked(out))}`)
      if (after !== undefined) assert.ok(out.endsWith(after), `${form} shows the line after the block ${JSON.stringify(masked(out))}`)
      if (exact !== undefined) assert.equal(out, exact, `${form} ${JSON.stringify(masked(out))}`)
      if (lf !== undefined) assert.equal(out, rt(lf).split('\n').join(n), `${form} is the LF redaction ${JSON.stringify(masked(out))}`)
    }
    for (const [name, said] of RAW_CALLERS) {
      for (const field of [raw, `${'w'.repeat(30)} ${raw}`]) {
        const out = said(field)
        const served = redact({ sentence: out }).sentence
        for (const value of [SECRET, MORE]) {
          assert.equal(fragmentIn(value, out), null, `${name}: sentence ${JSON.stringify(masked(out))}`)
          assert.equal(fragmentIn(value, served), null, `${name}: served ${JSON.stringify(masked(served))}`)
        }
        if (lf !== undefined) assert.equal(out, said(field.split(n).join('\n')), `${name}: the LF sentence`)
      }
    }
  })
}

// The sentence redacts the raw field ONCE, before its whitespace is collapsed (src/sentences.js
// quote()): a field's line ends still separate a key line from its value while the rules read it.
// A second pass over the collapsed sentence read the joined text as a key line instead — a
// comment line that named a key became a key line, and a parent's anchor became its value.
// These pin the three shapes between the two passes, through a raw-field caller, so a change to
// either the pass order or the rules has to say what it does to them (#396).
const ROSE = ['Qw7', 'zebra'].join('') // plain words built at run time: findable, not key-shaped
const sentenceOf = (text) => sentence({ lane: 'agentd', kind: 'agent_delta', fields: { response_id: 'r1', text } }).sentence

// G05: the key line is a comment, so nothing under it is a value: both lines are served as written.
test('a sentence keeps a comment line that names a key, and the line under it', () => {
  assert.equal(sentenceOf(`# db_pass:\n  ${ROSE}`), `Wrote reply text for r1: “# db_pass: ${ROSE}”`)
})

// L50: the parent's anchor is a property, the child key stays its own key; only the child's value
// is hidden. A second pass hid the anchor and the child key with it.
test('a sentence keeps a parent anchor and a child key, and hides only the child value', () => {
  assert.equal(sentenceOf(`secret: &d\n  db_pass: ${ROSE}`), 'Wrote reply text for r1: “secret: &d db_pass: [redacted]”')
})

// A plain later-line value is hidden, and the lines around it are served as written.
test('a sentence hides a plain later-line value and serves its neighbours', () => {
  assert.equal(sentenceOf(`alpha: 1\npassword:\n  ${ROSE}\nomega: 2`), 'Wrote reply text for r1: “alpha: 1 password: [redacted] omega: 2”')
})
