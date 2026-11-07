import {test} from 'node:test'
import assert from 'node:assert/strict'
import {resolve} from 'node:path'
const {chromium} = await import(process.env.PLAYWRIGHT_CORE)
const browser = await chromium.launch({headless:true})
test.after(() => browser.close())
for (const lang of ['en', 'pt-BR']) {
  test(`timer is default, shadow is on, model endpoint is opt-in: ${lang}`, async () => {
    const p = await browser.newPage()
    try {
      await p.goto(`file://${resolve(process.env.LISTENING_UI || 'app/ui', 'settings.html')}#pane=dictation&lang=${lang}`)
      assert.equal(await p.locator('#smart-turn-endpoint').getAttribute('aria-checked'), 'false')
      assert.equal(await p.locator('#smart-turn-shadow').getAttribute('aria-checked'), 'true')
      await p.locator('#smart-turn-endpoint').click()
      await p.waitForFunction(() => document.getElementById('smart-turn-endpoint').getAttribute('aria-checked') === 'true')
      assert.equal(await p.locator('#smart-turn-shadow').isDisabled(), true)
      await p.locator('#smart-turn-endpoint').click()
      await p.waitForFunction(() => document.getElementById('smart-turn-endpoint').getAttribute('aria-checked') === 'false')
      assert.equal(await p.locator('#smart-turn-shadow').getAttribute('aria-checked'), 'true')
      await p.locator('#smart-turn-shadow').click()
      await p.waitForFunction(() => document.getElementById('smart-turn-shadow').getAttribute('aria-checked') === 'false')
    } finally { await p.close() }
  })
}
