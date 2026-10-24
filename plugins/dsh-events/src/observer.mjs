// Read-only attachment to rth-agentd (protocol v2, JSON lines). The agent pushes every
// event; nothing here polls. A dropped link reconnects with backoff when the socket closes,
// but reconnecting is not recovery: agentd's observer stream has no replay cursor, skips
// events when an attachment lags, and never carries the host frames it admitted.
import net from 'node:net'

export const PROTOCOL_VERSION = 2
export const MAX_FRAME = 64 * 1024
const BACKOFF_MS = [250, 500, 1000, 2000, 5000]

export class AgentdObserver {
  constructor({ host = '127.0.0.1', port = 7700, onEnvelope, onState = () => {}, producer } = {}) {
    this.host = host
    this.port = port
    this.onEnvelope = onEnvelope
    this.onState = onState
    this.producer = producer ?? `dsh-events-${Date.now().toString(16)}-${process.pid}`
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
    this.connect()
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
    this.setState('connecting')
    const socket = net.connect({ host: this.host, port: this.port })
    this.socket = socket
    let buf = Buffer.alloc(0)
    let phase = 'hello'
    // Bytes, not characters: MAX_FRAME is agentd's UTF-8 cap on one frame without its newline
    // (rth-protocol io.rs), checked on every complete frame before parsing and on the partial tail.
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
    if (env.v !== PROTOCOL_VERSION || env.source !== 'guest') {
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

  retry() {
    const delay = BACKOFF_MS[Math.min(this.attempt++, BACKOFF_MS.length - 1)]
    this.setState('waiting', delay)
    this.timer = setTimeout(() => this.connect(), delay)
  }
}
