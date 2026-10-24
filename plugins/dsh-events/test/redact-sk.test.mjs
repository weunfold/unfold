// The events copy of the credential patterns: a standalone sk- credential never leaves the
// window, in a log message, a nested field, a reply split across deltas, the 240-char tail or the
// 16 KiB private bound. Every emitted added/updated payload is inspected, not only the final row.
// Synthetic values only.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../src/catalog.mjs'
import { EventWindow, redact, redactText } from '../src/window.mjs'
import { redactText as mindRedactText } from '../../dsh-mind/src/redact.mjs'
import { redactText as settingsRedactText } from '../../dsh-settings/src/redact.mjs'

const catalog = loadCatalog()
const CANARY = 'sk-proofCANARY0123456789abcdefABCDEF'
const fragmentsOf = (s, n = 5) => Array.from({ length: s.length - n + 1 }, (_, i) => s.slice(i, i + n))
const fragmentIn = (s, text) => fragmentsOf(s).find((f) => text.includes(f)) ?? null
const delta = (i, text, response = 'rS') => ({ v: 2, id: `g:${response}:d${i}`, producer: 'g', seq: 1000 + i, t_ns: i, source: 'guest', kind: 'agent_delta', cause: 'g:decision', response_id: response, text })

function rig() {
  const w = new EventWindow(catalog, { limit: 20 })
  const seen = []
  w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  return { w, seen, all: () => [...seen, JSON.stringify(w.rows), JSON.stringify(w.status ?? null)].join('\n') }
}

test('events: the three copies of the text patterns agree on the canary, its variants and the controls', () => {
  const inputs = [
    `my key is ${CANARY} ok`, `"${CANARY}"`, `line\\n${CANARY}`, `api_key: "${CANARY}"`, `Authorization: Bearer ${CANARY}`,
    'sk-proj-Synth3tic_Proj-Key4Value5abcDEF6ghiJKL7mno', 'sk-ant-api03-Synth3tic_AntKey-4Value5abcDEF6ghiJKL7mnoPQR8AA',
    'we use sk-learn now', 'risk-Assessment0123456789abcdefABCDEF', 'sha 8bc340e750b82c176990fe60d623d024f0e59da7f21d6cb8dc33de7e1472110d',
    `ends open ${CANARY.slice(0, 9)}`, 'Bearer abcdef0123456789secret', 'key AIzaSyD-THIS-IS-A-FAKE-KEY-0123456789',
  ]
  for (const s of inputs) {
    assert.equal(redactText(s), mindRedactText(s), `events vs mind: ${s}`)
    assert.equal(redactText(s), settingsRedactText(s), `events vs settings: ${s}`)
  }
  assert.equal(redactText(`my key is ${CANARY} ok`), 'my key is [redacted] ok')
  assert.equal(redactText('we use sk-learn now'), 'we use sk-learn now')
})

test('events: a log message and nested fields with the bare canary are masked in every payload', () => {
  const r = rig()
  r.w.push({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'log', level: 'info', msg: `connect with ${CANARY} now` })
  r.w.push({ v: 2, id: 'g:2', producer: 'g', seq: 2, t_ns: 2, source: 'guest', kind: 'some_future_kind', response_id: 'r-keep', nested: { note: `see ${CANARY}`, list: [CANARY, 'plain'] } })
  r.w.push({ v: 2, id: 'g:3', producer: 'g', seq: 3, t_ns: 3, source: 'guest', kind: 'heartbeat', uptime_ms: 1, attachments: 1, detail: { line: CANARY } })
  assert.equal(fragmentIn(CANARY, r.all()), null, fragmentIn(CANARY, r.all()))
  assert.equal(r.w.rows[1].fields.nested.list[1], 'plain')
  assert.equal(r.w.rows[1].fields.response_id, 'r-keep', 'protocol identities are kept')
  assert.deepEqual(redact({ a: { b: [`x ${CANARY}`] } }), { a: { b: ['x [redacted]'] } })
})

test('events split deltas: no added or updated payload ever carries a fragment, at any split and delta size', () => {
  const full = `my key is ${CANARY} and that is all`
  for (const size of [1, 2, 5, 11]) {
    for (let at = 1; at < full.length; at++) {
      const r = rig()
      let i = 0
      r.w.push(delta(i++, full.slice(0, at)))
      for (let k = at; k < full.length; k += size) r.w.push(delta(i++, full.slice(k, k + size)))
      assert.equal(fragmentIn(CANARY, r.all()), null, `first delta ${at}, then ${size}: ${fragmentIn(CANARY, r.all())}`)
      assert.equal(r.w.rows[0].text, 'my key is [redacted] and that is all')
    }
  }
})

test('events tail and bound: the 240-char tail and the 16 KiB private bound never show part of it', () => {
  for (let after = 200; after <= 250; after++) {
    const r = rig()
    const full = `${'lorem ipsum '.repeat(30)}${CANARY} ${'z'.repeat(after)}`
    let i = 0
    for (let k = 0; k < full.length; k += 37) r.w.push(delta(i++, full.slice(k, k + 37)))
    assert.ok(r.w.rows[0].text.length <= 240)
    assert.equal(fragmentIn(CANARY, r.all()), null, `tail after ${after}`)
  }
  // The 16 KiB bound drops the head after whitespace only: the canary is kept whole or dropped whole.
  for (const pad of [16384 - 40, 16384 - 20, 16384 - 5, 16384]) {
    const r = rig()
    r.w.push(delta(0, `${CANARY} `))
    r.w.push(delta(1, `${'w '.repeat(pad / 2)}`))
    r.w.push(delta(2, `then ${CANARY.slice(0, 12)}`))
    r.w.push(delta(3, `${CANARY.slice(12)} end`))
    assert.equal(fragmentIn(CANARY, r.all()), null, `bound pad ${pad}`)
    assert.ok(r.w.rows[0].text.endsWith('then [redacted] end'), r.w.rows[0].text.slice(-40))
  }
})
