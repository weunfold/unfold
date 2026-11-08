// The DSH side of Unfold's session-control endpoint: `POST /submit {session, text}` (a typed
// turn), and `GET /answer-with` / `POST /answer-with {choice?, effort?, ack?}` (what Unfold
// answers with: the model menu of the Unfold Bot, ./answer-with.js), over the Unix socket in the instance's 0700 socket dir, with the per-start bearer token. The
// socket path and the token come only from the launcher preload's registry: the preload read
// them from the instance child's environment once, deleted the variables and unlinked the token
// file before any plugin loaded. There is no TCP path, no default and no token file to re-read.
// Plain node:http with its own Agent, so no environment proxy ever sees the request.
import http from 'node:http'

/** The preload's registry: `{socket, token, observerFile}`, frozen and not enumerable. */
export const CHANNEL = Symbol.for('unfold.channel')

/** The registry's values, or null when this DSH was not started by Unfold's launcher. */
export function readChannel() {
  const c = globalThis[CHANNEL]
  if (!c || typeof c !== 'object') return null
  const str = (v) => (typeof v === 'string' && v !== '' ? v : null)
  return { socket: str(c.socket), token: str(c.token), observerFile: str(c.observerFile) }
}

/** A text the endpoint refuses as an owner command (check 6): its trimmed start is "/". */
export const isOwnerCommand = (text) => String(text).trim().startsWith('/')

// Validate without normalizing: command ids are opaque identities, never rewritten.
export const validTurnId = (id) => typeof id === 'string' && id.trim().length > 0

// The endpoint's refusal codes, as the person reads them. Never the text, never the token.
const REFUSED = {
  request: 'the control endpoint could not read the request',
  not_found: 'the control endpoint does not know this request',
  token: "the control endpoint refused this instance's token",
  body: 'the control endpoint refused the message (empty or over 8000 characters)',
  command: 'a message that starts with "/" is an owner command, which only Unfold\'s own keyboard sends',
  session: 'the message was for another Unfold Session',
  unavailable: 'Unfold did not take the turn (no chat is running)',
  not_offered: 'Unfold does not offer that model or level now',
  needs_ack: 'Unfold needs your confirmation before it sends your messages there',
}
const MAX_ANSWER = 64 * 1024

export const refusedText = (code) => REFUSED[code] ?? `the control endpoint answered ${code}`

export class HostClient {
  #channel
  constructor({ channel = readChannel, timeoutMs = 15_000 } = {}) {
    this.#channel = channel
    this.timeoutMs = timeoutMs
    this.agent = new http.Agent({ keepAlive: false })
  }

  /**
   * Submit `text` as one turn in Unfold Session `session`: `{status, body}`.
   * - 200 `{accepted: true, turn_id}`: the chat sent the turn; `turn_id` is its command id.
   * - any other 200: `sent: 'unknown'` (error `unreadable`); an answer over 64 KiB is cut off,
   *   `sent: 'unknown'` (error `too_large`).
   * - 400/401/403/404/503 `{refused}`: nothing was sent (`sent: false` is added).
   * - status 0: no answer. `sent: false` when nothing was written (no channel, the socket could
   *   not be reached, a refused peer); `sent: 'unknown'` when the connection failed after the
   *   request was written. Nothing here ever submits again.
   * An owner command is refused here (400 `command`) without connecting.
   */
  submit(session, text) {
    const c = this.#channel()
    if (!c?.socket || !c?.token) return Promise.resolve({ status: 0, body: { error: 'no_channel', sent: false } })
    if (typeof session !== 'string' || session === '') return Promise.resolve({ status: 0, body: { error: 'no_session', sent: false } })
    if (isOwnerCommand(text)) return Promise.resolve({ status: 400, body: { refused: 'command', sent: false } })
    const data = Buffer.from(JSON.stringify({ session, text }))
    return new Promise((resolve) => {
      let written = false
      let done = false
      const finish = (r) => {
        if (done) return
        done = true
        resolve(r)
      }
      const req = http.request(
        {
          agent: this.agent,
          socketPath: c.socket,
          method: 'POST',
          path: '/submit',
          headers: { host: 'unfold', authorization: `Bearer ${c.token}`, 'content-type': 'application/json', 'content-length': data.length },
          timeout: this.timeoutMs,
        },
        (res) => {
          // The endpoint caps its answers at 64 KiB; anything bigger is not its answer and is not read.
          const chunks = []
          let size = 0
          res.on('data', (b) => {
            size += b.length
            if (size > MAX_ANSWER) {
              finish({ status: 0, body: { error: 'too_large', sent: 'unknown' } })
              return req.destroy()
            }
            chunks.push(b)
          })
          res.on('end', () => {
            let body
            try {
              body = JSON.parse(Buffer.concat(chunks).toString('utf8'))
            } catch {
              body = { error: 'unparseable' }
            }
            // A 200 means the chat sent the turn: one that is not `{accepted: true, turn_id}` may
            // still have been sent, so it is unknown, never not sent.
            if (res.statusCode !== 200) body = { ...body, sent: false }
            else if (body?.accepted !== true || !validTurnId(body.turn_id)) body = { ...body, error: 'unreadable', sent: 'unknown' }
            finish({ status: res.statusCode, body })
          })
          res.on('error', () => finish({ status: 0, body: { error: 'transport', sent: 'unknown' } }))
        },
      )
      req.on('timeout', () => req.destroy(new Error('timeout')))
      // A refused peer gets one line that is not HTTP, before its request is read, and the
      // connection closes: the client sees that line (`peer`). EPIPE is a close before the request
      // was all written, by a refusal or an endpoint that stopped (`closed`). The endpoint accepts
      // only a whole request, so neither was sent.
      req.on('error', (e) => {
        const code = e.code?.startsWith('HPE_') ? 'peer' : e.code === 'EPIPE' ? 'closed' : e.code === 'ENOENT' || e.code === 'ECONNREFUSED' ? 'unreachable' : 'transport'
        finish({ status: 0, body: { error: code, sent: code === 'transport' && written ? 'unknown' : false } })
      })
      req.on('finish', () => { written = true })
      req.end(data)
    })
  }

