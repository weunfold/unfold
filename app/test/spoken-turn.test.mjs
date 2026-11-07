// A turn the person spoke goes to the app as 'say_spoken' (its reply is said in the chosen voice), a typed one as
// 'say', headless. No microphone: the bar's dictation is driven through window.__rthDictation as the engine would.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> [BROWSER=webkit] node --test test/spoken-turn.test.mjs
// (webkit: the engine of the app's own web view)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const PW = process.env.PLAYWRIGHT_CORE
if (!PW) { console.error('PLAYWRIGHT_CORE is unset: set it to the path of playwright-core\'s index.mjs'); process.exit(1) }
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/index.html')}#splash=off`
const engines = await import(PW)
const engine = engines[process.env.BROWSER || 'chromium']

async function chat(browser) {
  const p = await browser.newPage({ viewport: { width: 900, height: 700 } })
  const errors = []
  p.on('pageerror', (e) => errors.push(e.message))
  await p.addInitScript(() => {
    window.__sent = []
    window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => { if (cmd === 'rth_setup' && /^say/.test(args.action)) window.__sent.push(args); return Promise.resolve() } }
  })
  await p.goto(PAGE)
  await p.evaluate(() => window.__rthSetup({ screen: 'home', chat: 'ready', admitted: true, models: [{ part: 'talk', options: [] }] }))
  return { p, errors }
}
// one turn: `how` fills the bar, then it is sent and its reply ends
const turn = (p, how) => p.evaluate((how) => {
  const ta = document.getElementById('say-text'), d = window.__rthDictation
  const type = (t) => { ta.value = t; ta.dispatchEvent(new Event('input')) }
  if (how === 'spoken') { d.start(); d.words(['olá', 'tudo', 'bem'], []); d.stop() }
  if (how === 'typed') type('typed words')
  if (how === 'typed after spoken') { d.start(); d.words(['olá'], []); d.stop(); type(''); type('typed words') }
  if (how === 'spoken then cancelled') { d.start(); d.words(['olá'], []); d.cancel(); type('typed words') }
  if (how === 'typed then spoken') { type('a typed start'); d.start(); d.words(['and', 'more'], []); d.stop() }
  // Send while it still listens: the bar's capture-phase submit ends the listening first; the engine's 'cancelled'
  // that answers it comes after the turn went
  if (how === 'sent while listening') { d.start(); d.words(['lembre', 'que'], ['eu']) }
  document.getElementById('say').requestSubmit()
  window.__rthReply({ done: true })
  return window.__sent.at(-1)
}, how)

test('a spoken turn is say_spoken, a typed one is say', async () => {
  const browser = await engine.launch()
  try {
    const { p, errors } = await chat(browser)
    assert.deepEqual(await turn(p, 'spoken'), { action: 'say_spoken', text: 'olá tudo bem' })
    assert.deepEqual(await turn(p, 'typed'), { action: 'say', text: 'typed words' }, 'the flag ends with the turn')
    assert.deepEqual(await turn(p, 'typed after spoken'), { action: 'say', text: 'typed words' }, 'an emptied field is no longer spoken')
    assert.deepEqual(await turn(p, 'spoken then cancelled'), { action: 'say', text: 'typed words' }, 'Esc drops what was dictated')
    assert.deepEqual(await turn(p, 'typed then spoken'), { action: 'say_spoken', text: 'a typed start and more' }, 'dictated words make it spoken')
    assert.deepEqual(await turn(p, 'sent while listening'), { action: 'say_spoken', text: 'lembre que eu' }, 'Send while listening is still a spoken turn')
    await p.evaluate(() => window.__rthDictationEvent({ state: 'cancelled' }))
    assert.deepEqual(await turn(p, 'typed'), { action: 'say', text: 'typed words' }, 'the late cancelled changes nothing after')
    assert.deepEqual(errors, [])
  } finally {
    await browser.close()
  }
})
