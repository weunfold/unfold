import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'
const { chromium } = await import(process.env.PLAYWRIGHT_CORE)
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/index.html')}#splash=off`
const browser = await chromium.launch({ headless: true, ...(process.env.CHROME_HEADLESS_SHELL ? { executablePath: process.env.CHROME_HEADLESS_SHELL } : {}) })
test.after(() => browser.close())
async function page() {
 const p = await browser.newPage()
 await p.addInitScript(() => {
  window.calls = []
  window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => { window.calls.push({cmd, ...args}); return Promise.resolve() } }
 })
 await p.goto(PAGE)
 await p.evaluate(() => {
  window.__rthSetup({screen:'home',chat:'ready',admitted:true,models:[]})
  window.__rthDictationEvent({state:'available',ready:true,conversation:true})
  window.__rthDictationEvent({state:'listening'})
  window.__rthDictationEvent({seq:1,text:'Hello on Fold it going',is_final:false})
 })
 return p
}
const sends = p => p.evaluate(() => window.calls.filter(c => c.cmd === 'rth_setup' && /^say/.test(c.action)))
const actions = p => p.evaluate(() => window.calls.filter(c => c.cmd === 'rth_dictation').map(c => c.action))
const submit = p => p.evaluate(() => document.getElementById('say').requestSubmit())
const final = p => p.evaluate(() => {
 window.__rthDictationEvent({seq:2,text:"Hello Unfold, how's it going?",is_final:true})
 window.__rthDictationEvent({state:'stopped'})
})
test('Send while dictating waits for the final decode and submits it once', async () => {
 const p = await page()
 try {
  await submit(p); await submit(p)
  assert.deepEqual(await sends(p), [], 'never submit the partial draft')
  assert.deepEqual(await actions(p), ['stop'], 'one flush, no cancel')
  await p.evaluate(() => window.__rthDictationEvent({seq:2,text:"Hello Unfold, how's it going?",is_final:true}))
  assert.deepEqual(await sends(p), [], 'final text arrives before stop acknowledgement')
  await p.evaluate(() => window.__rthDictationEvent({state:'stopped'}))
  assert.deepEqual(await sends(p), [{cmd:'rth_setup',action:'say_spoken',text:"Hello Unfold, how's it going?"}])
  await final(p)
  assert.equal((await sends(p)).length, 1, 'late duplicate acknowledgements submit nothing')
 } finally {await p.close()}
})
test('Send during a cold model load waits through finishing and uses the late final', async () => {
 const p = await page()
 try {
  await submit(p)
  await p.evaluate(() => window.__rthDictationEvent({state:'finishing'}))
  assert.deepEqual(await sends(p), [])
  await final(p)
  assert.equal((await sends(p))[0]?.text, "Hello Unfold, how's it going?")
 } finally {await p.close()}
})
for (const cancel of ['Escape','conversation mode']) test(`${cancel} while Send is flushing discards the pending submit`, async () => {
 const p = await page()
 try {
  await submit(p)
  if (cancel === 'Escape') await p.keyboard.press('Escape')
  else await p.evaluate(() => window.__rthConversation({state:'listening'}))
  await final(p)
  assert.deepEqual(await sends(p), [])
  if (cancel === 'Escape') assert.equal(await p.locator('#say-text').inputValue(), '')
 } finally {await p.close()}
})
test('a failed finalization does not send the partial and leaves an editable draft', async () => {
 const p = await page()
 try {
  await submit(p)
  await p.evaluate(() => window.__rthDictationEvent({state:'error',message:'Try dictation again.'}))
  assert.deepEqual(await sends(p), [])
  assert.equal(await p.locator('#say-text').inputValue(), 'Hello on Fold it going')
  assert.equal(await p.locator('#say-text').getAttribute('readonly'), null)
 } finally {await p.close()}
})
test('editing a dictated draft stops and decodes instead of cancelling its audio', async () => {
 const p = await page()
 try {
  await p.locator('#say-text').focus()
  await p.keyboard.type('!')
  assert.deepEqual(await actions(p), ['stop'])
  await final(p)
  assert.deepEqual(await sends(p), [], 'editing never auto-submits')
  assert.match(await p.locator('#say-text').inputValue(), /!/, 'late decode cannot overwrite the edit')
 } finally {await p.close()}
})
test('conversation endpoints and barge-in never submit a JS partial', async () => {
 const p = await page()
 try {
  await p.evaluate(() => {
   window.__rthConversation({state:'listening'})
   window.__rthDictationEvent({seq:1,text:'wrong partial',is_final:false})
   document.getElementById('say').requestSubmit()
   window.__rthConversation({state:'speaking'})
   window.__rthConversation({state:'interrupted'})
   window.__rthDictationEvent({seq:2,text:'correct final',is_final:true})
  })
  assert.deepEqual(await sends(p), [], 'native finalized events own the conversation turn')
 } finally {await p.close()}
})
test('Send after the manual Stop ceiling still waits for final text', async () => {
 const p = await page()
 try {
  await p.clock.install()
  await p.locator('.rec-stop').click()
  await p.clock.runFor(5100)
  await submit(p)
  assert.deepEqual(await sends(p), [])
  await final(p)
  assert.equal((await sends(p))[0]?.text, "Hello Unfold, how's it going?")
 } finally {await p.close()}
})
test('a rejected Stop request never submits the partial draft', async () => {
 const p = await page()
 try {
  await p.evaluate(() => { window.__TAURI_INTERNALS__.invoke = (cmd, args) => {
   window.calls.push({cmd,...args}); return args?.action === 'stop' ? Promise.reject('Stop unavailable') : Promise.resolve()
  } })
  await submit(p)
  assert.deepEqual(await sends(p), [])
  assert.equal(await p.locator('#say-text').getAttribute('readonly'), null)
 } finally {await p.close()}
})
test('clicking Conversation while Send waits prevents automatic submission', async () => {
 const p = await page()
 try {
  await submit(p)
  await p.locator('#say-conversation').click()
  await final(p)
  assert.deepEqual(await sends(p), [])
 } finally {await p.close()}
})
test('Escape during a cold finishing wait discards the dictated draft', async () => {
 const p = await page()
 try {
  await p.evaluate(() => window.__rthDictationEvent({state:'finishing'}))
  await submit(p)
  await p.keyboard.press('Escape')
  await final(p)
  assert.deepEqual(await sends(p), [])
  assert.equal(await p.locator('#say-text').inputValue(), '')
 } finally {await p.close()}
})
test('one edit then Enter before stopped sends the edited field once without late overwrite', async () => {
 const p = await page()
 try {
  await p.locator('#say-text').focus()
  await p.keyboard.press('End')
  await p.keyboard.insertText('?')
  const edited = await p.locator('#say-text').inputValue()
  assert.match(edited, /\?$/)
  await p.keyboard.press('Enter')
  assert.equal((await sends(p)).length, 1, 'the edited field submits immediately')
  assert.equal((await sends(p))[0].text, edited)
  const afterSend = await p.locator('#say-text').inputValue()
  await final(p)
  assert.equal((await sends(p)).length, 1)
  assert.equal((await sends(p))[0].text, edited)
  assert.equal(await p.locator('#say-text').inputValue(), afterSend, 'late final cannot overwrite the field')
 } finally {await p.close()}
})
test('Escape after one edit preserves the edited draft during the late-final window', async () => {
 const p = await page()
 try {
  await p.locator('#say-text').focus()
  await p.keyboard.press('End')
  await p.keyboard.insertText('?')
  const edited = await p.locator('#say-text').inputValue()
  await p.keyboard.press('Escape')
  await final(p)
  assert.deepEqual(await sends(p), [])
  assert.equal(await p.locator('#say-text').inputValue(), edited)
 } finally {await p.close()}
})
