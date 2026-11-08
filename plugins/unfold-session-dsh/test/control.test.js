// The session-control edge: the composer reaches agentd only through Unfold's control endpoint,
// a Unix socket whose path and token come from the launcher preload's registry. These tests run
// against a fake endpoint (./fake-control.js) and a fake agentd observer on a kernel-picked port.
// Every TCP connect outside the ports a test opened itself is refused before it is made, and
// recorded: the plugin has no TCP path for control and no address of its own.
import test from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import * as llm from '@deepseek-ai/dsh-llm'
import { OWNER_UNAVAILABLE, apply } from '../index.js'
import { HostClient } from '../src/host-client.js'
import { AgentdObserver } from '../src/observer.js'
import { fakeControl, minted, preload } from './fake-control.js'
import { fakeDsh } from './fake-dsh.js'
import { TestWaitTimeout, within } from './wait.js'

const tcp = { allowed: new Set(), refused: [] }
const connect = net.Socket.prototype.connect
net.Socket.prototype.connect = function (...args) {
  const o = Array.isArray(args[0]) ? args[0][0] : typeof args[0] === 'object' ? args[0] : { port: args[0], host: args[1] }
  if (o?.path || tcp.allowed.has(Number(o?.port))) return connect.apply(this, args)
  tcp.refused.push(`${o?.host ?? '?'}:${o?.port ?? '?'}`)
  process.nextTick(() => this.destroy(new Error('the test refused a TCP connect')))
  return this
}

const ROOT = 'unfold-root'
const quiet = { start() {}, stop() {} }
const settle = (ms = 30) => new Promise((resolve) => setTimeout(resolve, ms))
const until = async (cond, ms = 2000) => {
  for (const end = Date.now() + ms; !cond(); ) {
    if (Date.now() > end) throw new TestWaitTimeout(`until: condition not met within ${ms} ms`)
    await settle(10)
  }
}

function guest(producer = 'guest-c1') {
  let seq = 0
  return (kind, fields = {}) => ({ v: 2, id: `${producer}:${++seq}`, producer, seq, source: 'guest', kind, ...fields })
}

/** apply() in a fake DSH, the registry set as the preload would, agentd's Hello seen. */
async function session(t, { channel = (fake) => fake.channel(), sessionId, hello = true } = {}) {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  preload(channel(fake))
  t.after(() => preload(null))
  const lines = []
  const logger = { info: (m) => lines.push(String(m)), warn: (m) => lines.push(String(m)) }
  const dsh = fakeDsh({ root: ROOT, logger })
  apply(dsh.ctx, { sessionId: ROOT }, { env: { AUTOMAGIK_HOST: 'unfold' }, observer: quiet })
  t.after(() => dsh.dispose())
  const service = dsh.provided.unfoldSession
  const g = guest()
  if (hello) service.bridge.ingest(g('hello', { boot_id: 'b1', command_generation: 0, instance: { session_id: sessionId ?? fake.session } }))
  service.bridge.onLink('attached')
  return { fake, dsh, service, bridge: service.bridge, g, lines }
}

/** What agentd broadcasts for an admitted typed turn, fed at once: before the 200 is read. */
function answers(s, reply) {
  s.fake.onAccept = (turnId) => {
    const rid = `r-${turnId}`
    s.bridge.ingest(s.g('command_admitted', { command_id: turnId, generation: 1, status: 'admitted', duplicate: false }))
    s.bridge.ingest(s.g('response_started', { response_id: rid, request_id: turnId }))
    s.bridge.ingest(s.g('agent_delta', { response_id: rid, text: reply }))
    s.bridge.ingest(s.g('agent_done', { response_id: rid, status: 'completed' }))
  }
}

test('the control client has no TCP path and no default: with no registry it connects to nothing', async () => {
  const before = tcp.refused.length
  preload(null)
  const r = await new HostClient().submit('unfold-session-1', 'hello')
  assert.equal(r.status, 0)
  assert.equal(r.body.sent, false)
  assert.equal(tcp.refused.length, before, `no TCP connect was tried (${tcp.refused.slice(before)})`)
})

