#!/usr/bin/env node
// The integrated app: DSH's web client with the session adapter, the events tab and the status
// tab, attached to a REAL rth-agentd through the one host control owner. Start the owner first,
// in its own terminal (it holds the controller lease, the native voice and the native stop keys):
//
//   cargo run -q -p rth --bin rth-host -- --agent 127.0.0.1:7700 --vm rth-core [--voice]
//
// then this launcher (DSH on 127.0.0.1:3080, no provider configured; it prints the URL):
//
//   node shell/app.mjs [--plain] [--agent-port 7700] [--host-url http://127.0.0.1:7781] [--token-file F] [--port 3080]
//
// The default is the Bot surface (the vendored @automagik/omni-dsh, shell/vendor): profile
// shell/dsh-home-bot, the Bot's home pinned to shell/dsh-home-bot/omni-home (never ~/.omni),
// its team seeded from shell/dsh-home-bot/omni-team-seed (Main Bot "RTH" = the root Session
// rth-root). The Bot copies the seed only into an EMPTY home: an existing team and its state
// are never overwritten. No model is configured. --plain opens plain DSH (shell/dsh-home), a
// diagnostic opt-out. --dsh-home DIR / --omni-home DIR point the same launcher at another home,
// e.g. a fresh copy. The profile's Bot guard admits an Omni home under <RTH_DATA_DIR or
// ~/.local/share/rth>/instances/<id>/, or under this tree's shell/ only with RTH_OMNI_DEV_LAYOUT=1.
//
// The token file defaults to the owner's: ~/.local/share/rth/host/host-<agent port>/token
// (RTH_DATA_DIR overrides the data dir, as for rth-host and rth-deploy).
import { spawn } from 'node:child_process'
import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, readdirSync, readFileSync, unlinkSync, writeSync } from 'node:fs'
import { createHash, randomBytes, randomUUID } from 'node:crypto'
import http from 'node:http'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { resolveDshBin } from './dsh-bin.mjs'

