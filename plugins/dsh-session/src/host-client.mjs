// The DSH side of the host control owner's local endpoint (crates/rth/src/host.rs). Plain
// node:http with its own Agent, so no environment proxy ever sees a loopback request, and the
// bearer token is read from the owner's 0600 file on each call (it can be rotated by
// restarting the owner). This client never talks to rth-agentd for input: only the owner
// holds the controller lease.
import http from 'node:http'
import { readFileSync } from 'node:fs'

export class HostClient {
  constructor({ url = 'http://127.0.0.1:7781', tokenFile, token, timeoutMs = 15_000 } = {}) {
    const u = new URL(url)
    if (!['127.0.0.1', 'localhost'].includes(u.hostname)) throw new Error(`host owner must be on loopback, got ${u.hostname}`)
    this.url = u
    this.tokenFile = tokenFile
    this.fixedToken = token
    this.timeoutMs = timeoutMs
    this.agent = new http.Agent({ keepAlive: false })
  }

  token() {
    if (this.fixedToken) return this.fixedToken
    if (!this.tokenFile) throw new Error('no host token configured')
    return readFileSync(this.tokenFile, 'utf8').trim()
  }

  /** `{status, body}`; a transport failure is `{status: 0, body: {error, retry: true}}`. */
  request(method, path, body) {
    return new Promise((resolve) => {
      let token
      try {
        token = this.token()
      } catch (e) {
        resolve({ status: 0, body: { error: `host token unavailable: ${e.message}`, retry: true } })
        return
      }
      const data = body === undefined ? undefined : Buffer.from(JSON.stringify(body))
      const req = http.request(
        {
          agent: this.agent,
          host: this.url.hostname,
          port: this.url.port,
          method,
          path,
          headers: {
            host: `${this.url.hostname}:${this.url.port}`,
            authorization: `Bearer ${token}`,
            ...(data ? { 'content-type': 'application/json', 'content-length': data.length } : {}),
          },
          timeout: this.timeoutMs,
        },
        (res) => {
          const chunks = []
          res.on('data', (c) => chunks.push(c))
          res.on('end', () => {
            let parsed
            try {
              parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'))
            } catch {
              parsed = { error: 'unparseable host response' }
            }
            resolve({ status: res.statusCode, body: parsed })
          })
          res.on('error', (e) => resolve({ status: 0, body: { error: e.message, retry: true } }))
        },
      )
      req.on('timeout', () => req.destroy(new Error('host owner timeout')))
      req.on('error', (e) => resolve({ status: 0, body: { error: e.message, retry: true } }))
      if (data) req.write(data)
      req.end()
    })
  }

  submit(commandId, text) {
    return this.request('POST', '/v1/commands', { command_id: commandId, text })
  }

  status() {
    return this.request('GET', '/v1/status')
  }

  commands() {
    return this.request('GET', '/v1/commands')
  }

  cancel() {
    return this.request('POST', '/v1/cancel', {})
  }

  /**
   * Ask the owner to start (`'start'`) or end (`'end'`) the call: 202 with a request receipt;
   * what happened is `host.call` in the status (commands are requests, receipts are
   * authority). 409 when the owner runs no call, 404 from an owner that predates calls.
   */
  call(action) {
    return this.request('POST', '/v1/call', { call: action })
  }

  /** Ask the owner to mute (true) or unmute its microphone input: 202; `host.mic` is the receipt. */
  mute(muted) {
    return this.request('POST', '/v1/mute', { muted })
  }

  /**
   * The recorded end of a spoken (provider-opened) reply, read only (rth-host):
   * `{reply: {response_id, status, text, detail?, text_complete?, observed?, truncated?}}`, 404
   * for an unknown or evicted id (a 256-entry ring). The adapter takes its text only when
   * `text_complete` is true.
   */
  reply(responseId) {
    return this.request('GET', `/v1/replies/${encodeURIComponent(responseId)}`)
  }
}