  /**
   * What Unfold answers with: `{status, body}`, 200 with the app's facts (see ./answer-with.js).
   * Any other status, or status 0 with `{error}` when nothing answered. Read-only, so a failed
   * read changes nothing.
   */
  answerWith() {
    return this.#call('GET', '/answer-with')
  }

  /**
   * Pick what Unfold answers with: `{choice?, effort?, ack?}` -> 200 `{rev, after_reply}`;
   * 400 `body`, 409 `not_offered` or `needs_ack`, 401/403/503 as for /submit. Never retried: a
   * status 0 after the request was written is `sent: 'unknown'`, and the next read tells.
   */
  chooseAnswer(pick) {
    const body = {}
    if (typeof pick?.choice === 'string') body.choice = pick.choice
    if (typeof pick?.effort === 'string') body.effort = pick.effort
    if (pick?.ack === true) body.ack = true
    if (body.choice === undefined && body.effort === undefined) return Promise.resolve({ status: 400, body: { refused: 'body', sent: false } })
    return this.#call('POST', '/answer-with', body)
  }

  // One request of the answer-with verbs, with the same channel, token and limits as /submit.
  #call(method, path, json) {
    const c = this.#channel()
    if (!c?.socket || !c?.token) return Promise.resolve({ status: 0, body: { error: 'no_channel', sent: false } })
    const data = json === undefined ? null : Buffer.from(JSON.stringify(json))
    return new Promise((resolve) => {
      let written = false
      let done = false
      const finish = (r) => {
        if (done) return
        done = true
        resolve(r)
      }
      const headers = { host: 'unfold', authorization: `Bearer ${c.token}`, ...(data ? { 'content-type': 'application/json', 'content-length': data.length } : {}) }
      const req = http.request({ agent: this.agent, socketPath: c.socket, method, path, headers, timeout: this.timeoutMs }, (res) => {
        const chunks = []
        let size = 0
        res.on('data', (b) => {
          size += b.length
          if (size > MAX_ANSWER) {
            finish({ status: 0, body: { error: 'too_large', sent: 'unknown' } })
            return req.destroy()
          }
          chunks.push(b)
        })
        res.on('end', () => {
          let body
          try {
            body = JSON.parse(Buffer.concat(chunks).toString('utf8'))
          } catch {
            body = { error: 'unparseable' }
          }
          if (body === null || typeof body !== 'object' || Array.isArray(body)) body = { error: 'unparseable' }
          finish({ status: res.statusCode, body: res.statusCode === 200 ? body : { ...body, sent: false } })
        })
        res.on('error', () => finish({ status: 0, body: { error: 'transport', sent: 'unknown' } }))
      })
      req.on('timeout', () => req.destroy(new Error('timeout')))
      req.on('error', (e) => {
        const code = e.code?.startsWith('HPE_') ? 'peer' : e.code === 'EPIPE' ? 'closed' : e.code === 'ENOENT' || e.code === 'ECONNREFUSED' ? 'unreachable' : 'transport'
        finish({ status: 0, body: { error: code, sent: code === 'transport' && written ? 'unknown' : false } })
      })
      req.on('finish', () => { written = true })
      req.end(data ?? undefined)
    })
  }

  toJSON() {
    return { endpoint: 'unix socket from the preload', timeoutMs: this.timeoutMs }
  }
}
