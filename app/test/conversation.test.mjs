import { test } from 'node:test'
import assert from 'node:assert/strict'
import { resolve } from 'node:path'
const { chromium } = await import(process.env.PLAYWRIGHT_CORE)
const page = 'file://' + resolve(process.env.CONVERSATION_APP || 'app/ui', 'index.html') + '#splash=off'
test('Conversation reflects native states, submits no JS turns, barges and exits', async () => {
 const browser = await chromium.launch({ headless: true, ...(process.env.BROWSER_EXECUTABLE ? {executablePath:process.env.BROWSER_EXECUTABLE} : {}) })
 try {
  const p = await browser.newPage()
  await p.addInitScript(() => {
   window.calls = []
   window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => { window.calls.push({ cmd, ...args }); return Promise.resolve() } }
  })
  await p.goto(page)
  await p.evaluate(() => { window.__rthSetup({ screen: 'home', chat: 'ready', admitted: true, models: [] }); window.__rthDictationEvent({state:'available', ready:true, conversation:true}) })
  const b = p.locator('#say-conversation')
  assert.equal(await b.count(), 1, 'chat bar has Conversation beside the mic')
  await b.click()
  assert.deepEqual(await p.evaluate(() => window.calls.at(-1)), {cmd:'rth_dictation',action:'conversation'})
  await p.evaluate(() => window.__rthConversation({state:'listening'}))
  assert.match(await b.innerText(), /Listening/)
  await p.evaluate(() => { window.__rthDictationEvent({state:'listening'}); window.__rthDictationEvent({seq:1,text:'Hello',is_final:false}); window.__rthConversation({state:'thinking',text:'Hello'}) })
  assert.match(await b.innerText(), /Thinking/)
  assert.equal(await p.locator('#say-text').inputValue(), '')
  assert.equal(await p.evaluate(() => window.calls.filter(c => /^say/.test(c.action)).length), 0, 'native endpoint alone submits')
  await p.evaluate(() => window.__rthConversation({state:'interrupted'}))
  assert.match(await b.innerText(), /Interrupted/)
  assert.equal(await b.getAttribute('aria-pressed'), 'true')
  assert.equal(await p.evaluate(() => window.calls.filter(c => /^say/.test(c.action)).length), 0)
  await p.evaluate(() => window.__rthConversation({state:'speaking'}))
  await p.evaluate(() => window.__rthConversation({state:'speaking',overlap_mode:'manual-stop-only'}))
  assert.match(await b.innerText(), /tap to stop/)
  assert.match(await b.getAttribute('title'), /Audio interruption unavailable/)
  await b.click()
  assert.equal(await p.evaluate(() => window.calls.at(-1).action), 'conversation')
  await p.keyboard.press('Escape')
  assert.equal(await p.evaluate(() => window.calls.at(-1).action), 'conversation_stop')
  await p.evaluate(() => { document.documentElement.lang='pt-BR'; window.__rthConversation({state:'listening'}) })
  assert.match(await b.innerText(), /Ouvindo/)
  await p.evaluate(() => window.__rthConversation({state:'off'}))
  assert.match(await b.innerText(), /Conversa/)
 } finally { await browser.close() }
})

test('Conversation preserves an existing draft, gates Send, and drops playback transcripts', async () => {
 const browser = await chromium.launch({headless:true})
 try {
  const p = await browser.newPage()
  await p.addInitScript(() => {
   window.calls = []
   window.__TAURI_INTERNALS__ = {invoke:(cmd,args) => {window.calls.push({cmd,...args}); return Promise.resolve()}}
  })
  await p.goto(page)
  await p.evaluate(() => {
   window.__rthSetup({screen:'home',chat:'ready',admitted:true,models:[]})
   window.__rthDictationEvent({state:'available',ready:true,conversation:true})
   const ta = document.getElementById('say-text'); ta.value='Keep this draft'; ta.dispatchEvent(new Event('input'))
   window.__rthConversation({state:'listening'})
   window.__rthDictationEvent({seq:1,text:'My new words',is_final:false})
  })
  assert.equal(await p.locator('#say-text').inputValue(), 'My new words')
  await p.evaluate(() => {
   document.getElementById('say').dispatchEvent(new Event('submit',{cancelable:true,bubbles:true}))
   window.__rthConversation({state:'speaking'})
   window.__rthDictationEvent({seq:2,text:'Playback echo',is_final:true})
   window.__rthDictationEvent({seq:3,text:'Playback echo',is_final:false})
  })
  assert.equal(await p.locator('#say-text').inputValue(), '')
  assert.equal(await p.evaluate(() => window.calls.filter(c => /^say/.test(c.action)).length), 0)
  await p.evaluate(() => window.__rthConversation({state:'off'}))
  assert.equal(await p.locator('#say-text').inputValue(), 'Keep this draft')
  assert.equal(await p.locator('#say-text').getAttribute('readonly'), null)
 } finally {await browser.close()}
})