test('the shipped code names no fixed port and no address', () => {
  const files = ['index.js', ...readdirSync(new URL('../src/', import.meta.url)).filter((f) => f.endsWith('.js')).map((f) => `src/${f}`)]
  for (const file of files) {
    const src = readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')
    // The two fixed ports the first Unfold plugins used, spelled so a scan finds only shipped code.
    for (const port of [77_00, 77_81]) assert.doesNotMatch(src, new RegExp(`\\b${port}\\b`), file)
    assert.doesNotMatch(src, /127\.0\.0\.1|localhost|0\.0\.0\.0/, file)
    assert.doesNotMatch(src, /\bport\s*[:=]\s*\d/i, file)
    assert.doesNotMatch(src, /UNFOLD_CONTROL|UNFOLD_AGENTD/, `${file} reads no channel variable`)
  }
})

test('the socket and the token come from the preload registry only, never from the environment', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  const tokenFile = `${fake.dir}/control.token`
  writeFileSync(tokenFile, `${fake.token}\n`, { mode: 0o600 })
  process.env.UNFOLD_CONTROL_SOCKET = fake.socket
  process.env.UNFOLD_CONTROL_TOKEN_FILE = tokenFile
  t.after(() => { delete process.env.UNFOLD_CONTROL_SOCKET; delete process.env.UNFOLD_CONTROL_TOKEN_FILE; preload(null) })
  preload(null)
  const none = await new HostClient().submit(fake.session, 'hello')
  assert.equal(none.body.sent, false)
  assert.equal(fake.rows.length, 0, 'the environment alone reaches nothing')
  preload(fake.channel({ token: undefined }))
  assert.equal((await new HostClient().submit(fake.session, 'hello')).body.sent, false)
  assert.equal(fake.rows.length, 0, 'a registry without a token reaches nothing')
  rmSync(tokenFile)
  preload(fake.channel())
  const r = await new HostClient().submit(fake.session, 'hello')
  assert.deepEqual([r.status, r.body.accepted], [200, true])
  assert.deepEqual(fake.rows, [{ verb: 'submit', accepted: true, turn_id: 'app:b1:1', refused: null }])
  assert.equal(process.env.UNFOLD_CONTROL_SOCKET, fake.socket, 'the plugin leaves the environment to the preload')
})

test('a text that starts with "/" is an owner command: refused here, nothing is sent', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  preload(fake.channel())
  t.after(() => preload(null))
  for (const text of ['/grant revoke all', '  /effect release', '\n/remember this']) {
    const r = await new HostClient().submit(fake.session, text)
    assert.deepEqual([r.status, r.body.refused, r.body.sent], [400, 'command', false], text)
  }
  assert.equal(fake.rows.length, 0, 'no connection was made for an owner command')
  const later = await new HostClient().submit(fake.session, 'a slash later /grant is an ordinary turn')
  assert.equal(later.status, 200)
})

test('a composer submit in the Unfold Session reaches the endpoint once, with the token, and shows the reply', async (t) => {
  const s = await session(t)
  answers(s, 'Hi.')
  s.dsh.type('hello there')
  await s.dsh.idle()
  assert.deepEqual(s.fake.texts, ['hello there'])
  assert.deepEqual(s.fake.rows, [{ verb: 'submit', accepted: true, turn_id: 'app:b1:1', refused: null }])
  assert.deepEqual(s.dsh.bubbles().map((b) => [b.role, b.text]), [['user', 'hello there'], ['assistant', 'Hi.']])
  assert.match(s.dsh.bubbles()[1].model, /app:b1:1/, 'the bubble names the command id agentd admitted')
})

test('the user bubble is flushed to the Session log before the message is submitted', async (t) => {
  const s = await session(t)
  answers(s, 'Kept.')
  const broadcast = s.fake.onAccept
  let flushedAtSubmit = null
  s.fake.onAccept = (...a) => {
    flushedAtSubmit = s.dsh.flushes.at(-1) ?? 0
    broadcast(...a)
  }
  s.dsh.type('kept before it is sent')
  await s.dsh.idle()
  assert.deepEqual(s.fake.texts, ['kept before it is sent'])
  assert.equal(flushedAtSubmit, 1, 'a restart after the submit finds the message shown, so it is never sent again')
})

