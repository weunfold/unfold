// A fake of Unfold's session-control endpoint for the tests: the wire and the order of checks
// of the channel contract, over a Unix socket in a 0700 dir under /tmp/ufd-<uid>/. Route
// (POST /submit, GET and POST /answer-with, else 404), bearer token (401), body (400 body), a text that starts with
// "/" (400 command), the Unfold Session (403), then the submit: 200 with the turn's command id,
// or 503 while `unavailable` is set. The peer check (one line, no HTTP) is `refusePeer`. One
// journal row per call, never the text. The answer-with verbs keep their own journal
// (`answerRows`, ids only) and state (`facts`: what GET returns; `replying`: a pick waits
// for the reply in progress), so the submit journal reads as before. `close()` removes the dir by the path it recorded.
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { lstatSync, mkdirSync, rmSync } from 'node:fs'
import net from 'node:net'

const MAX_REQUEST = 64 * 1024
const SAY_MAX = 8000
const REASONS = { 200: 'OK', 400: 'Bad Request', 401: 'Unauthorized', 403: 'Forbidden', 404: 'Not Found', 409: 'Conflict', 503: 'Service Unavailable' }

/** Facts like the app's: Granite on this Mac, DeepSeek direct with its levels, one bridge model. */
export const answerFacts = () => ({
  rev: 1,
  chosen: 'deepseek',
  answering: 'deepseek',
  why: null,
  effort: { chosen: 'auto', levels: ['auto', 'off', 'low', 'high', 'max'] },
  options: [
    { id: 'granite', name: 'Granite 4.1 3B', where: 'mac', provider: null, memories: true, confirm: false, efforts: false },
    { id: 'deepseek', name: 'DeepSeek V4.1 Flash', where: 'direct', provider: 'DeepSeek', memories: true, confirm: false, efforts: true },
    { id: 'openai-codex/gpt-5.5', name: 'GPT-5.5', where: 'automagik', provider: 'ChatGPT', memories: false, confirm: true, efforts: false },
  ],
})

// The answer-with verbs, after the token: GET the facts; POST a pick `{choice?, effort?, ack?}`.
function answerWith(fake, method, body) {
  const { facts } = fake
  if (method === 'GET') return [200, facts]
  const b = body
  const keys = b && typeof b === 'object' && !Array.isArray(b) ? Object.keys(b) : null
  if (!keys || keys.length === 0 || keys.some((k) => !['choice', 'effort', 'ack'].includes(k))
    || (b.choice !== undefined && typeof b.choice !== 'string') || (b.effort !== undefined && typeof b.effort !== 'string')
    || (b.ack !== undefined && typeof b.ack !== 'boolean') || (b.choice === undefined && b.effort === undefined)) return [400, { refused: 'body' }]
  const option = b.choice === undefined ? null : facts.options.find((o) => o.id === b.choice)
  if (b.choice !== undefined && !option) return [409, { refused: 'not_offered' }]
  if (b.effort !== undefined && !facts.effort.levels.includes(b.effort)) return [409, { refused: 'not_offered' }]
  if (option?.confirm && b.ack !== true) return [409, { refused: 'needs_ack' }]
  facts.rev++
  if (b.effort !== undefined) facts.effort.chosen = b.effort
  const afterReply = option !== null && option.id !== facts.chosen && fake.replying === true
  if (option) {
    facts.chosen = option.id
    // Waiting for the reply: the agent answers as before until it ends.
    if (!afterReply) facts.answering = option.id
  }
  return [200, { rev: facts.rev, after_reply: afterReply }]
}

/** Every channel value a test makes, so a scan of logs and statuses can look for each. */
export const minted = []

