import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'
const { chromium } = await import(process.env.PLAYWRIGHT_CORE)
const browser = await chromium.launch({headless:true, ...(process.env.CHROME_HEADLESS_SHELL ? {executablePath:process.env.CHROME_HEADLESS_SHELL} : {})})
test.after(() => browser.close())
async function page() {
 const p=await browser.newPage()
 await p.addInitScript(() => {
  window.calls=[]
  window.__TAURI_INTERNALS__={invoke:(cmd,args)=>{window.calls.push({cmd,...args});return args.action==='conversation_send' ? new Promise((resolve,reject)=>{window.accept=resolve;window.refuse=reject}) : Promise.resolve()}}
 })
 await p.goto(`file://${resolve(fileURLToPath(import.meta.url),'../../ui/index.html')}#splash=off`)
 await p.evaluate(()=>{
  window.__rthSetup({screen:'home',chat:'ready',admitted:true,models:[]})
  window.__rthDictationEvent({state:'available',ready:true,conversation:true})
  window.__rthConversation({state:'listening'})
  window.__rthDictationEvent({seq:1,text:'Current spoken words',is_final:false})
 })
 return p
}
test('Conversation Send claims the current text once without stopping or cancelling capture',async()=>{
 const p=await page()
 try {
  await p.locator('#say-go').click()
  await p.evaluate(()=>document.getElementById('say').requestSubmit())
  assert.deepEqual(await p.evaluate(()=>window.calls.filter(c=>c.action==='conversation_send')),[{cmd:'rth_dictation',action:'conversation_send',choice:'Current spoken words'}])
  assert.deepEqual(await p.evaluate(()=>window.calls.filter(c=>['stop','cancel','say','say_spoken'].includes(c.action))),[])
  await p.evaluate(()=>{window.__rthConversation({state:'thinking'});window.accept()})
  assert.equal(await p.locator('#say-text').inputValue(),'')
 } finally {await p.close()}
})
test('a refused Conversation Send preserves its words and permits retry',async()=>{
 const p=await page()
 try {
  await p.locator('#say-go').click()
  await p.evaluate(()=>window.refuse('Wait for this reply to finish.'))
  assert.equal(await p.locator('#say-text').inputValue(),'Current spoken words')
  assert.match(await p.locator('.dict-note').innerText(),/Wait for this reply/)
  await p.locator('#say-go').click()
  assert.equal(await p.evaluate(()=>window.calls.filter(c=>c.action==='conversation_send').length),2)
 } finally {await p.close()}
})
test('a late Send acknowledgement cannot erase the draft restored on mode exit',async()=>{
 const p=await page()
 try {
  await p.evaluate(()=>{
   window.__rthConversation({state:'off'})
   const ta=document.getElementById('say-text');ta.value='Current spoken words';ta.dispatchEvent(new Event('input'))
   window.__rthConversation({state:'listening'})
   window.__rthDictationEvent({seq:1,text:'Current spoken words',is_final:false})
  })
  await p.locator('#say-go').click()
  await p.evaluate(()=>{window.__rthConversation({state:'off'});window.accept()})
  assert.equal(await p.locator('#say-text').inputValue(),'Current spoken words')
 } finally {await p.close()}
})
