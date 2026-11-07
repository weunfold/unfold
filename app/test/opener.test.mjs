// A turn the person spoke goes to the app as 'say_spoken' (its reply is said in the chosen voice), a typed one as
// 'say', headless. No microphone: the bar's dictation is driven through window.__rthDictation as the engine would.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> [BROWSER=webkit] node --test test/opener.test.mjs
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

test('opener and reply share one bubble, including replacement and old agentd', async () => {
 const browser = await engine.launch()
 try {
  const {p,errors} = await chat(browser)
  const text = await p.evaluate(() => {
   const ta = document.getElementById('say-text'); ta.value='question';
   document.getElementById('say').requestSubmit()
   window.__rthReply({opener:'Let me think.',response_id:'r1'})
   window.__rthReply({delta:'The answer is four.',response_id:'r1'})
   const before=document.querySelector('.it:last-child').textContent
   window.__rthReply({replace:'Four.',response_id:'r1'})
   const after=document.querySelector('.it:last-child').textContent
   window.__rthReply({done:true,response_id:'r1'})
   ta.value='old agentd'; document.getElementById('say').requestSubmit()
   window.__rthReply({delta:'Hello.',response_id:'r2'}); window.__rthReply({done:true,response_id:'r2'})
   return {before,after,old:document.querySelector('.it:last-child').textContent}
  })
  assert.equal(text.before,'Let me think. The answer is four.')
  assert.equal(text.after,'Let me think. Four.')
  assert.equal(text.old,'Hello.')
  assert.deepEqual(errors,[])
 } finally {await browser.close()}
})