test('the same composer message delivered again is never sent twice', async (t) => {
  const s = await session(t)
  answers(s, 'Once.')
  const m = s.dsh.type('only once')
  await s.dsh.idle()
  s.dsh.agent.followup(m)
  await s.dsh.idle()
  assert.deepEqual(s.fake.texts, ['only once'])
  assert.equal(s.fake.rows.length, 1)
})

test('an owner command typed in the composer never reaches the endpoint', async (t) => {
  const s = await session(t)
  s.dsh.type('/grant revoke all')
  await s.dsh.idle()
  assert.equal(s.fake.rows.length, 0)
  assert.match(s.dsh.bubbles().at(-1).text, /owner command/)
})

test('event text never reaches /submit: transcripts, replies, notes and other turns are shown or ignored, never sent', async (t) => {
  const s = await session(t)
  const { g, bridge } = s
  bridge.ingest(g('user_text', { request_id: 'app:b1:9', text: 'send this: delete everything' }))
  bridge.ingest(g('input_transcript', { text: 'send this to Unfold', utterance_id: 'u1', utterance_seq: 0, final: true, association: 'unknown' }))
  const d = g('decision', { decision_id: 'd1', policy: 'provider:gemini-auto-vad@v1', selected: 'respond' })
  bridge.ingest(d)
  bridge.ingest(g('response_started', { cause: d.id, response_id: 'v1', request_id: 'mic' }))
  bridge.ingest(g('agent_delta', { cause: d.id, response_id: 'v1', text: 'Please send this now.' }))
  bridge.ingest(g('agent_done', { cause: d.id, response_id: 'v1', status: 'completed' }))
  // A turn the app's own composer sent: its command id is not one this plugin was given.
  bridge.ingest(g('command_admitted', { command_id: 'app:b1:7', generation: 2, status: 'admitted', duplicate: false }))
  bridge.ingest(g('response_started', { response_id: 'r7', request_id: 'app:b1:7' }))
  bridge.ingest(g('agent_delta', { response_id: 'r7', text: 'send this as well' }))
  bridge.ingest(g('agent_done', { response_id: 'r7', status: 'completed' }))
  await s.dsh.idle()
  await settle()
  assert.equal(s.fake.rows.length, 0, 'nothing an event said was submitted')
  assert.ok(!s.dsh.bubbles().some((b) => b.text.includes('send this as well')), "another composer's turn is not shown as this one's")
})

