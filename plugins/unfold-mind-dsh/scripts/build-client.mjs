// Builds client.js, the DSH client half, from src/client/index.jsx (the Mind tab) and the pure
// modules it shares with the server half (src/display.js, src/moments.js, src/metrics.js).
//
//   node scripts/build-client.mjs           writes client.js
//   node scripts/build-client.mjs --check   exits 1 when client.js differs from a fresh build
//
// client.js is committed, so `link:` and git installs need no build step. DSH loads it as a
// classic script that hands a factory to its module loader; `require` resolves the shell-seeded
// platform modules (react, its JSX runtime, react-dom, @deepseek-ai/*), which therefore stay
// external: no React or Cordis copy is bundled.
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'

const PLUGIN = dirname(dirname(fileURLToPath(import.meta.url)))
// DSH's client loader matches a bundle's id to the npm package that ships it.
const PACKAGE_NAME = JSON.parse(readFileSync(join(PLUGIN, 'package.json'), 'utf8')).name
const OUTPUT = join(PLUGIN, 'client.js')
export const PLATFORM_MODULES = ['react', 'react/jsx-runtime', 'react-dom', 'react-dom/client', '@deepseek-ai/*']

export async function bundleClient() {
  const result = await build({
    absWorkingDir: PLUGIN,
    entryPoints: ['src/client/index.jsx'],
    bundle: true,
    format: 'cjs',
    platform: 'browser',
    target: 'es2022',
    jsx: 'automatic',
    external: PLATFORM_MODULES,
    legalComments: 'none',
    charset: 'utf8',
    write: false,
    logLevel: 'silent',
  })
  if (result.warnings.length > 0) throw new Error(`esbuild warnings:\n${result.warnings.map(warning => warning.text).join('\n')}`)
  return [
    '// Generated from src/client by `npm run build` (scripts/build-client.mjs). Do not edit by hand.',
    'window.__ModuleLoader__.load({',
    `  id: '${PACKAGE_NAME}',`,
    '  factory: (require) => {',
    'const module = { exports: {} }',
    result.outputFiles[0].text.trimEnd(),
    // The loader gets a plain exports object tagged as a module, as DSH's own bundles are.
    'return Object.defineProperty({ ...module.exports }, Symbol.toStringTag, { value: \'Module\' })',
    '  },',
    '})',
    '',
  ].join('\n')
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const next = await bundleClient()
  if (process.argv.includes('--check')) {
    let current = null
    try { current = readFileSync(OUTPUT, 'utf8') } catch { /* missing counts as stale */ }
    if (current !== next) {
      console.error('client.js is stale: run `npm run build` in apps/unfold-mind-dsh and commit the result')
      process.exit(1)
    }
  } else {
    writeFileSync(OUTPUT, next)
  }
}
