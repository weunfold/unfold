// Live dictation in the chat bar, headless: settled words solid, the tail quiet, and a corrected span that
// dissolves into dust while its replacement lands in its place. The page's clock is paused (Playwright's clock:
// timers, requestAnimationFrame and performance.now), so every frame is reproducible. No microphone: the engine's
// events are fed as the app would send them.
//   PLAYWRIGHT_CORE=<playwright-core's index.mjs> [CHROME_HEADLESS_SHELL=<a headless shell>] node --test test/dictation-live.test.mjs
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const PW = process.env.PLAYWRIGHT_CORE
if (!PW) { console.error('PLAYWRIGHT_CORE is unset: set it to the path of playwright-core\'s index.mjs'); process.exit(1) }
const SHELL = process.env.CHROME_HEADLESS_SHELL
const PAGE = `file://${resolve(fileURLToPath(import.meta.url), '../../ui/index.html')}#splash=off`
const { chromium } = await import(PW)

// A recorded-shape hypothesis sequence (the engine's {seq, text, settled, tail, is_final}): one correction inside a
// word (governo → governador), one whole-word correction (dezenove → 19), one word taken back (dos dos → dos),
// growth at the end that is not a correction, and the final.
const ev = (seq, settled, tail, is_final = false) => ({ seq, settled, tail, is_final, text: [settled, tail].filter(Boolean).join(' ') })
const SEQUENCE = [
  ev(1, '', 'o gabinete do governo'),
  ev(2, 'o gabinete', 'do governador'),
  ev(3, 'o gabinete do governador', 'afirmou que'),
  ev(4, 'o gabinete do governador', 'afirmou que dezenove'),
  ev(5, 'o gabinete do governador afirmou', 'que 19 dos dos'),
  ev(6, 'o gabinete do governador afirmou', 'que 19 dos'),
  ev(7, 'o gabinete do governador afirmou que 19 dos feridos', '', true),
]
const FINAL = 'o gabinete do governador afirmou que 19 dos feridos'

async function page(browser, { reduced = false } = {}) {
  const p = await browser.newPage({ viewport: { width: 900, height: 700 } })
  const errors = []
  p.on('pageerror', (e) => errors.push(e.message))
  await p.emulateMedia({ reducedMotion: reduced ? 'reduce' : 'no-preference' })
  // a paused clock: no time passes but what a test runs (install alone lets it flow with real time)
  await p.clock.install({ time: 0 })
  await p.clock.pauseAt(1000)
  await p.goto(PAGE)
  await p.clock.runFor(2000)
  await p.evaluate(() => {
    window.__rthDictationAnims = []
    // every dust canvas the bar ever shows, and whether any grain was drawn on it
    window.__dust = { made: 0, drawn: 0 }
    new MutationObserver((ms) => { for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains('rec-dust')) window.__dust.made++ }).observe(document.body, { childList: true, subtree: true })
    window.__rthDictationEvent({ state: 'listening' })
  })
  return { p, errors }
}

// The twin's word spans: text, class and box, in order.
const words = (p) => p.evaluate(() => [...document.querySelector('.chatbar .rec-text').children].filter((s) => s.tagName === 'SPAN' && !s.classList.contains('wait')).map((s) => {
  const r = s.getBoundingClientRect()
  return { text: s.textContent, quiet: s.classList.contains('quiet'), box: [r.left, r.top, r.width, r.height].map((v) => Math.round(v * 10) / 10) }
}))
const send = (p, e) => p.evaluate((e) => window.__rthDictationEvent(e), e)
const dustPixels = (p) => p.evaluate(() => {
  const c = document.querySelector('.chatbar .rec-dust')
  if (!c || !c.width || !c.height) return 0
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data
  let n = 0
  for (let i = 3; i < d.length; i += 4) if (d[i] > 0) n++
  return n
})

const browser = await chromium.launch(SHELL ? { executablePath: SHELL } : {})
test.after(() => browser.close())

test('settled words are solid, the tail is quiet', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser)
  await send(p, SEQUENCE[0])
  await send(p, SEQUENCE[1])
  const w = await words(p)
  assert.deepEqual(w.map((x) => [x.text, x.quiet]), [['o', false], ['gabinete', false], ['do', true], ['governador', true]])
  assert.deepEqual(errors, [])
  await p.close()
})

