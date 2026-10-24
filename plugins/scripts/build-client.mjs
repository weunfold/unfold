// Build each plugin's browser half into lib/client.js in DSH's client bundle format (the
// upstream tsdown preset's closure-factory artifact): the bundle calls
// window.__ModuleLoader__.load({ id, factory }) and resolves externals through the injected
// require, i.e. the shell's shared module table (react, react/jsx-runtime, @deepseek-ai/*).
// No React or cordis copy is bundled. Usage: node scripts/build-client.mjs [plugin-dir...]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dirs = process.argv.slice(2).length ? process.argv.slice(2) : ['dsh-events', 'dsh-capabilities', 'dsh-session']

for (const dir of dirs) {
  const pkgDir = resolve(root, dir)
  const pkg = JSON.parse(readFileSync(join(pkgDir, 'package.json'), 'utf8'))
  const out = join(pkgDir, pkg.exports['./client'].default)
  const result = await build({
    entryPoints: [join(pkgDir, 'client/index.jsx')],
    bundle: true,
    write: false,
    format: 'cjs',
    platform: 'browser',
    target: 'es2022',
    jsx: 'automatic',
    external: ['react', 'react/jsx-runtime', 'react-dom', 'react-dom/client', '@deepseek-ai/*'],
    legalComments: 'none',
    logLevel: 'warning',
  })
  const body = result.outputFiles[0].text.replace(/\n$/, '').split('\n').map((l) => `\t\t${l}`).join('\n')
  const text = [
    'window.__ModuleLoader__.load({',
    `\tid: ${JSON.stringify(pkg.name)},`,
    '\tfactory: (require) => {',
    '\t\tvar module = { exports: {} };',
    '\t\tvar exports = module.exports;',
    '\t\tObject.defineProperty(exports, Symbol.toStringTag, { value: "Module" });',
    body,
    '\t\treturn module.exports;',
    '\t}',
    '});',
    '',
  ].join('\n')
  mkdirSync(dirname(out), { recursive: true })
  writeFileSync(out, text)
  console.log(`${pkg.name}: ${out} (${text.length} bytes)`)
}