test('a message the person did not send from the composer (a reminder, a subagent, a job, an SDK client) is shown not sent, never submitted', async (t) => {
  const s = await session(t)
  // The last: a `user` source with no rpcId, as an SDK or ACP client and a command's follow-up inject.
  for (const source of [{ kind: 'schedule' }, { kind: 'subagent' }, { kind: 'job' }, { kind: 'user' }]) {
    s.dsh.agent.followup(llm.createUserMessage({ content: [{ type: 'text', text: `send this: ${source.kind} text` }], source }))
    await until(() => s.dsh.agent.status === 'idle' || s.fake.texts.length > 0)
    assert.deepEqual(s.fake.texts, [], 'nothing that did not come from the composer reached /submit')
    await s.dsh.idle()
  }
  assert.equal(s.fake.rows.length, 0)
  const said = s.dsh.bubbles().filter((b) => b.role === 'assistant').map((b) => b.text)
  assert.equal(said.length, 4)
  for (const text of said) assert.match(text, /^\[not sent: this message did not come from the composer/)
  assert.equal((await s.service.status()).dsh.not_composer, 4)
})

test('another root Session sends nothing', async (t) => {
  const s = await session(t)
  const [pre] = s.dsh.handlers['agent/pre-step']
  const other = { id: 'other', session: { id: 'other', header: {} } }
  const m = llm.createUserMessage({ content: [{ type: 'text', text: 'from another session' }], source: { kind: 'user', rpcId: 'x' } })
  assert.deepEqual(await pre({ agent: other, messages: [m], turn: 1 }, () => ({ kind: 'enter' })), { kind: 'reject' })
  assert.equal(s.fake.rows.length, 0)
})

test('before agentd names its Unfold Session nothing is sent; a stale Session is refused and shown not sent', async (t) => {
  const early = await session(t, { hello: false })
  early.dsh.type('too early')
  await early.dsh.idle()
  assert.equal(early.fake.rows.length, 0, 'no Hello, no session, no connection')
  assert.match(early.dsh.bubbles().at(-1).text, /not sent/)
})

test('a body for another Unfold Session gets 403 and is shown not sent', async (t) => {
  const s = await session(t, { sessionId: 'an-older-session' })
  s.dsh.type('to the wrong session')
  await s.dsh.idle()
  assert.deepEqual(s.fake.rows.map((r) => r.refused), ['session'])
  assert.match(s.dsh.bubbles().at(-1).text, /not sent/)
})

test('without the right token nothing is accepted; a 503 is shown not sent and is not retried', async (t) => {
  const wrong = await session(t, { channel: (fake) => fake.channel({ token: 'f'.repeat(64) }) })
  wrong.dsh.type('with a wrong token')
  await wrong.dsh.idle()
  assert.deepEqual(wrong.fake.rows.map((r) => [r.accepted, r.refused]), [[false, 'token']])
  assert.match(wrong.dsh.bubbles().at(-1).text, /not sent/)
})

test('a 503 means the chat did not take the turn: shown not sent, never retried', async (t) => {
  const s = await session(t)
  s.fake.unavailable = true
  s.dsh.type('while unavailable')
  await s.dsh.idle()
  await settle(100)
  assert.deepEqual(s.fake.rows.map((r) => r.refused), ['unavailable'])
  assert.match(s.dsh.bubbles().at(-1).text, /not sent/)
})

test('a refused peer (one line, no HTTP) is shown not sent', async (t) => {
  const s = await session(t)
  s.fake.refusePeer = true
  s.dsh.type('from a refused peer')
  await s.dsh.idle()
  assert.match(s.dsh.bubbles().at(-1).text, /not sent: the control endpoint refused this process/)
})

test('a refused peer whose connection closes before the request is read is shown not sent, never unknown', async (t) => {
  const big = 'x'.repeat(7000)
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  fake.refusePeer = 'close'
  preload(fake.channel())
  t.after(() => preload(null))
  for (let i = 0; i < 5; i++) {
    const r = await new HostClient().submit(fake.session, `${big} ${i}`)
    assert.deepEqual([r.status, r.body.sent], [0, false], JSON.stringify(r.body))
  }
})

test('a 200 that is not exactly {accepted: true, turn_id} is unknown, never shown not sent', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  preload(fake.channel())
  t.after(() => preload(null))
  for (const body of ['not json', JSON.stringify({accepted:true,turn_id:''}), JSON.stringify({accepted:true,turn_id:' \t\n '}), '{"accepted":true,"turn_id":7}', '{"accepted":false}', '{"turn_id":"app:b1:1"}']) {
    fake.answer = (conn) => conn.end(`HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${body}`)
    const r = await new HostClient().submit(fake.session, 'a 200 the client cannot read')
    assert.deepEqual([r.status, r.body.sent], [200, 'unknown'], body)
  }
})

test('an answer body over 64 KiB is cut off: the request ends as unknown, the stream is not read on', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  preload(fake.channel())
  t.after(() => preload(null))
  let streamed = 0
  fake.answer = (conn) => {
    conn.write(`HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${10 * 1024 * 1024}\r\n\r\n`)
    const chunk = Buffer.alloc(16 * 1024, 0x20)
    const more = () => {
      while (!conn.destroyed && streamed < 10 * 1024 * 1024) {
        streamed += chunk.length
        if (!conn.write(chunk)) return conn.once('drain', more)
      }
    }
    conn.on('error', () => {})
    more()
  }
  const r = await within(new HostClient({ timeoutMs: 60_000 }).submit(fake.session, 'an endless answer'), 10_000, 'a capped answer')
  assert.deepEqual([r.status, r.body.error, r.body.sent], [0, 'too_large', 'unknown'])
  assert.ok(streamed < 2 * 1024 * 1024, `read on to ${streamed} bytes`)
})

