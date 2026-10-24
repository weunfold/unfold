// Quoted credentials on the Events → Mind path: an unfinished QUOTED credential value (`MY_TOKEN="…`) is never shown by rth-events or Mind while
// it streams, nor after rth-events' private-buffer trim. Every emitted payload is checked.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow, redactText as eventsRedact } from '../../dsh-events/src/window.mjs'
import { MindStream } from '../src/mind.mjs'
import { redactText } from '../src/redact.mjs'

const catalog = loadCatalog()
const VALUE = 'abcdefgh'
let seq = 0
function rig() {
  const window = new EventWindow(catalog)
  const mind = new MindStream({ limit: 10 })
  const events = []
  const minds = []
  mind.onChange((t, p) => minds.push(JSON.stringify(p)))
  window.onChange((t, p) => {
    events.push(JSON.stringify(p))
    if (t === 'added') mind.agentd(p)
    if (t === 'updated') mind.agentdPatch(p)
  })
  const push = (text) => window.push({ v: 2, id: `synthetic:${++seq}`, producer: 'synthetic', seq, t_ns: seq * 1e6, source: 'guest', kind: 'agent_delta', response_id: 'test', text })
  return { window, mind, events, minds, push }
}
const leaked = (r) => [...r.events, ...r.minds, JSON.stringify(r.mind.snapshot())].filter((s) => s.includes(VALUE))

test('the closing quote in the next delta; nothing emitted ever carried the value', () => {
  const r = rig()
  const full = `MY_TOKEN="prefix ${VALUE}"`
  r.push(full.slice(0, -1))
  assert.equal(r.window.rows[0].text, 'MY_TOKEN=[redacted]', 'hidden while the quote is open')
  r.push('"')
  assert.deepEqual(leaked(r), [])
  assert.ok(!redactText(full.slice(0, -1)).includes(VALUE), "Mind's own redaction hides the open value too")
})

test('a quoted value crossing the 16 KiB private bound is never shown on its own', () => {
  const r = rig()
  r.push('ordinary start ')
  r.push(`MY_TOKEN="prefix ${' '.repeat(17000)}${VALUE}"`)
  assert.ok(!r.window.rows[0].text.includes(VALUE), r.window.rows[0].text.slice(0, 60))
  assert.deepEqual(leaked(r), [])
})

test('the copied credential patterns are identical in dsh-events and dsh-mind', () => {
  for (const s of [`MY_TOKEN="open ${VALUE}`, `{"api_key": "sk-${VALUE} x"}`, `token: '${VALUE} y`, 'say "hi" there', `Bearer ${VALUE}`]) {
    assert.equal(eventsRedact(s), redactText(s), s)
  }
})

// JSON escapes inside a quoted credential value, on the real Events → Mind path.
test('an escaped quote in a JSON api_key leaks in neither Events nor Mind', () => {
  const r = rig()
  r.push(JSON.stringify({ api_key: `prefix " ${VALUE}` }))
  assert.equal(r.window.rows[0].text, '{"api_key":"[redacted]"}')
  assert.deepEqual(leaked(r), [])
})

test('a backslash/quote pair split across deltas leaks in neither Events nor Mind', () => {
  const r = rig()
  r.push('{"api_key": "prefix \\')
  r.push(`" ${VALUE}`)
  r.push('", "ok": 1}')
  assert.equal(r.window.rows[0].text, '{"api_key": "[redacted]", "ok": 1}')
  assert.deepEqual(leaked(r), [])
  assert.ok(!redactText(`{"api_key": "prefix \\" ${VALUE}"}`).includes(VALUE), "Mind's own redaction honours the escape")
})

// Whole-value priority for quoted credential values, on the real Events → Mind path.
test('Bearer / Basic / [redacted] prefixes expose nothing in Events or Mind', () => {
  for (const prefix of ['Bearer prefix ', 'Basic prefix ', '[redacted] ']) {
    const r = rig()
    r.push(JSON.stringify({ api_key: prefix + VALUE }))
    assert.equal(r.window.rows[0].text, '{"api_key":"[redacted]"}', prefix)
    assert.deepEqual(leaked(r), [], prefix)
    const s = rig() // streamed
    for (const part of ['{"api_key": "', prefix, VALUE.slice(0, 4), `${VALUE.slice(4)}"}`]) s.push(part)
    assert.deepEqual(leaked(s), [], `streamed ${prefix}`)
    assert.equal(redactText(JSON.stringify({ api_key: prefix + VALUE })), '{"api_key":"[redacted]"}', `Mind's own redaction, ${prefix}`)
  }
})