export async function fakeControl(testId, { session = 'unfold-session-1', boot = 'b1' } = {}) {
  const uid = process.getuid()
  const top = process.env.TEST_SOCKET_ROOT || `/tmp/ufd-${uid}`
  mkdirSync(top, { recursive: true, mode: 0o700 })
  const dir = `${top}/${createHash('sha256').update(testId).digest('hex').slice(0, 8)}`
  rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { mode: 0o700 })
  const st = lstatSync(dir)
  if (!st.isDirectory() || (st.mode & 0o777) !== 0o700 || st.uid !== uid) throw new Error(`fake control: ${dir} is not a 0700 dir of this user`)
  const socket = `${dir}/control.sock`
  const token = randomBytes(32).toString('hex')
  minted.push(token)
  const rows = []
  const texts = []
  const answerRows = []
  const fake = { dir, socket, token, session, rows, texts, unavailable: false, refusePeer: false, turns: 0, facts: answerFacts(), answerRows, replying: false }
  const reply = (conn, row, code, body) => {
    rows.push(row)
    const data = JSON.stringify(body)
    conn.end(`HTTP/1.1 ${code} ${REASONS[code]}\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(data)}\r\nConnection: close\r\n\r\n${data}`)
  }
  const conns = new Set()
  const server = net.createServer((conn) => {
    conns.add(conn)
    conn.on('close', () => conns.delete(conn))
    const row = { verb: '-', accepted: false, turn_id: null, refused: null }
    if (fake.refusePeer) {
      rows.push({ ...row, refused: 'peer' })
      // 'close' shuts the socket at once, unread, as an endpoint that drops the connection does.
      if (fake.refusePeer === 'close') {
        conn.write('refused: not the instance child\n')
        conn.destroy()
      }
      // 'silent' closes at once with no line, as an endpoint that stops mid-request does.
      else if (fake.refusePeer === 'silent') conn.destroy()
      else conn.end('refused: not the instance child\n')
      return
    }
    let buf = Buffer.alloc(0)
    conn.on('data', (chunk) => {
      buf = Buffer.concat([buf, chunk])
      if (buf.length > MAX_REQUEST) return reply(conn, { ...row, refused: 'request' }, 400, { refused: 'request' })
      const end = buf.indexOf('\r\n\r\n')
      if (end < 0) return
      const [line, ...headers] = buf.subarray(0, end).toString('latin1').split('\r\n')
      const parts = line.split(' ')
      if (parts.length !== 3 || parts[2] !== 'HTTP/1.1') return reply(conn, { ...row, refused: 'request' }, 400, { refused: 'request' })
      const head = {}
      let lengths = 0
      for (const h of headers) {
        const i = h.indexOf(':')
        const k = h.slice(0, i).trim().toLowerCase()
        if (k === 'content-length') lengths++
        head[k] = h.slice(i + 1).trim()
      }
      if (parts[1] === '/answer-with' && (parts[0] === 'GET' || parts[0] === 'POST')) {
        const answerRow = { verb: 'answer_with', method: parts[0], refused: null }
        const answer = (code, value) => {
          const data = JSON.stringify(value)
          answerRows.push({ ...answerRow, status: code, ...(value?.refused ? { refused: value.refused } : {}) })
          conn.end(`HTTP/1.1 ${code} ${REASONS[code]}\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(data)}\r\nConnection: close\r\n\r\n${data}`)
        }
        const auth = head.authorization ?? ''
        const given = Buffer.from(auth.startsWith('Bearer ') ? auth.slice(7) : '')
        if (given.length !== token.length || !timingSafeEqual(given, Buffer.from(token))) return answer(401, { refused: 'token' })
        if (fake.unavailable) return answer(503, { refused: 'unavailable' })
        if (parts[0] === 'GET') return answer(...answerWith(fake, 'GET'))
        const n = Number(head['content-length'])
        if (lengths !== 1 || !Number.isInteger(n)) return answer(400, { refused: 'body' })
        const body = buf.subarray(end + 4)
        if (body.length < n) return
        let b = null
        try { b = JSON.parse(body.subarray(0, n).toString('utf8')) } catch {}
        return answer(...answerWith(fake, 'POST', b))
      }
      if (parts[0] !== 'POST' || parts[1] !== '/submit') return reply(conn, { ...row, verb: 'unknown', refused: 'not_found' }, 404, { refused: 'not_found' })
      row.verb = 'submit'
      const auth = head.authorization ?? ''
      const given = Buffer.from(auth.startsWith('Bearer ') ? auth.slice(7) : '')
      if (given.length !== token.length || !timingSafeEqual(given, Buffer.from(token))) return reply(conn, { ...row, refused: 'token' }, 401, { refused: 'token' })
      const n = Number(head['content-length'])
      if (lengths !== 1 || 'transfer-encoding' in head || !Number.isInteger(n)) return reply(conn, { ...row, refused: 'body' }, 400, { refused: 'body' })
      const body = buf.subarray(end + 4)
      if (body.length < n) return
      let b = null
      try { b = JSON.parse(body.subarray(0, n).toString('utf8')) } catch {}
      const keys = b && typeof b === 'object' ? Object.keys(b).sort().join() : ''
      const text = typeof b?.text === 'string' ? b.text.trim() : ''
      if (keys !== 'session,text' || typeof b.session !== 'string' || !text || [...text].length > SAY_MAX) return reply(conn, { ...row, refused: 'body' }, 400, { refused: 'body' })
      if (text.startsWith('/')) return reply(conn, { ...row, refused: 'command' }, 400, { refused: 'command' })
      if (b.session !== fake.session) return reply(conn, { ...row, refused: 'session' }, 403, { refused: 'session' })
      if (fake.unavailable) return reply(conn, { ...row, refused: 'unavailable' }, 503, { accepted: false, refused: 'unavailable' })
      const turnId = `app:${boot}:${++fake.turns}`
      texts.push(text)
      fake.onAccept?.(turnId, text)
      // `answer(conn)`: a test's own bytes for the accepted submit (an odd 200, an endless body).
      if (fake.answer) {
        rows.push({ ...row, accepted: true, turn_id: turnId })
        return fake.answer(conn)
      }
      reply(conn, { ...row, accepted: true, turn_id: turnId }, 200, { accepted: true, turn_id: turnId })
    })
    conn.on('error', () => {})
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(socket, resolve)
  })
  fake.channel = (extra = {}) => ({ observerFile: `${dir}/observer`, socket, token, ...extra })
  // A refused peer's connection is never read, so it never sees the client's close: it would
  // hold server.close open.
  fake.close = async () => {
    const closed = new Promise((resolve) => server.close(resolve))
    for (const conn of conns) conn.destroy()
    await closed
    rmSync(dir, { recursive: true, force: true })
  }
  return fake
}

/** The launcher preload's registry, as DSH main holds it: frozen and not enumerable. */
export const CHANNEL = Symbol.for('unfold.channel')
export function preload(value) {
  delete globalThis[CHANNEL]
  if (value) Object.defineProperty(globalThis, CHANNEL, { value: Object.freeze(value), enumerable: false, configurable: true })
}