test('a connection closed before the request is read, with no refusal line, is not sent and names no refused process', async (t) => {
  const big = 'x'.repeat(7000)
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  fake.refusePeer = 'silent'
  preload(fake.channel())
  t.after(() => preload(null))
  const seen = new Set()
  for (let i = 0; i < 5; i++) {
    const r = await new HostClient().submit(fake.session, `${big} ${i}`)
    seen.add(r.body.error)
    assert.equal(r.status, 0)
    assert.notEqual(r.body.error, 'peer', 'nothing said this process was refused')
    if (r.body.error === 'closed') assert.equal(r.body.sent, false)
  }
  assert.ok(seen.has('closed'), `closed before the request was read: ${[...seen]}`)
})

test('the token never reaches a log line, the status, a bubble or an error', async (t) => {
  const s = await session(t)
  answers(s, 'Fine.')
  s.dsh.type('one')
  await s.dsh.idle()
  s.fake.refusePeer = true
  s.dsh.type('two')
  await s.dsh.idle()
  s.fake.refusePeer = false
  await s.fake.close()
  s.dsh.type('three')
  await s.dsh.idle()
  const seen = [
    ...s.lines,
    JSON.stringify(await s.service.status()),
    JSON.stringify(s.dsh.events),
    JSON.stringify(s.service),
    JSON.stringify(new HostClient()),
  ].join('\n')
  for (const token of minted) assert.ok(!seen.includes(token), 'a minted token appears in what the plugin shows or logs')
  assert.ok(s.lines.length > 0, 'the plugin did log')
})

test('status() names the host owner as unavailable, in the one shape the events and settings plugins read', async (t) => {
  const s = await session(t)
  const { owner } = await s.service.status()
  // Exactly `{unavailable: <reason>}`: an object, `unavailable` a non-empty string, no `host`.
  assert.deepEqual(owner, { unavailable: OWNER_UNAVAILABLE })
  assert.ok(typeof owner.unavailable === 'string' && owner.unavailable.length > 0)
  assert.ok(!('host' in owner), 'no host facts are claimed')
  assert.equal(s.fake.rows.length, 0, 'reading the status sends nothing')
})

test('no DSH route of the plugin leads to /submit', async (t) => {
  const s = await session(t)
  assert.ok(s.dsh.routes.length > 0)
  for (const route of s.dsh.routes) {
    assert.ok(route.path.startsWith('/api/unfold-session/'), route.path)
    for (const method of route.methods.filter((m) => m !== 'GET')) {
      const body = JSON.stringify({ text: 'hello', call: 'start', muted: true, session: s.fake.session })
      await route.fetch(new Request(`http://dsh${route.path}`, { method, body, headers: { 'content-type': 'application/json' } }))
    }
  }
  assert.equal(s.fake.rows.length, 0)
})

test('a tool the instance spawns sees neither control variable', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  // The launcher preload: read the three variables once, delete them, keep the values in memory.
  process.env.UNFOLD_CONTROL_SOCKET = fake.socket
  process.env.UNFOLD_CONTROL_TOKEN_FILE = `${fake.dir}/control.token`
  process.env.UNFOLD_AGENTD_OBSERVER = `${fake.dir}/observer`
  const kept = { socket: process.env.UNFOLD_CONTROL_SOCKET, token: fake.token, observerFile: process.env.UNFOLD_AGENTD_OBSERVER }
  for (const name of ['UNFOLD_CONTROL_SOCKET', 'UNFOLD_CONTROL_TOKEN_FILE', 'UNFOLD_AGENTD_OBSERVER']) delete process.env[name]
  preload(kept)
  t.after(() => preload(null))
  const dsh = fakeDsh({ root: ROOT })
  apply(dsh.ctx, { sessionId: ROOT }, { env: { AUTOMAGIK_HOST: 'unfold' }, observer: quiet })
  t.after(() => dsh.dispose())
  await new HostClient().submit(fake.session, 'hello')
  const child = spawnSync(process.execPath, ['-e', 'process.stdout.write(JSON.stringify(Object.keys(process.env).filter((k) => k.startsWith("UNFOLD_"))))'], { env: process.env, encoding: 'utf8' })
  assert.equal(child.stdout, '[]')
  assert.ok(!child.stdout.includes(fake.token))
})

