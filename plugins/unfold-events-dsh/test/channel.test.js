// The observer channel in Unfold's own instance: agentd's address reaches this plugin only as
// the path of a file Unfold publishes while its chat holds agentd's lease, handed over by the
// launcher preload's registry (globalThis[Symbol.for('unfold.channel')]), never by env. The
// file is read at every connect and never cached; with no channel, or no file, nothing is dialled.
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, readdirSync, rmSync, unlinkSync, writeFileSync } from 'node:fs'
import net from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { SessionProjectionRegistry } from '@deepseek-ai/dsh-session-projection'
import * as plugin from '../index.js'
import { AgentdObserver } from '../src/observer.js'

const CHANNEL = Symbol.for('unfold.channel')
const dir = mkdtempSync(join(tmpdir(), 'unfold-events-channel-'))
after(() => rmSync(dir, { recursive: true, force: true }))
let files = 0
const observerFile = (text) => {
  const path = join(dir, `observer-${++files}`)
  if (text !== undefined) writeFileSync(path, `${text}\n`, { mode: 0o600 })
  return path
}
const until = async (what, ok, ms = 3000) => {
  const end = Date.now() + ms
  while (!ok()) {
    if (Date.now() > end) throw new Error(`${what} within ${ms} ms`)
    await new Promise((r) => setTimeout(r, 10))
  }
}

/** Counts every outbound dial; the plugin's only network call is net.connect. */
const dials = []
const realConnect = net.connect
net.connect = (...args) => {
  dials.push(args[0])
  return realConnect(...args)
}
after(() => {
  net.connect = realConnect
})

/** A stand-in agentd: Hello, then Ready with the role asked for (or `grant`), on a kernel-picked port. */
async function fakeAgentd({ grant } = {}) {
  const attaches = []
  const sockets = new Set()
  const server = net.createServer((sock) => {
    sockets.add(sock)
    sock.on('close', () => sockets.delete(sock))
    sock.write(`${JSON.stringify({ v: 2, id: 'g:~', producer: 'g', seq: 0, t_ns: 1, source: 'guest', kind: 'hello', protocol: 2 })}\n`)
    let buf = ''
    sock.on('data', (chunk) => {
      buf += chunk
      for (let nl; (nl = buf.indexOf('\n')) >= 0; buf = buf.slice(nl + 1)) {
        const env = JSON.parse(buf.slice(0, nl))
        attaches.push(env)
        sock.write(`${JSON.stringify({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 2, source: 'guest', kind: 'ready', attachment_id: env.attachment_id, role: grant ?? env.role })}\n`)
      }
    })
  })
  server.listen(0, '127.0.0.1')
  await until('the listening socket', () => server.listening, 2000)
  return {
    address: `127.0.0.1:${server.address().port}`,
    attaches,
    drop: () => sockets.forEach((s) => s.destroy()),
    close: () => new Promise((r) => { sockets.forEach((s) => s.destroy()); server.close(r) }),
  }
}

const SHIPPED = ['index.js', ...readdirSync(new URL('../src/', import.meta.url), { recursive: true }).filter((f) => /\.(js|jsx)$/.test(f)).map((f) => `src/${f}`), 'client.js']

