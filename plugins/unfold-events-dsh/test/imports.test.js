// Every package a shipped Node file imports is declared in dependencies or peerDependencies, so a
// strict install (pnpm, as DSH installs a profile) links it. What ships is package.json `files`,
// `main` and `exports`; client.js and src/client/ are browser bundles whose externals the DSH
// client provides, and node: imports and builtins need no declaration.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { builtinModules } from 'node:module'
import { join, relative } from 'node:path'

const root = join(import.meta.dirname, '..')
const manifest = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
const SPEC = /(?:\bfrom\s*|\bimport\s*\(\s*|\brequire\s*\(\s*|^\s*import\s+)(['"])([^'"]+)\1/gm
const BUILTIN = new Set(builtinModules)

const exportTargets = (e) => (typeof e === 'string' ? [e] : e && typeof e === 'object' ? Object.values(e).flatMap(exportTargets) : [])
const walk = (path) => (statSync(path).isDirectory() ? readdirSync(path).flatMap((n) => walk(join(path, n))) : [path])
const browser = (rel) => rel === 'client.js' || rel.startsWith('src/client/')
const packageName = (spec) => spec.split('/').slice(0, spec.startsWith('@') ? 2 : 1).join('/')

function shippedNodeFiles() {
  const entries = [...(manifest.files ?? []), manifest.main, ...exportTargets(manifest.exports)].filter(Boolean)
  const files = new Set(entries.flatMap((e) => walk(join(root, e))).map((f) => relative(root, f)))
  return [...files].filter((rel) => /\.(m?js|cjs|jsx)$/.test(rel) && !browser(rel)).sort()
}

test('every package a shipped Node file imports is declared as a dependency or a peer', () => {
  const declared = new Set([...Object.keys(manifest.dependencies ?? {}), ...Object.keys(manifest.peerDependencies ?? {})])
  const files = shippedNodeFiles()
  assert.ok(files.includes('index.js'), 'the entry is among the shipped files')
  const undeclared = {}
  for (const rel of files) {
    for (const [, , spec] of readFileSync(join(root, rel), 'utf8').matchAll(SPEC)) {
      if (/^(\.|\/|node:|#|https?:)/.test(spec) || BUILTIN.has(spec.split('/')[0])) continue
      const name = packageName(spec)
      if (name !== manifest.name && !declared.has(name)) (undeclared[name] ??= new Set()).add(rel)
    }
  }
  const named = Object.entries(undeclared).map(([name, from]) => `${name} (${[...from].join(', ')})`)
  assert.deepEqual(named, [], `declare in peerDependencies: ${named.join('; ')}`)
})