test('only the changed span of a corrected word animates, settled words never, and the final text is exact', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser)
  const seen = []
  let settled = 0  // words settled before this event: they never change, so they never animate
  for (const e of SEQUENCE) {
    const before = await words(p)
    await send(p, e)
    const anims = await p.evaluate(() => window.__rthDictationAnims.splice(0))
    const after = await words(p)
    seen.push(...anims.map((a) => ({ word: a.word, from: a.from, to: a.to, mode: a.mode })))
    for (const a of anims) assert.ok(a.word >= settled, `seq ${e.seq}: a settled word never animates (${a.word} < ${settled})`)
    settled = e.settled ? e.settled.split(' ').length : 0
    // every word before the first animated one keeps its box while the animation runs
    const first = anims.length ? Math.min(...anims.map((a) => a.word)) : after.length
    for (let i = 0; i < Math.min(first, before.length); i++) assert.deepEqual(after[i].box, before[i].box, `seq ${e.seq}: word ${i} moved`)
    if (anims.length) {
      await p.clock.runFor(60)
      assert.ok(await dustPixels(p) > 0, `seq ${e.seq}: grains are drawn mid-dissolve`)
    }
    await p.clock.runFor(1000)
  }
  assert.deepEqual(seen, [
    { word: 3, from: 'o', to: 'ador', mode: 'dust' },
    { word: 6, from: 'dezenove', to: '19', mode: 'dust' },
    { word: 8, from: 'dos', to: '', mode: 'dust' },
  ])
  const field = await p.evaluate(() => document.getElementById('say-text').value)
  const twin = await p.evaluate(() => document.querySelector('.chatbar .rec-text').textContent)
  assert.equal(field, FINAL)
  assert.equal(twin, FINAL)
  assert.equal(await p.evaluate(() => document.querySelectorAll('.chatbar .rec-text [style*="width"]').length), 0, 'no held widths after the last frame')
  assert.equal(await dustPixels(p), 0, 'no grains left after the last frame')
  assert.deepEqual(errors, [])
  await p.close()
})

// Words after a correction hold still while it runs (the corrected span keeps the old one's box), whether a word
// is corrected, taken back or added in the middle, or the first word goes; they reflow only after the last frame.
test('the words after a corrected span keep their boxes until its last frame', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser)
  const cases = [
    [ev(1, '', 'o gabinete do governo afirmou que'), ev(2, 'o gabinete', 'do governador afirmou que'), 'swap', 3],
    [ev(3, 'o gabinete', 'do governador que'), null, 'leave', 4],
    [ev(4, 'o gabinete', 'do governador afirmou que'), null, 'arrive', 4],
  ]
  await send(p, cases[0][0])
  await p.clock.runFor(1000)
  for (const [first, second, kind, at] of cases) {
    if (second) { await send(p, first); await p.clock.runFor(1000) }
    const e = second || first
    const before = await words(p)
    const tail = (w) => w.filter((x) => x.text === 'que').map((x) => x.box)
    await send(p, e)
    const anims = await p.evaluate(() => window.__rthDictationAnims.splice(0))
    assert.deepEqual(anims.map((a) => [a.kind, a.word]), [[kind, at]], `seq ${e.seq}`)
    for (const ms of [0, 100, 200]) {
      await p.clock.runFor(ms && 100)
      assert.deepEqual(tail(await words(p)), tail(before), `seq ${e.seq}: "que" moved at ${ms} ms`)
    }
    await p.clock.runFor(1000)
    assert.equal(await p.evaluate(() => document.querySelector('.chatbar .rec-text').textContent), e.text)
  }
  assert.deepEqual(errors, [])
  await p.close()
})

test('the first word taken back leaves its box and the space after it', { timeout: 30000 }, async () => {
  const { p } = await page(browser)
  await send(p, ev(1, '', 'um dois três'))
  const before = await words(p)
  await send(p, ev(2, '', 'dois três'))
  assert.deepEqual((await p.evaluate(() => window.__rthDictationAnims.splice(0))).map((a) => [a.kind, a.word, a.from]), [['leave', 0, 'um']])
  await p.clock.runFor(100)
  const mid = await words(p)
  assert.deepEqual(mid.filter((x) => x.text).map((x) => x.box), before.slice(1).map((x) => x.box), 'dois and três hold still')
  await p.close()
})

