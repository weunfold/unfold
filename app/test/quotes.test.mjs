// The quoted text in the conversation's cards, headless: the page is fed the frames the app passes on, a forget
// card (agentd's effect_card) and a DSH watch notice (dsh_notice; dsh.rs caps its summary at 500 characters). Each
// quotes at most 280 characters and marks a cut. Fixtures only: no agentd, no DSH.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> node --test test/quotes.test.mjs
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const PW = process.env.PLAYWRIGHT_CORE
if (!PW) { console.error('PLAYWRIGHT_CORE is unset: set it to the path of playwright-core\'s index.mjs'); process.exit(1) }
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/index.html')}#splash=off`
const { chromium } = await import(PW)

const LONG = 'All tests pass and the branch is ready. '.repeat(12).trim()
const EXACT = 'x'.repeat(280)

// Feeds the frames, then returns the quotes of the cards that match `sel`, in order.
async function quotes(frames, sel) {
  const browser = await chromium.launch()
  try {
    const p = await browser.newPage({ viewport: { width: 900, height: 700 } })
    const errors = []
    p.on('pageerror', (e) => errors.push(e.message))
    await p.goto(PAGE)
    await p.waitForFunction(() => typeof window.__rthMemory === 'function')
    await p.evaluate((fs) => fs.forEach((f) => window.__rthMemory(f)), frames)
    const q = await p.evaluate((sel) => [...document.querySelectorAll(`#convo ${sel} .ec-quote`)].map((q) => q.textContent), sel)
    assert.deepEqual(errors, [])
    return q
  } finally {
    await browser.close()
  }
}
function cutRule([short, whole, cut]) {
  assert.equal(short, '“Done.”')
  assert.equal(whole, `“${EXACT}”`, 'exactly 280 characters is not cut')
  assert.ok(cut.endsWith('…”'), `a cut quote ends with an ellipsis: ${cut.slice(-12)}`)
  assert.ok(LONG.startsWith(cut.slice(1, -2)) && cut.length <= 283, 'what is shown is the text\'s start, at most 280 characters')
}

test('a forget card quotes its summary whole up to 280 characters, and marks a longer one as cut', async () => {
  const card = (id, summary) => ({ wire: 'effect_card', effect_id: id, effect_kind: 'forget', summary })
  cutRule(await quotes([card('e-1', 'Done.'), card('e-2', EXACT), card('e-3', LONG)], '.effect-card:not(.dsh-notice)'))
})

test('a DSH notice quotes its summary whole up to 280 characters, and marks a longer one as cut', async () => {
  const notice = (summary) => ({ wire: 'dsh_notice', event_kind: 'turn_done', agent_name: 'builder', summary })
  cutRule(await quotes([notice('Done.'), notice(EXACT), notice(LONG)], '.dsh-notice'))
})
