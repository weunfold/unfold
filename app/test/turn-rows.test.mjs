// The icon row under each reply, headless: the page is fed a reply's frames in the order the app sends them, as
// recorded from agentd on the DSH route (the route nod from response_provenance, the deltas, turn_summary, the end).
// Fixtures only: no agentd, no chat of anyone's.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> node --test test/turn-rows.test.mjs
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const PW = process.env.PLAYWRIGHT_CORE
if (!PW) { console.error('PLAYWRIGHT_CORE is unset: set it to the path of playwright-core\'s index.mjs'); process.exit(1) }
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/index.html')}#splash=off`
const { chromium } = await import(PW)

// The route wire as the app builds it from response_provenance (flash.rs route_wire): the nod's fields, no
// response_id.
const ROUTE = {
  wire: 'route',
  provenance: { backend: 'dsh+local@granite41-3b-q4km', model: 'deepseek-official/deepseek-flash', device_kind: 'Mac', config: { effort: 'off', thread: 'main', thinking: false } },
  names: { 'deepseek-official/deepseek-flash': 'DeepSeek V4 Flash' },
}
const summary = (rid, extra = {}) => ({ wire: 'turn_summary', summary: { kind: 'turn_summary', response_id: rid, lang: 'en', memory: { decision: 'none', used: [], changed: [], sent_to_cloud: false, blocked: null }, shown_text: { history_guard: false, markdown_stripped: false, sentence_cased: false, truncated: null }, effects: [], ...extra } })

async function open(browser) {
  const p = await browser.newPage({ viewport: { width: 900, height: 700 } })
  const errors = []
  p.on('pageerror', (e) => errors.push(e.message))
  await p.goto(PAGE)
  await p.waitForFunction(() => typeof window.__rthReply === 'function' && typeof window.__rthSigns === 'function')
  return { p, errors }
}
const feed = (p, frames) => p.evaluate((frames) => {
  for (const [to, m] of frames) (to === 'reply' ? window.__rthReply : window.__rthMemory)(m)
}, frames)
// Each reply's text and the sign kinds drawn right under it.
const rows = (p) => p.evaluate(() => [...document.querySelectorAll('#convo .it')].map((it) => {
  const next = it.nextElementSibling
  return { text: it.textContent, signs: next && next.classList.contains('signs') ? [...next.querySelectorAll('button[data-kind]')].map((b) => b.dataset.kind) : [] }
}))

test('a reply on the DSH route gets its icon row: the route and effort, and what its summary says', async () => {
  const browser = await chromium.launch()
  try {
    const { p, errors } = await open(browser)
    await feed(p, [['memory', ROUTE], ['reply', { delta: 'Oi' }], ['reply', { delta: '!' }], ['memory', summary('b-r1', { shown_text: { history_guard: false, markdown_stripped: true, sentence_cased: false, truncated: null } })], ['reply', { done: true }]])
    assert.deepEqual(await rows(p), [{ text: 'Oi!', signs: ['route', 'effort', 'safety'] }])
    await feed(p, [['memory', ROUTE], ['reply', { delta: 'Second.' }], ['memory', summary('b-r2')], ['reply', { done: true }]])
    assert.deepEqual((await rows(p)).map((r) => r.signs), [['route', 'effort', 'safety'], ['route', 'effort']], 'each reply keeps its own row')
    assert.deepEqual(errors, [])
  } finally {
    await browser.close()
  }
})

test('a reply with no route nod (agentd\'s own fixed reply) still gets the row its summary gives', async () => {
  const browser = await chromium.launch()
  try {
    const { p, errors } = await open(browser)
    await feed(p, [['reply', { delta: 'Done.' }], ['memory', summary('b-r3', { effects: [{ effect_id: 'e-1', effect_kind: 'forget', state: 'card' }] })], ['reply', { done: true }]])
    assert.deepEqual(await rows(p), [{ text: 'Done.', signs: ['action'] }])
    assert.deepEqual(errors, [])
  } finally {
    await browser.close()
  }
})

test('a replaced reply shows only the new text in its bubble, with its row', async () => {
  const browser = await chromium.launch()
  try {
    const { p, errors } = await open(browser)
    await feed(p, [['memory', ROUTE], ['reply', { delta: 'Ana buys seven pens. ' }], ['reply', { delta: 'She gives three away, so' }], ['reply', { replace: '4', response_id: 'b-r4' }], ['memory', summary('b-r4')], ['reply', { done: true }]])
    assert.deepEqual(await rows(p), [{ text: '4', signs: ['route', 'effort'] }])
    assert.deepEqual(errors, [])
  } finally {
    await browser.close()
  }
})
