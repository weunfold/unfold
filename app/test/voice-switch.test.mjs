// Settings › Voice's top row is a switch, the one on/off for spoken replies, headless on the page's own sample
// (#voice=switch-off|switch-on|unreachable): its words beside it, what a click sends, and Never speak turning it off.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> [BROWSER=webkit] node --test test/voice-switch.test.mjs
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const PW = process.env.PLAYWRIGHT_CORE
if (!PW) { console.error('PLAYWRIGHT_CORE is unset: set it to the path of playwright-core\'s index.mjs'); process.exit(1) }
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/settings.html')}`
const engines = await import(PW)
const engine = engines[process.env.BROWSER || 'chromium']

async function pane(browser, state, lang) {
  const p = await browser.newPage({ viewport: { width: 900, height: 900 } })
  const errors = []
  p.on('pageerror', (e) => errors.push(e.message))
  await p.goto(`${PAGE}#pane=voice&voice=${state}&lang=${lang}`)
  await p.waitForSelector('#v-switch')
  return { p, errors }
}
const row = (p) => p.evaluate(() => {
  const s = document.getElementById('v-switch')
  return { label: document.getElementById('v-switch-l').textContent, sub: s.closest('.row').querySelector('.sub').textContent, on: s.getAttribute('aria-checked'), mode: document.getElementById('say-mode').value }
})
const sent = (p) => p.evaluate(() => window.__voiceDemo().calls.filter((c) => c.action === 'voice_on' || c.action === 'say_set'))

test('the switch says Off, turns on to the voice and its mood, and Never speak turns it off', async () => {
  const browser = await engine.launch()
  try {
    for (const [lang, w] of [['en', { voice: 'Voice', off: 'Off', on: 'Unfold (pt-BR) · Neutral' }], ['pt-BR', { voice: 'Voz', off: 'Desligada', on: 'Unfold (pt-BR) · Natural' }]]) {
      const { p, errors } = await pane(browser, 'switch-off', lang)
      assert.deepEqual(await row(p), { label: w.voice, sub: w.off, on: 'false', mode: 'full' }, lang)
      // Never speak while off, then the switch: on, and the short summary
      await p.selectOption('#say-mode', 'off')
      await p.click('#v-switch')
      assert.deepEqual(await row(p), { label: w.voice, sub: w.on, on: 'true', mode: 'summary' }, lang)
      // Never speak while on turns it off
      await p.selectOption('#say-mode', 'off')
      assert.deepEqual(await row(p), { label: w.voice, sub: w.off, on: 'false', mode: 'off' }, lang)
      assert.deepEqual(await sent(p), [{ action: 'say_set', value: { mode: 'off' } }, { action: 'voice_on', value: { on: true } }, { action: 'say_set', value: { mode: 'off' } }])
      assert.deepEqual(errors, [])
      await p.close()
    }
  } finally {
    await browser.close()
  }
})

test('choosing a voice while off leaves the switch off', async () => {
  const browser = await engine.launch()
  try {
    const { p, errors } = await pane(browser, 'switch-off', 'en')
    await p.click('[data-fpick="lf-2"]')
    assert.deepEqual(await row(p), { label: 'Voice', sub: 'Off', on: 'false', mode: 'full' })
    assert.deepEqual(await sent(p), [])
    assert.deepEqual(errors, [])
  } finally {
    await browser.close()
  }
})

test('with LocalFleet out of reach the switch stays as set and says so', async () => {
  const browser = await engine.launch()
  try {
    for (const [lang, sub] of [['en', 'LocalFleet isn’t reachable'], ['pt-BR', 'O LocalFleet não está acessível']]) {
      const { p, errors } = await pane(browser, 'unreachable', lang)
      assert.deepEqual(await row(p), { label: lang === 'en' ? 'Voice' : 'Voz', sub, on: 'false', mode: 'full' }, lang)
      await p.click('#v-switch')
      assert.deepEqual(await row(p), { label: lang === 'en' ? 'Voice' : 'Voz', sub, on: 'true', mode: 'full' }, lang)
      assert.deepEqual(errors, [])
      await p.close()
    }
  } finally {
    await browser.close()
  }
})

test('on with no voice listed, the row asks for one', async () => {
  const browser = await engine.launch()
  try {
    for (const [lang, w] of [['en', { voice: 'Voice', sub: 'Choose a voice' }], ['pt-BR', { voice: 'Voz', sub: 'Escolher uma voz' }]]) {
      const { p, errors } = await pane(browser, 'switch-none', lang)
      assert.deepEqual(await row(p), { label: w.voice, sub: w.sub, on: 'true', mode: 'full' }, lang)
      assert.deepEqual(errors, [])
      await p.close()
    }
  } finally {
    await browser.close()
  }
})

test('the footer under Spoken replies says when Unfold speaks', async () => {
  const browser = await engine.launch()
  try {
    for (const [lang, foot] of [
      ['en', 'Unfold speaks when you talk to it by voice, or when you ask it to. Code, links and tables stay on screen: when a reply’s first sentence has one, Unfold says “It’s on screen.” instead.'],
      ['pt-BR', 'Unfold fala quando você conversa por voz ou quando pede. Código, links e tabelas ficam na tela: quando a primeira frase da resposta tem algum desses, Unfold diz “Deixei na tela.”'],
    ]) {
      const { p, errors } = await pane(browser, 'switch-on', lang)
      const after = await p.evaluate(() => document.getElementById('say-mode').closest('.group').nextElementSibling.textContent)
      assert.equal(after, foot, lang)
      assert.deepEqual(errors, [])
      await p.close()
    }
  } finally {
    await browser.close()
  }
})
