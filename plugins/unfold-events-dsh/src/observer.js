// Read-only attachment to agentd (protocol v2, JSON lines). The agent pushes every
// event; nothing here polls. A dropped link reconnects with backoff when the socket closes,
// but reconnecting is not recovery: agentd's observer stream has no replay cursor, skips
// events when an attachment lags, and never carries the host frames it admitted.
//
// Where agentd is: in Unfold's own instance the app publishes agentd's address in a 0600 file
// (one line, a literal loopback address with a port) only while its chat holds agentd's
// controller lease, and withdraws it before it lets the lease go. The observer gets that file's
// path, never an address, and reads it at every connect: an absent file means "not now", so it
// waits without dialling; anything but a literal loopback address with a port is refused. No
// address or port is ever assumed.
import { readFileSync } from 'node:fs'
import net from 'node:net'

export const PROTOCOL_VERSION = 2
export const MAX_FRAME = 64 * 1024
const BACKOFF_MS = [250, 500, 1000, 2000, 5000]
const LOOPBACK = /^(?:127(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}|\[::1\]):(\d{1,5})$/

/**
 * The address in an observer file, or why there is none: `{ host, port }`, `{ absent }` (not
 * published now) or `{ refused }` (one line saying what is wrong).
 */
export function readObserverFile(path) {
  let text
  try {
    text = readFileSync(path, 'utf8')
  } catch (err) {
    if (err.code === 'ENOENT') return { absent: 'the observer address is not published now (Unfold does not hold the agent now)' }
    return { refused: `the observer file cannot be read (${err.code ?? err.message})` }
  }
  const line = text.replace(/\n$/, '')
  const m = LOOPBACK.exec(line)
  const port = m ? Number(m[1]) : 0
  if (!m || port < 1 || port > 65535) return { refused: `the observer file holds ${JSON.stringify(line.slice(0, 64))}, not a literal loopback address with a port` }
  const host = line.slice(0, line.lastIndexOf(':')).replace(/^\[(.*)\]$/, '$1')
  return { host, port }
}

export class AgentdObserver {
  /** `observerFile`: the path of the published address; without one there is no channel. */
  constructor({ observerFile, onEnvelope, onState = () => {}, producer } = {}) {
    this.observerFile = observerFile ?? null
    this.onEnvelope = onEnvelope
    this.onState = onState
    this.producer = producer ?? `unfold-events-${Date.now().toString(16)}-${process.pid}`
    this.origin = process.hrtime.bigint()
    this.seq = 0
    this.attempt = 0
    this.state = 'idle'
    this.rejected = 0
    this.socket = null
    this.timer = null
    this.stopped = true
  }

  start() {
    this.stopped = false
    if (!this.observerFile) this.setState('unconfigured', 'no observer channel: this DSH is not an Unfold instance, or its launcher handed none over')
    else this.connect()
    return this
  }

  stop() {
    this.stopped = true
    clearTimeout(this.timer)
    this.socket?.destroy()
    this.setState('stopped')
  }

  now() {
    return Number(process.hrtime.bigint() - this.origin)
  }

  setState(state, detail) {
    this.state = state
    this.onState(state, detail)
  }

  connect() {
    const at = readObserverFile(this.observerFile)
    // Not published now, or refused: try the file again later, keeping the reason as the state.
    if (at.absent) return this.retry('unpublished', at.absent)
    if (at.refused) return this.retry('refused', at.refused)
    this.setState('connecting', `${at.host.includes(':') ? `[${at.host}]` : at.host}:${at.port}`)
    const socket = net.connect({ host: at.host, port: at.port })
    this.socket = socket
    let buf = Buffer.alloc(0)
    let phase = 'hello'
    // Bytes, not characters: MAX_FRAME is agentd's UTF-8 cap on one frame without its newline
    // (unfold-protocol io.rs), checked on every complete frame before parsing and on the partial tail.
    socket.on('data', (chunk) => {
      buf = buf.length ? Buffer.concat([buf, chunk]) : chunk
      let nl
      while ((nl = buf.indexOf(0x0a)) >= 0) {
        const frame = buf.subarray(0, nl)
        buf = buf.subarray(nl + 1)
        if (frame.length > MAX_FRAME) return this.oversize(socket, frame.length)
        phase = this.frame(socket, frame.toString('utf8'), phase)
        if (socket.destroyed) return
      }
      if (buf.length > MAX_FRAME) this.oversize(socket, buf.length)
    })
    socket.on('error', (err) => this.setState('error', err.message))
    socket.on('close', () => {
      if (this.socket === socket) this.socket = null
      if (!this.stopped) this.retry()
    })
  }

  oversize(socket, bytes) {
    this.rejected++
    socket.destroy(new Error(`frame of ${bytes} bytes exceeds ${MAX_FRAME}`))
  }

  frame(socket, line, phase) {
    const t_ns = this.now()
    let env
    try {
      env = JSON.parse(line)
    } catch {
      this.rejected++
      return phase
    }
    if (!env || typeof env !== 'object' || Array.isArray(env) || env.v !== PROTOCOL_VERSION || env.source !== 'guest') {
      this.rejected++
      return phase
    }
    env.observed = { by: this.producer, t_ns } // env.received, if any, stays the producer's own
    if (phase === 'hello') {
      if (env.kind !== 'hello') {
        socket.destroy(new Error(`expected hello, got ${env.kind}`))
        return phase
      }
      socket.write(this.attach())
      phase = 'ready'
    } else if (phase === 'ready' && env.kind === 'ready' && env.attachment_id === this.attachmentId) {
      // Attached only once agentd grants exactly what was asked: a read-only observer.
      if (env.role !== 'observer') {
        socket.destroy(new Error(`asked for observer, granted ${env.role}`))
        return phase
      }
      this.attempt = 0
      this.setState('attached', env.role)
      phase = 'stream'
    }
    this.onEnvelope?.(env)
    return phase
  }

  attach() {
    this.seq++
    this.attachmentId = `${this.producer}-a${this.seq}`
    const env = {
      v: PROTOCOL_VERSION,
      id: `${this.producer}:${this.seq}`,
      producer: this.producer,
      seq: this.seq,
      t_ns: this.now(),
      source: 'host',
      kind: 'attach',
      protocol: PROTOCOL_VERSION,
      attachment_id: this.attachmentId,
      role: 'observer',
    }
    return `${JSON.stringify(env)}\n`
  }

  retry(state = 'waiting', detail) {
    const delay = BACKOFF_MS[Math.min(this.attempt++, BACKOFF_MS.length - 1)]
    this.setState(state, state === 'waiting' ? delay : detail)
    this.timer = setTimeout(() => this.connect(), delay)
  }
}