test('a word still forming is more heard, not a correction', { timeout: 30000 }, async () => {
  const { p } = await page(browser)
  await send(p, ev(1, '', 'o gabinete do gover'))
  await send(p, ev(2, '', 'o gabinete do governo'))
  await send(p, ev(3, '', 'o gabinete do governo afir'))
  assert.deepEqual(await p.evaluate(() => window.__rthDictationAnims.length), 0)
  await p.close()
})

test('a final that lands mid-dissolve: the last frame shows the final text, no error', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser)
  await send(p, ev(1, '', 'o gabinete do governo'))
  await send(p, ev(2, 'o gabinete', 'do governador'))
  await p.clock.runFor(50)
  await send(p, ev(3, 'o gabinete do governador geral', '', true))
  await send(p, ev(4, '', 'disse'))
  await p.clock.runFor(1000)
  assert.equal(await p.evaluate(() => document.querySelector('.chatbar .rec-text').textContent), 'o gabinete do governador geral disse')
  assert.equal(await p.evaluate(() => document.getElementById('say-text').value), 'o gabinete do governador geral disse')
  assert.deepEqual((await words(p)).map((x) => x.quiet), [false, false, false, false, false, true])
  assert.equal(await dustPixels(p), 0)
  assert.deepEqual(errors, [])
  await p.close()
})

test('the dissolve is deterministic on the paused clock: the same frame twice is the same pixels', { timeout: 30000 }, async () => {
  const shots = []
  for (let run = 0; run < 2; run++) {
    const { p } = await page(browser)
    await send(p, SEQUENCE[0])
    await send(p, SEQUENCE[1])
    await p.clock.runFor(120)
    shots.push(await p.evaluate(async () => {
      const c = document.querySelector('.chatbar .rec-dust'); if (!c) return ''
      const h = new Uint8Array(await crypto.subtle.digest('SHA-256', c.getContext('2d').getImageData(0, 0, c.width, c.height).data))
      return [...h].map((x) => x.toString(16).padStart(2, '0')).join('')
    }))
    await p.close()
  }
  assert.equal(shots[0].length, 64, 'a dust canvas mid-dissolve')
  assert.equal(shots[0], shots[1])
})

test('with reduced motion the changed span cross-fades and no grain is ever drawn', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser, { reduced: true })
  const modes = []
  for (const e of SEQUENCE) {
    await send(p, e)
    modes.push(...(await p.evaluate(() => window.__rthDictationAnims.splice(0))).map((a) => a.mode))
    await p.clock.runFor(30)
    assert.equal(await dustPixels(p), 0)
    await p.clock.runFor(1000)
  }
  assert.deepEqual(modes, ['fade', 'fade', 'fade'])
  assert.equal(await p.evaluate(() => window.__dust.made), 0, 'no dust canvas')
  assert.equal(await p.evaluate(() => document.getElementById('say-text').value), FINAL)
  assert.deepEqual(errors, [])
  await p.close()
})

test('an engine without settle passes keeps the old wake: no animation, the newest words quiet', { timeout: 30000 }, async () => {
  const { p } = await page(browser)
  await send(p, { seq: 1, text: 'olá mundo bonito', is_final: false })
  await send(p, { seq: 2, text: 'olá mundo bonita', is_final: false })
  assert.deepEqual(await p.evaluate(() => window.__rthDictationAnims.length), 0)
  assert.deepEqual((await words(p)).map((x) => x.quiet), [true, true, true])
  await p.close()
})

test('a stop while the model loads leaves listening at once, and the words heard before it land late', { timeout: 30000 }, async () => {
  const { p, errors } = await page(browser)
  const rec = () => p.evaluate(() => !!document.querySelector('[data-rec]'))
  assert.equal(await rec(), true)
  await send(p, { state: 'finishing' })
  assert.equal(await rec(), false, 'still listening after the stop')
  await send(p, { seq: 1, text: 'o que foi dito', is_final: true })
  assert.equal(await p.evaluate(() => document.getElementById('say-text').value), 'o que foi dito')
  await send(p, { state: 'stopped' })
  assert.equal(await p.evaluate(() => document.getElementById('say-text').value), 'o que foi dito')
  assert.deepEqual(errors, [])
  await p.close()
})
