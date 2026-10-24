#!/usr/bin/env node
// A synthetic rth-agentd for development: a small TCP server speaking protocol v2 (JSON lines)
// that replays the guest frames of a recorded transcript. It is not agentd and
// runs no model. It never touches the rth-core VM; it only binds 127.0.0.1.
//
// The handshake is agentd's: on connect it sends `hello`; it reads the client's `attach` and
// answers `ready` for that attachment id, granting `observer` when observer is asked (and
// refusing input from it, by ignoring every later frame). Each process start is a new boot:
// producers and ids are rewritten with a fresh boot nonce, so a restart reads as a new agent.
//
// Usage: node replay.mjs [--port 7787] [--auto] [--gap-ms 600] [--fixtures a.jsonl,b.jsonl]
//   --auto     replay the transcripts on their own clock (gaps capped), looping with new ids
//   (manual)   read commands on stdin: `emit N` sends the next N frames, `emit all` the rest
import { readFileSync } from 'node:fs'
import net from 'node:net'
import { dirname, resolve } from 'node:path'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const FIXTURES = resolve(here, '../../plugins/dsh-events/test/fixtures')
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`)
  return i < 0 ? fallback : process.argv[i + 1]
}
const port = Number(arg('port', 7787))
const auto = process.argv.includes('--auto')
const gapCap = Number(arg('gap-ms', 600))
const fixtures = arg('fixtures', 'tc-dev-03-second-input-reactive.jsonl,tc-dev-05-cancel-after-first-delta.jsonl')
  .split(',')
  .map((f) => resolve(FIXTURES, f))

const boot = `${Date.now().toString(16)}-${process.pid}`
let loop = 0

/** Guest frames after the handshake, from each transcript, with producers renamed for this boot and loop. */
function script() {
  loop++
  const frames = []
  for (const file of fixtures) {
    const recs = readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l))
    const recv = recs.filter((r) => r.rec === 'recv' && r.env?.source === 'guest' && !['hello', 'ready'].includes(r.env.kind))
    for (const r of recv) {
      // The recorded boot id is in the producer, every id and every response id: renaming it
      // makes each boot and loop a distinct agent run, so nothing coalesces across them.
      const original = r.env.producer.replace(/^guest-/, '')
      const text = JSON.stringify(r.env).replaceAll(original, `synthetic-${boot}-l${loop}`)
      frames.push({ t_ns: r.t_ns, env: JSON.parse(text) })
    }
  }
  return frames
}

const producer = `guest-synthetic-${boot}`
let seq = 0
const guest = (kind, fields) => ({ v: 2, id: `${producer}:${++seq}`, producer, seq, t_ns: Number(process.hrtime.bigint()), source: 'guest', kind, ...fields })

const clients = new Set()
let queue = script()
let sent = 0

function broadcast(env) {
  const line = `${JSON.stringify(env)}\n`
  for (const c of clients) if (c.ready) c.sock.write(line)
}

function emit(n) {
  let k = 0
  while (k < n) {
    if (!queue.length) queue = script()
    broadcast(queue.shift().env)
    sent++
    k++
  }
  log(`emitted ${k} (total ${sent})`)
}

function log(msg) {
  process.stderr.write(`[agentd-replay ${boot}] ${msg}\n`)
}

const server = net.createServer((sock) => {
  const c = { sock, ready: false, role: null }
  clients.add(c)
  sock.on('error', () => {})
  sock.on('close', () => clients.delete(c))
  sock.write(`${JSON.stringify(guest('hello', { protocol: 2, agent: 'rth-agentd', version: 'synthetic-replay', boot_id: boot, journal: null }))}\n`)
  let buf = ''
  sock.on('data', (chunk) => {
    buf += chunk
    for (let nl; (nl = buf.indexOf('\n')) >= 0; buf = buf.slice(nl + 1)) {
      const line = buf.slice(0, nl)
      let env
      try {
        env = JSON.parse(line)
      } catch {
        continue
      }
      if (c.ready) continue // an observer's input is refused, as agentd does
      if (env.kind === 'attach') {
        c.role = 'observer' // this stand-in grants observer only, whatever is asked: it takes no input
        sock.write(`${JSON.stringify(guest('ready', { attachment_id: env.attachment_id, role: c.role }))}\n`)
        c.ready = true
        log(`attached ${env.attachment_id} as ${c.role}`)
      }
    }
  })
})

server.listen(port, '127.0.0.1', () => {
  log(`listening on 127.0.0.1:${port} (${auto ? 'auto' : 'manual'}), ${queue.length} frames per loop`)
  process.stdout.write(`READY ${port}\n`)
  if (auto) tick()
})

function tick() {
  if (!queue.length) {
    queue = script()
    return setTimeout(tick, 3000)
  }
  const [a, b] = queue
  emit(1)
  const gap = b ? Math.min(gapCap, Math.max(20, (b.t_ns - a.t_ns) / 1e6)) : 0
  setTimeout(tick, gap)
}

if (!auto) {
  createInterface({ input: process.stdin }).on('line', (line) => {
    const m = /^emit (\d+|all)$/.exec(line.trim())
    if (!m) return log(`unknown command: ${line}`)
    emit(m[1] === 'all' ? queue.length : Number(m[1]))
    process.stdout.write(`EMITTED ${sent}\n`)
  })
}

const stop = () => {
  for (const c of clients) c.sock.destroy()
  server.close(() => process.exit(0))
}
process.on('SIGTERM', stop)
process.on('SIGINT', stop)