// ── the observer: agentd's address from the observer file, read at every connect ──

/** A fake agentd observer endpoint on a kernel-picked port: Hello, then the role it grants. */
async function fakeAgentd(t, { grant = 'observer' } = {}) {
  const conns = []
  const server = net.createServer((c) => {
    conns.push(c)
    c.write(`${JSON.stringify({ v: 2, id: 'g:0', producer: 'g', seq: 0, source: 'guest', kind: 'hello', protocol: 2, boot_id: 'b', instance: { session_id: 's' } })}\n`)
    c.on('data', (b) => {
      const attach = JSON.parse(b.toString().trim().split('\n')[0])
      c.attached = attach
      c.write(`${JSON.stringify({ v: 2, id: 'g:1', producer: 'g', seq: 1, source: 'guest', kind: 'ready', attachment_id: attach.attachment_id, role: grant })}\n`)
    })
    c.on('error', () => {})
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const port = server.address().port
  tcp.allowed.add(port)
  t.after(() => { for (const c of conns) c.destroy(); server.close(); tcp.allowed.delete(port) })
  return { conns, address: `127.0.0.1:${port}`, port }
}


test('the observer reads its address from the observer file at every connect, and attaches as observer only', async (t) => {
  const fake = await fakeControl(`g6-${t.name}`)
  t.after(() => within(fake.close(), 5000, 'the fake endpoint close'))
  const file = `${fake.dir}/observer`
  const a = await fakeAgentd(t)
  const b = await fakeAgentd(t)
  writeFileSync(file, `${a.address}\n`, { mode: 0o600 })
  preload(fake.channel())
  t.after(() => preload(null))
  const states = []
  const dsh = fakeDsh({ root: ROOT })
  apply(dsh.ctx, { sessionId: ROOT }, { env: { AUTOMAGIK_HOST: 'unfold' } })
  t.after(() => dsh.dispose())
  const bridge = dsh.provided.unfoldSession.bridge
  bridge.onChange((kind, link) => { if (kind === 'link') states.push(link.state) })
  await until(() => bridge.link.state === 'attached')
  assert.equal(a.conns[0].attached.role, 'observer')
  assert.equal(bridge.link.session_id, 's')
  // The app withdraws the file, then publishes a new agentd's address: the old one is never dialled.
  rmSync(file)
  const before = tcp.refused.length
  a.conns[0].destroy()
  await until(() => bridge.link.state !== 'attached')
  await settle(600)
  assert.equal(a.conns.length, 1, 'no file: no dial, not even to the old address')
  writeFileSync(file, `${b.address}\n`, { mode: 0o600 })
  await until(() => bridge.link.state === 'attached', 8000)
  assert.equal(b.conns.length, 1)
  assert.equal(tcp.refused.length, before)
})

test('the observer drops any grant but observer, and never dials an address that is not loopback', async (t) => {
  const c = await fakeAgentd(t, { grant: 'controller' })
  const states = []
  let address = c.address
  const o = new AgentdObserver({ address: () => address, onState: (s, d) => states.push([s, d]) }).start()
  t.after(() => o.stop())
  await until(() => states.some(([s, d]) => s === 'error' && /controller/.test(d)))
  assert.ok(!states.some(([s]) => s === 'attached'))
  address = '192.0.2.1:9'
  const before = tcp.refused.length
  await settle(1200)
  assert.equal(tcp.refused.length, before, 'a non-loopback address is never dialled')
})
