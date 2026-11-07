// What this package may not carry: a fixed port or a default address (its only data source is
// the Mind stream route on the DSH page's own origin), an environment read, the proof-of-concept
// names it was ported from, or a local path. And the fork's pins: DeepSeek's trajectory UI at
// 0.2.1-rc.3, its MIT notice, and the notice of the BSD-licensed jsdiff that upstream bundles.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const HERE = fileURLToPath(import.meta.url)
const ROOT = join(dirname(HERE), '..')
const sha = (b) => createHash('sha256').update(b).digest('hex')
const read = (file) => readFileSync(join(ROOT, file), 'latin1')

// Every file under dir, relative to root, without node_modules at any depth: CI installs the DSH
// harness in dev/harness/node_modules before the tests run, and those are not this package's files.
function walk(root, dir = '') {
  return readdirSync(join(root, dir)).filter((name) => name !== 'node_modules').flatMap((name) => {
    const path = join(dir, name)
    return statSync(join(root, path)).isDirectory() ? walk(root, path) : [path]
  })
}
// What a DSH install runs: the host half, the browser bundle and the sources it is built from.
const SHIPPED = ['index.js', 'client.js', ...walk(ROOT, 'src')]
// Everything this package keeps in the repository, except this file.
const TRACKED = walk(ROOT).filter((file) => join(ROOT, file) !== HERE)

test('no fixed port and no default address in what ships', () => {
  // The fixed ports of the first Unfold plugins, spelled so that a scan for them finds only shipped code.
  const ports = [77_00, 77_81].join('|')
  const address = new RegExp(`\\b(?:${ports})\\b|127\\.0\\.0\\.1|\\blocalhost\\b|0\\.0\\.0\\.0|\\[::1?\\]|:\\/\\/[^/\\s"'\`]*:\\d+`, 'i')
  for (const file of SHIPPED) {
    const hit = read(file).match(address)
    assert.equal(hit, null, `${file}: ${hit?.[0]}`)
  }
  // The one connection the bundle opens, and nothing else: no fetch, socket or absolute URL.
  const client = read('client.js')
  const calls = [...client.matchAll(/new (?:EventSource|WebSocket|XMLHttpRequest)\(([^)]*\)?)|\bfetch\(([^)]*)/g)].map((m) => m[0])
  assert.deepEqual(calls, ['fetch(new URL(path, document.baseURI', 'new EventSource(new URL(MIND_STREAM, document.baseURI)'])
})

test('the stream is the Mind route on the page\'s own origin, and no environment is read', () => {
  const runtime = read('src/runtime.mjs')
  assert.match(runtime, /new EventSource\(new URL\(MIND_STREAM, document\.baseURI\)\)/)
  assert.match(runtime, /read\('api\/unfold-mind\/config'\)/)
  for (const file of SHIPPED) assert.doesNotMatch(read(file), /process\.env|import\.meta\.env/, file)
})

test('no proof-of-concept name, plugin id or local path in any file of the package', () => {
  const old = ['r', 't', 'h'].join('')
  const names = new RegExp([
    `(?<![A-Za-z])[Rr]${old.slice(1)}`, old.toUpperCase(), `R${old.slice(1)}(?![a-z])`,
    // the proof-of-concept plugin ids (DeepSeek's own @deepseek-ai/dsh-session is not one)
    `(?<![\\w/-])dsh-(?:sse|events|capabilities|settings|mind|session|${old}-trajectory)\\b`,
    `/${'Us'}ers/`, '/home/', '[A-Z]:\\\\\\\\', `scratch${'pad'}`, `coun${'cil'}/`,
  ].join('|'))
  // The pinned upstream bundle names upstream's own CI checkout in its region comments: those are
  // upstream's bytes. The build drops that path, so client.js and every other file carry no home.
  const checkout = /\/home\/runner\/work\/deepseek-harness\//g
  for (const file of TRACKED) {
    for (const [i, line] of read(file).split('\n').entries()) {
      const hit = (file === 'upstream/lib/client.js' ? line.replace(checkout, '') : line).match(names)
      assert.equal(hit, null, `${file}:${i + 1}: ${hit?.[0]}`)
    }
  }
})

test('the bundle registers under this package\'s name, and no file names the old one', () => {
  const name = JSON.parse(read('package.json')).name
  assert.equal(name, '@automagik/unfold-trajectory-dsh')
  const client = read('client.js')
  assert.ok(client.includes(`"${name}"`), 'the loader id is the package name')
  assert.ok(!client.includes('@deepseek-ai/dsh-client-ui-trajectory"'), 'never upstream\'s loader id')
})

test('upstream is DeepSeek\'s trajectory UI 0.2.1-rc.3, with normalized build paths and its MIT notice', () => {
  const up = JSON.parse(read('upstream/package.json'))
  assert.equal(up.name, '@deepseek-ai/dsh-client-ui-trajectory')
  assert.equal(up.version, '0.2.1-rc.3')
  const sums = Object.fromEntries(read('upstream/SHA256SUMS').trim().split('\n').map((l) => l.split(/\s+/).reverse()))
  for (const file of ['LICENSE', 'package.json', 'lib/client.js']) assert.equal(sha(readFileSync(join(ROOT, 'upstream', file))), sums[file], `upstream/${file}`)
  assert.equal(read('LICENSE'), read('upstream/LICENSE'))
  assert.match(read('LICENSE'), /Copyright \(c\) 2026 DeepSeek/)
})

test('the jsdiff that upstream bundles keeps its BSD notice, in the repository and in the bundle', () => {
  assert.match(read('upstream/lib/client.js'), /node_modules\/\.pnpm\/diff@9\.0\.0\//)
  const license = read('third_party/diff-9.0.0/LICENSE')
  assert.match(license, /BSD 3-Clause License/)
  assert.match(license, /Copyright \(c\) 2009-2015, Kevin Decker/)
  const header = read('client.js').slice(0, read('client.js').indexOf('*/'))
  assert.match(header, /diff 9\.0\.0/)
  assert.match(header, /Redistribution and use in source and binary forms/)
  assert.match(read('NOTICE.md'), /BSD 3-Clause License/)
})

test('the scan covers the whole package', () => {
  assert.ok(TRACKED.length > 20, `${TRACKED.length} files`)
  assert.ok(TRACKED.every((f) => !relative(ROOT, join(ROOT, f)).startsWith('..')))
})

test('the scan leaves out an installed harness, and keeps the harness manifest', () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-traj-scan-'))
  try {
    const lib = join(dir, 'dev/harness/node_modules/@deepseek-ai/dsh-agent-loop/lib')
    mkdirSync(lib, { recursive: true })
    writeFileSync(join(lib, 'index.js'), `const PLUGIN_ID = "${['dsh', 'session'].join('-')}"\n`)
    writeFileSync(join(dir, 'dev/harness/package.json'), '{}\n')
    mkdirSync(join(dir, 'node_modules'))
    writeFileSync(join(dir, 'node_modules/x.js'), '')
    assert.deepEqual(walk(dir), [join('dev', 'harness', 'package.json')])
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
