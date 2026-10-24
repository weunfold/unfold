// The DSH entry: packaging drops node_modules/.bin, so the shell resolves @deepseek-ai/dsh through
// its package.json "bin" field, with the same result as the dev .bin link. Temporary fixtures only.
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { resolveDshBin } from '../dsh-bin.mjs'

const SHELL = join(dirname(fileURLToPath(import.meta.url)), '..')

test('dev-equivalent: the bin field names the same file the .bin/dsh link does', () => {
  assert.equal(resolveDshBin(SHELL), realpathSync(join(SHELL, 'node_modules/.bin/dsh')))
})

test('a packaged tree without node_modules/.bin still resolves DSH from its package.json bin field', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'rth-dsh-bin-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const pkg = join(root, 'node_modules/@deepseek-ai/dsh')
  mkdirSync(join(pkg, 'lib'), { recursive: true })
  writeFileSync(join(root, 'package.json'), '{"name":"fixture-shell","private":true}')
  writeFileSync(join(pkg, 'package.json'), JSON.stringify({ name: '@deepseek-ai/dsh', version: '0.0.0', bin: { dsh: 'lib/bin.js' }, exports: { './package.json': './package.json' } }))
  writeFileSync(join(pkg, 'lib/bin.js'), '// fixture\n')
  assert.equal(resolveDshBin(root), realpathSync(join(pkg, 'lib/bin.js')))
})

test('a missing DSH package is a clear error, not a guess', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'rth-dsh-bin-none-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  writeFileSync(join(root, 'package.json'), '{"name":"fixture-shell","private":true}')
  assert.throws(() => resolveDshBin(root), /@deepseek-ai\/dsh/)
})

test('app.mjs starts DSH through the bin-field resolver, never through node_modules/.bin', () => {
  const app = readFileSync(join(SHELL, 'app.mjs'), 'utf8')
  assert.ok(!app.includes('node_modules/.bin'), 'no .bin path in app.mjs')
  assert.match(app, /resolveDshBin\(SHELL\)/)
})