test('no shipped file names a fixed port or a default address', () => {
  // The fixed observer and host-owner ports of the first Unfold plugins, spelled so that a scan
  // for them finds only shipped code.
  const ports = [77_00, 77_81].map((p) => new RegExp(`\\b${p}\\b`))
  for (const file of SHIPPED) {
    const src = readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')
    for (const port of ports) assert.doesNotMatch(src, port, `${file} names a fixed port`)
    assert.doesNotMatch(src, /['"`](?:127\.0\.0\.1|localhost|::1)['"`]/, `${file} names a default address`)
    assert.doesNotMatch(src, /\bport\b[^\n]*\.default\(|\bport\s*[:=]\s*\d/i, `${file} has a default port`)
    assert.doesNotMatch(src, /process\.env\.UNFOLD_/, `${file} reads a channel variable from env`)
  }
})

test('an observer with no channel never dials', async () => {
  const states = []
  const before = dials.length
  const o = new AgentdObserver({ onState: (s, d) => states.push([s, d]) }).start()
  await new Promise((r) => setTimeout(r, 300))
  o.stop()
  assert.equal(dials.length, before)
  assert.equal(states[0][0], 'unconfigured')
  assert.match(states[0][1], /no observer channel/)
})

test('an absent observer file waits without dialling, and attaches once the file appears', async () => {
  const agentd = await fakeAgentd()
  const path = observerFile()
  const states = []
  const before = dials.length
  const o = new AgentdObserver({ observerFile: path, onState: (s, d) => states.push([s, d]) }).start()
  try {
    await until('an unpublished state', () => states.some(([s]) => s === 'unpublished'))
    await new Promise((r) => setTimeout(r, 600)) // past the first two retries
    assert.equal(dials.length, before, 'nothing dialled while the file is absent')
    assert.ok(states.every(([s, d]) => s !== 'unpublished' || /not published/.test(d)))
    assert.ok(!states.some(([s]) => s === 'waiting'), 'an absent file is not a lost link')
    writeFileSync(path, `${agentd.address}\n`, { mode: 0o600 })
    await until('an observer attachment', () => states.some(([s]) => s === 'attached'), 8000)
    assert.equal(agentd.attaches[0].role, 'observer')
  } finally {
    o.stop()
    await agentd.close()
  }
})

for (const [label, text] of [
  ['a hostname', 'localhost:1234'],
  ['a non-loopback address', '192.0.2.7:1234'],
  ['an address with no port', '127.0.0.1'],
  ['port 0', '127.0.0.1:0'],
  ['two lines', '127.0.0.1:1234\n127.0.0.1:1235'],
]) {
  test(`an observer file holding ${label} is refused in one line and never dialled`, async () => {
    const states = []
    const before = dials.length
    const o = new AgentdObserver({ observerFile: observerFile(text), onState: (s, d) => states.push([s, d]) }).start()
    await until('a refusal', () => states.some(([s]) => s === 'refused'))
    o.stop()
    assert.equal(dials.length, before)
    const [, detail] = states.find(([s]) => s === 'refused')
    assert.doesNotMatch(detail, /\n/)
    assert.match(detail, /not a literal loopback address with a port/)
  })
}

test('the file is read again at every reconnect: a new agentd is followed, the old port is never dialled again', async () => {
  const first = await fakeAgentd()
  const second = await fakeAgentd()
  const path = observerFile(first.address)
  const states = []
  const o = new AgentdObserver({ observerFile: path, onState: (s, d) => states.push([s, d]) }).start()
  try {
    await until('the first attachment', () => first.attaches.length === 1 && states.some(([s]) => s === 'attached'))
    writeFileSync(path, `${second.address}\n`, { mode: 0o600 })
    const from = dials.length
    first.drop()
    await until('the second attachment', () => second.attaches.length === 1, 8000)
    const port = (a) => Number(a.split(':')[1])
    assert.ok(dials.slice(from).every((d) => d.port === port(second.address)), 'every later dial went to the published address')
    assert.equal(first.attaches.length, 1)
  } finally {
    o.stop()
    await first.close()
    await second.close()
  }
})

test('a file withdrawn after an attach is never dialled again, not even at the last-known address', async () => {
  // The app withdraws the file before it lets agentd's lease go. The old port may then belong to
  // any process of the same user, so the last address read is never reused.
  const squatter = await fakeAgentd()
  const path = observerFile(squatter.address)
  const states = []
  const o = new AgentdObserver({ observerFile: path, onState: (s, d) => states.push([s, d]) }).start()
  try {
    await until('the attachment', () => squatter.attaches.length === 1 && states.some(([s]) => s === 'attached'))
    unlinkSync(path)
    const from = dials.length
    const seen = states.length
    squatter.drop()
    await new Promise((r) => setTimeout(r, 1000)) // past the first two retries
    assert.equal(dials.length, from, 'nothing dialled once the file is withdrawn')
    assert.equal(squatter.attaches.length, 1, 'the old address was never attached to again')
    const later = states.slice(seen).map(([s]) => s)
    assert.ok(!later.some((s) => s === 'connecting' || s === 'attached'), `states after the drop: ${later}`)
    assert.equal(o.state, 'unpublished')
  } finally {
    o.stop()
    await squatter.close()
  }
})

test('a file that turns unreadable after an attach is refused, and the last-known address is never dialled', async () => {
  // A torn write or a bad value is not the address it replaced: refused, not a reason to fall back.
  const squatter = await fakeAgentd()
  const path = observerFile(squatter.address)
  const states = []
  const o = new AgentdObserver({ observerFile: path, onState: (s, d) => states.push([s, d]) }).start()
  try {
    await until('the attachment', () => squatter.attaches.length === 1 && states.some(([s]) => s === 'attached'))
    writeFileSync(path, 'localhost:1234\n', { mode: 0o600 })
    const from = dials.length
    const seen = states.length
    squatter.drop()
    await new Promise((r) => setTimeout(r, 1000)) // past the first two retries
    assert.equal(dials.length, from, 'nothing dialled once the file holds a bad value')
    assert.equal(squatter.attaches.length, 1, 'the old address was never attached to again')
    const later = states.slice(seen).map(([s]) => s)
    assert.ok(!later.some((s) => s === 'connecting' || s === 'attached'), `states after the drop: ${later}`)
    assert.equal(o.state, 'refused')
  } finally {
    o.stop()
    await squatter.close()
  }
})

test('a grant other than observer drops the link', async () => {
  const agentd = await fakeAgentd({ grant: 'controller' })
  const states = []
  const o = new AgentdObserver({ observerFile: observerFile(agentd.address), onState: (s, d) => states.push([s, d]) }).start()
  try {
    await until('an error', () => states.some(([s, d]) => s === 'error' && /granted controller/.test(d)))
    assert.ok(!states.some(([s]) => s === 'attached'))
  } finally {
    o.stop()
    await agentd.close()
  }
})

/** Load the plugin in a fresh Cordis context with `channel` as the preload's registry (or none). */
async function load(channel) {
  if (channel) Object.defineProperty(globalThis, CHANNEL, { value: Object.freeze({ ...channel }), configurable: true, enumerable: false })
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  ctx.provide('webServer', { register: () => () => {} })
  const fiber = ctx.plugin(plugin, { presence: { host: false }, transport: 'webServer' })
  await fiber
  await new Promise((r) => setImmediate(r))
  return {
    service: ctx.get('unfoldEvents'),
    dispose: async () => {
      await fiber.dispose()
      delete globalThis[CHANNEL]
    },
  }
}

test("the plugin takes the observer file from the preload's registry only, never from env", async () => {
  const agentd = await fakeAgentd()
  process.env.UNFOLD_AGENTD_OBSERVER = observerFile(agentd.address)
  const before = dials.length
  const none = await load(null)
  try {
    await until('the link state', () => none.service.snapshot().link.state === 'unconfigured')
    assert.equal(dials.length, before, 'an env value left behind is never used')
  } finally {
    delete process.env.UNFOLD_AGENTD_OBSERVER
    await none.dispose()
  }
  const live = await load({ observerFile: observerFile(agentd.address), socket: join(dir, 'control.sock'), token: 'not-used-by-this-plugin' })
  try {
    await until('the plugin attaching', () => live.service.snapshot().link.state === 'attached')
    assert.equal(agentd.attaches.at(-1).role, 'observer')
  } finally {
    await live.dispose()
    await agentd.close()
  }
})
