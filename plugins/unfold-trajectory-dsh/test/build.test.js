// The fork's build contract: the pinned upstream
// bundle, every patch applied exactly as counted, the MIT notice shipped in the bundle, and no
// upstream id, dictionary, storage name or CSS-module class left that could collide with upstream.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = join(HERE, '..')
const sha = (b) => createHash('sha256').update(b).digest('hex')
const UPSTREAM_FULL = '391740afdfde2e8db406b0ac22d02375e17772ff1dff4676d1b91efb591c6c96'

const dir = mkdtempSync(join(tmpdir(), 'unfold-traj-build-'))
const out = join(dir, 'client.js')
execFileSync(process.execPath, [join(ROOT, 'scripts/build-client.mjs'), '--out', out], { stdio: 'pipe' })
const built = readFileSync(out, 'utf8')
rmSync(dir, { recursive: true, force: true })
const body = built.slice(built.indexOf('*/') + 2)

test('upstream bundle is the pinned 0.2.1-rc.3 client (the same bytes as rc.2) and its license is the MIT text', () => {
  assert.equal(sha(readFileSync(join(ROOT, 'upstream/lib/client.js'))), UPSTREAM_FULL)
  const up = JSON.parse(readFileSync(join(ROOT, 'upstream/package.json'), 'utf8'))
  assert.equal(up.name, '@deepseek-ai/dsh-client-ui-trajectory')
  assert.equal(up.version, '0.2.1-rc.3')
  const lic = readFileSync(join(ROOT, 'LICENSE'), 'utf8')
  assert.equal(lic, readFileSync(join(ROOT, 'upstream/LICENSE'), 'utf8'))
  assert.match(lic, /MIT License/)
  assert.match(lic, /Copyright \(c\) 2026 DeepSeek/)
})

test('the built bundle carries the license notice, the upstream pin and the icon attribution', () => {
  const header = built.slice(0, built.indexOf('*/') + 2)
  assert.ok(header.startsWith('/*!'), 'a preserved (/*!) comment')
  assert.match(header, /Copyright \(c\) 2026 DeepSeek/)
  assert.match(header, /Permission is hereby granted, free of charge/)
  assert.ok(header.includes(UPSTREAM_FULL))
  assert.match(header, /NOTICE\.md/)
  assert.match(header, /Lucide/)
  assert.match(header, /ISC/)
})

test('committed client.js is exactly what build.mjs produces from these sources', () => {
  assert.equal(sha(readFileSync(join(ROOT, 'client.js'), 'utf8')), sha(built))
})

test('no upstream identity survives in the code: loader id, dictionary, storage names, CSS-module classes', () => {
  assert.ok(!body.includes('@deepseek-ai/dsh-client-ui-trajectory'), 'own loader/style/dataset ids')
  assert.ok(!body.includes('const NS = "trajectory"'), 'own dictionary namespace')
  assert.ok(body.includes('const NS = "unfold-trajectory"'))
  assert.ok(!/name: "dsh\.trajectory\./.test(body), 'own persisted-preference names')
  const upstream = readFileSync(join(ROOT, 'upstream/lib/client.js'), 'utf8')
  const prefixes = new Set()
  for (const m of upstream.matchAll(/_module_css_default = \{([\s\S]*?)\n\t\t\};/g)) {
    for (const e of m[1].matchAll(/"([\w-]+)": "([\w-]+)"/g)) if (e[2].endsWith(e[1])) prefixes.add(e[2].slice(0, -e[1].length))
  }
  assert.equal(prefixes.size, 4)
  for (const p of prefixes) {
    const bare = p.replace(/^_/, '')
    assert.ok(!new RegExp(`(?<!uft)${bare}`).test(body), `upstream CSS-module prefix ${p} survives only as uft${bare}`)
    assert.ok(body.includes(`uft${bare}`))
  }
  assert.ok(!body.includes('__UNFOLD_'), 'every placeholder was filled')
})

test('every documented patch is listed in NOTICE.md', async () => {
  const notice = readFileSync(join(ROOT, 'NOTICE.md'), 'utf8')
  const src = readFileSync(join(ROOT, 'scripts/build-client.mjs'), 'utf8')
  const whys = [...src.matchAll(/^\s+\['[^']*', '((?:[^'\\]|\\.)*)'/gm)].map((m) => m[1].replace(/\\'/g, "'"))
  assert.ok(whys.length >= 18, `found ${whys.length} patches`)
  for (const why of whys) assert.ok(notice.includes(why), `NOTICE.md lists: ${why}`)
})