const SHELL = dirname(fileURLToPath(import.meta.url))
const REPO = join(SHELL, '..')
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`)
  return i < 0 ? fallback : process.argv[i + 1]
}
const port = arg('port', '3080')
const agentPort = arg('agent-port', '7700')
const hostUrl = arg('host-url', 'http://127.0.0.1:7781')
const bot = !process.argv.includes('--plain')
const data = process.env.RTH_DATA_DIR || join(homedir(), '.local/share/rth')
const tokenFile = arg('token-file', join(data, 'host', `host-${agentPort}`, 'token'))
if (!existsSync(tokenFile)) {
  console.error(`rth app: no host owner token at ${tokenFile}.\nStart the owner first: cargo run -q -p rth --bin rth-host -- --agent 127.0.0.1:${agentPort} --vm <box>`)
  process.exit(2)
}
const env = {
  ...process.env,
  DSH_HOME: arg('dsh-home', join(SHELL, bot ? 'dsh-home-bot' : 'dsh-home')),
  ...(bot ? { OMNI_HOME: arg('omni-home', join(SHELL, 'dsh-home-bot/omni-home')), RTH_OMNI_TEAM_SEED: join(SHELL, 'dsh-home-bot/omni-team-seed') } : {}),
  RTH_AGENTD_PORT: agentPort,
  RTH_HOST_URL: hostUrl,
  RTH_HOST_TOKEN_FILE: tokenFile,
  RTH_BENCH_RESULTS: process.env.RTH_BENCH_RESULTS || join(REPO, 'bench/results'),
  // This tree's root: the Bot guard admits its packaged read-only team seed (cordis.patch.yml).
  RTH_DSH_ROOT: REPO,
}
// DSH is a view here: it gets no model provider or key.
for (const k of ['DEEPSEEK_API_KEY', 'ANTHROPIC_API_KEY', 'OPENAI_API_KEY', 'GEMINI_API_KEY', 'RTH_GEMINI_API_KEY']) delete env[k]
// Nor any other credential in the launcher's environment (by name; RTH_HOST_TOKEN_FILE is a path).
const CREDENTIAL = /(API_KEY|SECRET|PASSWORD|_TOKEN$)/i
for (const k of Object.keys(env)) if (CREDENTIAL.test(k) && k !== 'RTH_HOST_TOKEN_FILE') delete env[k]
// Delegation: with the explicit opt-in RTH_DELEGATE_WORKER=1, and only once this launch's
// registration is committed (below), the one worker key goes back after the strip above. Its value
// is never printed. A launch identity in our own environment is never this DSH's, and DSH reads no
// opt-in of its own: both are dropped.
const WORKER_KEY = 'LABS_OPENAI_API_KEY_HV'
const delegate = process.env.RTH_DELEGATE_WORKER === '1'
delete env.RTH_DSH_LAUNCH_ID
delete env.RTH_DELEGATE_WORKER
// DSH gets the launch id; the secret stays here. The host
// owner registers sha256(secret) before DSH exists (register-once, POST /v1/launches), and only
// this launcher can later prove DSH's exit (POST /v1/executor, state exited).
const launchId = randomUUID()
const launchSecret = randomBytes(32).toString('hex')
const receipts = join(data, 'host', 'launcher-receipts')
const owner = (path, body) => new Promise((resolve) => {
  let token = ''
  try { token = readFileSync(tokenFile, 'utf8').trim() } catch {}
  const u = new URL(hostUrl)
  const bytes = Buffer.from(JSON.stringify(body))
  const req = http.request({ host: u.hostname, port: u.port, path, method: 'POST', agent: new http.Agent({ keepAlive: false }), headers: { host: `${u.hostname}:${u.port}`, authorization: `Bearer ${token}`, 'content-type': 'application/json', 'content-length': bytes.length }, timeout: 3000 }, (res) => { res.resume(); res.on('end', () => resolve(res.statusCode)) })
  req.on('error', () => resolve(0))
  req.on('timeout', () => req.destroy())
  req.end(bytes)
})
const committed = (code) => code === 200 || code === 202
const writeReceipt = (rec) => {
  mkdirSync(receipts, { recursive: true, mode: 0o700 })
  const fd = openSync(join(receipts, `${rec.launch_id}.json`), 'w', 0o600)
  try { writeSync(fd, JSON.stringify(rec)); fsyncSync(fd) } finally { closeSync(fd) }
}
// Earlier launches' exits first: a receipt (it holds that launch's secret, 0600) is kept until
// the owner accepts it, then removed.
if (delegate && existsSync(receipts)) for (const f of readdirSync(receipts)) {
  const rec = JSON.parse(readFileSync(join(receipts, f), 'utf8'))
  if (committed(await owner('/v1/executor', { state: 'exited', ...rec }))) unlinkSync(join(receipts, f))
}
const registered = delegate && (await owner('/v1/launches', { launch_id: launchId, secret_sha256: createHash('sha256').update(launchSecret).digest('hex') })) === 200
// Registration gates the worker. Only a committed registration gives DSH its launch id and
// the worker key, together; a refused, timed-out or lost answer gives it neither, so the plugin's
// executor stays off (no launch id: no lease, no admission) and no worker credential exists.
if (registered) {
  env.RTH_DSH_LAUNCH_ID = launchId
  if (process.env[WORKER_KEY]) env[WORKER_KEY] = process.env[WORKER_KEY]
} else if (delegate) console.error('rth app: the host owner did not register this launch; the delegate worker stays off')
// DSH runs on this node with shell/dsh-exit.mjs preloaded in DSH's own process only (an --import
// argument, not NODE_OPTIONS, so no child of DSH inherits it): its shutdown otherwise blocks in macOS
// FSEvents watcher teardown.
// DSH's command from its package.json "bin" field (dsh-bin.mjs): packaging drops the .bin links.
const dshBin = resolveDshBin(SHELL)
const dsh = spawn(process.execPath, ['--import', pathToFileURL(join(SHELL, 'dsh-exit.mjs')).href, dshBin, '--profile', 'web', '--no-open', '--host', '127.0.0.1', '--port', port], { cwd: SHELL, stdio: ['ignore', 'inherit', 'inherit'], env })
let stopping = false
const stop = () => {
  if (stopping) return
  stopping = true
  dsh.kill('SIGTERM')
  // Backstop in case DSH outlives SIGTERM (FSEvents watcher teardown); its own forced exit is at 5 s.
  setTimeout(() => dsh.exitCode === null && dsh.signalCode === null && dsh.kill('SIGKILL'), 10_000).unref()
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
dsh.on('exit', async (code, signal) => {
  process.exitCode = code ?? 0
  if (!registered) return
  // The parent-observed exit of its own child: persisted before it is posted, retried next launch.
  const rec = { launch_id: launchId, pid: dsh.pid, code, signal, launch_proof: launchSecret }
  writeReceipt(rec)
  if (committed(await owner('/v1/executor', { state: 'exited', ...rec }))) unlinkSync(join(receipts, `${launchId}.json`))
})
