// A standalone sk- credential, with no key, assignment or scheme around it, must never reach a
// Mind row, snapshot, audit record or SSE payload, in host text or agent text, whole, nested, split across deltas or cut by a tail.
// Synthetic values only: CANARY is a fixed canary; the others are made up for the variants.
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
import { MindStream } from '../src/mind.mjs'
import { REDACTED, redact, redactText, secretsIn } from '../src/redact.mjs'

const catalog = loadCatalog()
const BOOT = 'b0'
const P = `guest-${BOOT}`
const settle = () => new Promise((r) => setImmediate(r))
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
    'artifact_id 3f2a9c1e-7b4d-4e2a-9c1f-0a1b2c3d4e5f boot b0 host-1-pid42 rth-de3aecdb-m17',
    'risk-Assessment0123456789abcdefABCDEF and desk-Booking0123456789abcdefABC', // not an sk- start
    'task-sk-Abc0123456789defghijklmno', // "-" before sk- is not a start
    'we use sk-learn and sk-learn-pipeline-v2 here', // no upper case: prose, kept
    'SK-UPPER0123456789ABCDEFGHIJ', // the prefix is lower case
    'sk-Short0123abc is short', // under 20 characters (text follows: an open run at the end is the streaming rule)
    'sk_live_Synth3ticStripeKey0123456789abc', // another family, out of scope
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
    if (change === 'added') mind.agentd(payload, { via: 'rth-events observer feed' })
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

test('host started, response_started, reply_text with the bare canary — the snapshot never carries it', () => {
  const mind = fixed(new MindStream({ now: () => 1 }))
  const seen = []
  mind.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  const o = (offset) => ({ via: 'rth-host --log file', file: '/x', offset })
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

// ── the served surfaces: /api/rth-mind and its SSE stream ───────────────────────────────────

test('canary on the real /api/rth-mind snapshot route and SSE stream: no fragment in any frame', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'rth-mind-sk-'))
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => routes.splice(routes.indexOf(route), 1)) } })
  const window = new EventWindow(catalog, { limit: 50 })
  let seq = 0
  const push = (kind, fields) => window.push({ v: 2, id: `${P}:${++seq}`, producer: P, seq, t_ns: seq * 1000, source: 'guest', kind, ...fields })
  ctx.provide('rthEvents', { window, catalog, snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: dir })
  try {
    await fiber
    await settle()
    fixed(ctx.get('rthMind').mind)
    const abort = new AbortController()
    const res = await routes.find((r) => r.path === '/api/rth-mind/stream').fetch(new Request('http://h/api/rth-mind/stream', { signal: abort.signal }))
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
    const snap = await (await routes.find((r) => r.path === '/api/rth-mind').fetch(new Request('http://h/api/rth-mind'))).text()
    abort.abort()
    await pump.catch(() => {})
    assert.ok(frames.includes('event: '), 'the stream carried frames')
    for (const [what, text] of [['snapshot route', snap], ['SSE frames', frames]]) assert.equal(fragmentIn(CANARY, text), null, `${what}: ${fragmentIn(CANARY, text)}`)
  } finally {
    await fiber.dispose()
    rmSync(dir, { recursive: true, force: true })
  }
})
