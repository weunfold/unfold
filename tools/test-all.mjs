#!/usr/bin/env node
// Run each JavaScript suite with explicit test inputs and report every skip.
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const NO_RTH = process.argv.includes('--no-rth')
const mode = NO_RTH ? 'alone' : 'with-rth'
const LOGS = join(REPO, '.test-logs', mode)
mkdirSync(LOGS, { recursive: true })
const testEnv = Object.fromEntries(['TMPDIR', 'TEST_SOCKET_ROOT', 'NODE_OPTIONS', 'ESBUILD_BINARY_PATH'].filter(key => process.env[key]).map(key => [key, process.env[key]]))
const env = { PATH: `${dirname(process.execPath)}:/usr/bin:/bin`, HOME: process.env.HOME, ...testEnv, ...(NO_RTH ? { RTH_CHECKOUT: join(REPO, '.no-rth-checkout') } : process.env.RTH_CHECKOUT ? { RTH_CHECKOUT: process.env.RTH_CHECKOUT } : {}) }
const SUITES = [
  ...readdirSync(join(REPO, 'plugins')).filter(name => /^unfold-.*-dsh$/.test(name)).sort().map(name => [name, `plugins/${name}`, [
    ...(existsSync(join(REPO, 'plugins', name, 'test/harness-resolve.mjs')) ? ['--import', './test/harness-resolve.mjs'] : []),
    '--test', '--test-concurrency=4', '--test-force-exit', 'test/*.test.js',
  ], { ...(process.env.DSH_HARNESS ? { DSH_HARNESS: process.env.DSH_HARNESS } : {}) }]),
  ['bot', 'shell/vendor/omni-ui-dsh', ['--import', './test/harness-resolve.mjs', '--test', '--test-concurrency=4', 'test/*.test.js'], { DSH_HARNESS: process.env.DSH_HARNESS || join(REPO, 'test/bot-harness') }],
  ['shell', 'shell', ['--test', '--test-concurrency=4', 'test/*.test.mjs']],
]
const rows = []
let failed = false
for (const [name, dir, args, extra = {}] of SUITES) {
  const r = spawnSync(process.execPath, args, { cwd: join(REPO, dir), env: { ...env, ...extra }, encoding: 'utf8', timeout: 1_200_000, maxBuffer: 256 * 1024 * 1024 })
  const out = `${r.stdout ?? ''}${r.stderr ?? ''}`
  writeFileSync(join(LOGS, `${name}.log`), out)
  const n = (k) => Number(out.match(new RegExp(`^ℹ ${k} (\\d+)`, 'm'))?.[1] ?? NaN)
  const row = { suite: name, tests: n('tests'), pass: n('pass'), fail: n('fail'), skipped: n('skipped'), exit: r.status ?? r.signal }
  if (row.exit !== 0 || !(row.fail === 0)) failed = true
  rows.push(row)
}
writeFileSync(join(LOGS, 'summary.json'), `${JSON.stringify({ mode, rth_checkout: env.RTH_CHECKOUT ?? (existsSync(join(REPO, 'agentd/Cargo.toml')) ? REPO : resolve(REPO, '../rth')), node: process.version, rows }, null, 2)}\n`)
for (const r of rows) console.log(`${mode}\t${r.suite}\ttests ${r.tests}\tpass ${r.pass}\tfail ${r.fail}\tskipped ${r.skipped}\texit ${r.exit}`)
process.exit(failed ? 1 : 0)
