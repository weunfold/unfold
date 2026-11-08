import {test} from 'node:test'
import assert from 'node:assert/strict'
import {resolve} from 'node:path'
const {chromium}=await import(process.env.PLAYWRIGHT_CORE)
const browser=await chromium.launch({headless:true})
test.after(()=>browser.close())
for(const [lang,expected,label] of [['en','low','Listening sounds'],['pt-BR','off','Sons ao ouvir']]) {
 test(`listening sounds default and persisted choice: ${lang}`,async()=>{
  const p=await browser.newPage()
  try {
   await p.goto(`file://${resolve(process.env.LISTENING_UI || 'app/ui','settings.html')}#pane=voice&voice=switch-on&lang=${lang}`)
   assert.equal(await p.locator('#listening-sounds').count(),1)
   assert.equal(await p.locator('label[for="listening-sounds"]').innerText(),label)
   assert.equal(await p.locator('#listening-sounds').inputValue(),expected)
   assert.deepEqual(await p.locator('#listening-sounds option').evaluateAll(xs=>xs.map(x=>x.value)),['off','low','moderate'])
   await p.selectOption('#listening-sounds','moderate')
   assert.equal(await p.locator('#listening-sounds').inputValue(),'moderate')
   assert.deepEqual(await p.evaluate(()=>window.__voiceDemo().calls.filter(x=>x.action==='listening_set')),[{action:'listening_set',value:{level:'moderate'}}])
  } finally {await p.close()}
 })
}
