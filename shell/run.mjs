#!/usr/bin/env node
// Launch the shell for a person: the synthetic agentd (auto replay) on 127.0.0.1:7787 and
// plain DSH 0.2.1-rc.2 (`dsh --profile web`) on 127.0.0.1:3080 with shell/dsh-home as DSH_HOME.
// Prints the DSH URL (it carries the one-time browser token); Ctrl-C stops both.
// Usage: node shell/run.mjs [--port 3080] [--agentd-port 7787] [--no-agentd]
import { spawn } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const SHELL = dirname(fileURLToPath(import.meta.url))
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`)
  return i < 0 ? fallback : process.argv[i + 1]
}
const port = arg('port', '3080')
const agentdPort = arg('agentd-port', '7787')
const children = []

if (!process.argv.includes('--no-agentd')) {
  children.push(spawn(process.execPath, [join(SHELL, 'agentd/replay.mjs'), '--port', agentdPort, '--auto'], { stdio: ['ignore', 'ignore', 'ignore'] }))
}
const dsh = spawn(join(SHELL, 'node_modules/.bin/dsh'), ['--profile', 'web', '--no-open', '--host', '127.0.0.1', '--port', port], {
  cwd: SHELL,
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, DSH_HOME: join(SHELL, 'dsh-home'), RTH_AGENTD_PORT: agentdPort },
})
children.push(dsh)
// DSH 0.2.1-rc.2 sometimes outlives SIGTERM while a browser is attached, so a child still
// running 10 s after SIGTERM is killed.
let stopping = false
const stop = () => {
  if (stopping) return
  stopping = true
  for (const c of children) {
    if (c.exitCode !== null || c.signalCode !== null) continue
    c.kill('SIGTERM')
    setTimeout(() => c.exitCode === null && c.signalCode === null && c.kill('SIGKILL'), 10_000).unref()
  }
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
dsh.on('exit', (code) => {
  stop()
  process.exitCode = code ?? 0
})
