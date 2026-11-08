import {test} from 'node:test'
import assert from 'node:assert/strict'
import {resolve} from 'node:path'
const {chromium}=await import(process.env.PLAYWRIGHT_CORE)
const browser=await chromium.launch({headless:true})
test.after(()=>browser.close())
for (const lang of ['en', 'pt-BR']) {
 test(`retired floor model switch is absent: ${lang}`, async () => {
  const p = await browser.newPage()
  try {
   await p.goto(`file://${resolve(process.env.LISTENING_UI || 'app/ui','settings.html')}#pane=voice&voice=switch-on&lang=${lang}`)
   assert.equal(await p.locator('#floor-model').count(), 0)
   assert.equal(await p.getByText(/Smarter turn-taking|Alternância de fala inteligente/).count(), 0)
  } finally { await p.close() }
 })
}
