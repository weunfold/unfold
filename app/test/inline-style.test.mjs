// What the app's CSP blocks, kept out of its pages. It gives each <style> and <script> a nonce, so the webview ignores
// 'unsafe-inline' and drops every style="" attribute, in the markup and in the HTML the scripts build: layout lives in
// the stylesheet, and a value the script works out (a bar's width, a swatch's colour) is set through the CSSOM. Its
// default-src 'self' also blocks data: images, so a stylesheet's image is a file beside the page.
//   node --test test/inline-style.test.mjs
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const UI = resolve(fileURLToPath(import.meta.url), '../../ui')

for (const page of ['settings.html', 'index.html']) {
  test(`${page} has no style attribute`, () => {
    const lines = readFileSync(resolve(UI, page), 'utf8').split('\n')
    const hits = lines.flatMap((l, i) => (/\sstyle\s*=\s*["'`$]|setAttribute\(\s*['"`]style/.test(l) ? [`${page}:${i + 1}: ${l.trim().slice(0, 120)}`] : []))
    assert.deepEqual(hits, [])
  })
}

for (const page of ['settings.html', 'index.html']) {
  test(`${page} has no data: image`, () => {
    const lines = readFileSync(resolve(UI, page), 'utf8').split('\n')
    const hits = lines.flatMap((l, i) => (/url\(\s*["']?data:/.test(l) ? [`${page}:${i + 1}: ${l.trim().slice(0, 120)}`] : []))
    assert.deepEqual(hits, [])
  })
}

// The classes that moved the attributes' layout into the stylesheet set display with !important; .hidden, at the same
// weight, has to come after every one of them, or class="btn-row hidden" shows.
for (const page of ['settings.html', 'index.html']) {
  test(`${page}: .hidden comes after every class that sets display`, () => {
    const css = (readFileSync(resolve(UI, page), 'utf8').match(/<style>([\s\S]*?)<\/style>/) || [, ''])[1]
    const rules = [...css.matchAll(/(^|\n)\s*\.([\w-]+)\s*\{([^}]*)\}/g)].map((m) => ({ cls: m[2], body: m[3], at: m.index }))
    const hidden = rules.filter((r) => r.cls === 'hidden').map((r) => r.at)
    if (!hidden.length) return
    const later = rules.filter((r) => r.cls !== 'hidden' && /display\s*:[^;]*!important/.test(r.body) && r.at > Math.max(...hidden)).map((r) => '.' + r.cls)
    assert.deepEqual(later, [])
  })
}
