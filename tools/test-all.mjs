#!/usr/bin/env node
// Every DSH-side suite of this repository, from its own installs (npm ci in plugins/, shell/,
// shell/vendor/omni-ui-dsh/ and test/bot-harness/), each in a clean environment (PATH and HOME only).
// Prints pass/fail/skipped per suite and writes the logs to .test-logs/<mode>/ (gitignored).
//   node tools/test-all.mjs            cross-repo contract tests read the sibling ../rth (or $RTH_CHECKOUT)
//   node tools/test-all.mjs --no-rth   this repository alone: those tests are SKIPPED, with their reasons
// Exit 0 only when no suite has a failure (a skip is reported, never counted as a pass).
import { spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const NO_RTH = process.argv.includes('--no-rth')
const mode = NO_RTH ? 'alone' : 'with-rth'
const LOGS = join(REPO, '.test-logs', mode)
mkdirSync(LOGS, { recursive: true })
const env = { PATH: `${dirname(process.execPath)}:/usr/bin:/bin`, HOME: process.env.HOME, ...(NO_RTH ? { RTH_CHECKOUT: join(REPO, '.no-rth-checkout') } : process.env.RTH_CHECKOUT ? { RTH_CHECKOUT: process.env.RTH_CHECKOUT } : {}) }
const SUITES = [
  ['dsh-session', 'plugins/dsh-session', ['--test', '--test-timeout=20000', 'test/*.test.mjs']],
  ['plugins', 'plugins', ['--test']],
  ['bot', 'shell/vendor/omni-ui-dsh', ['--import', './test/harness-resolve.mjs', '--test', 'test/*.test.js'], { DSH_HARNESS: join(REPO, 'test/bot-harness') }],
  ['shell', 'shell', ['--test', 'test/*.test.mjs']],
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
writeFileSync(join(LOGS, 'summary.json'), `${JSON.stringify({ mode, rth_checkout: env.RTH_CHECKOUT ?? resolve(REPO, '../rth'), node: process.version, rows }, null, 2)}\n`)
for (const r of rows) console.log(`${mode}\t${r.suite}\ttests ${r.tests}\tpass ${r.pass}\tfail ${r.fail}\tskipped ${r.skipped}\texit ${r.exit}`)
process.exit(failed ? 1 : 0)
