import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Server, request } from 'node:http'
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import * as plugin from '../index.js'
import { fakeLlm } from './fake-llm.js'

// The host plugin context the bridge uses: ctx.llm and ctx.effect (whose disposer runs at
// unload), plus a logger.
function fakeContext(llm) {
  const disposers = []
  const logs = []
  return {
    llm,
    logs,
    logger: { info: (...args) => logs.push(args), warn: (...args) => logs.push(args) },
    effect(execute) { disposers.push(execute()) },
    async unload() { for (const dispose of disposers.reverse()) await dispose?.() },
  }
}

const get = (port, key) => new Promise((resolve, reject) => {
  request({ host: '127.0.0.1', port, path: '/v1/models', agent: false, headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${key}` } }, res => {
    let text = ''
    res.on('data', chunk => { text += chunk })
    res.on('end', () => resolve({ status: res.statusCode, text }))
  }).on('error', reject).end()
})

async function until(check) {
  for (let i = 0; i < 200; i++) {
    try { if (check()) return } catch {}
    await new Promise(resolve => setTimeout(resolve, 10))
  }
  throw new Error('timed out')
}

test('the plugin is unfold-bridge, injects llm, and declares nothing else', () => {
  assert.equal(plugin.name, 'unfold-bridge')
  assert.deepEqual(plugin.inject, ['llm'])
})

test('apply serves the bridge, writes its endpoint file with a fresh key, and unload stops both', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-bridge-host-'))
  const was = process.env.XDG_STATE_HOME
  process.env.XDG_STATE_HOME = dir
  const file = join(dir, 'unfold-bridge', 'endpoint.json')
  try {
    const first = fakeContext(fakeLlm())
    plugin.apply(first)
    await until(() => statSync(file))
    const endpoint = JSON.parse(readFileSync(file, 'utf8'))
    assert.equal(statSync(file).mode & 0o777, 0o600)
    assert.equal(endpoint.v, 1)
    assert.equal(endpoint.pid, process.pid)
    assert.match(endpoint.key, /^[A-Za-z0-9_-]{43}$/, '32 random bytes, base64url')
    const res = await get(endpoint.port, endpoint.key)
    assert.equal(res.status, 200)
    assert.equal(JSON.parse(res.text).data[0].id, 'codex/gpt-5.5')
    // the history's assistant turn is made by dsh-llm's own createAssistantMessage
    const body = JSON.stringify({ model: 'deepseek-official/deepseek-v4-flash', messages: [{ role: 'user', content: 'Oi' }, { role: 'assistant', content: 'Oi!' }, { role: 'user', content: 'Tudo bem?' }] })
    const chat = await new Promise((resolve, reject) => {
      request({ host: '127.0.0.1', port: endpoint.port, method: 'POST', path: '/v1/chat/completions', agent: false, headers: { Host: `127.0.0.1:${endpoint.port}`, Authorization: `Bearer ${endpoint.key}`, 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body) } }, res => {
        let text = ''
        res.on('data', chunk => { text += chunk })
        res.on('end', () => resolve({ status: res.statusCode, text }))
      }).on('error', reject).end(body)
    })
    assert.equal(chat.status, 200)
    const [, said] = first.llm.calls[0].messages
    assert.equal(said.role, 'assistant')
    assert.equal(typeof said.id, 'string')
    assert.deepEqual(said.source, { kind: 'model', provider: 'deepseek-official', model: 'deepseek-v4-flash' })
    assert.ok(!JSON.stringify(first.logs).includes(endpoint.key), 'the key is never logged')

    await first.unload()
    assert.throws(() => statSync(file), { code: 'ENOENT' })
    await assert.rejects(get(endpoint.port, endpoint.key), { code: 'ECONNREFUSED' })

    const second = fakeContext(fakeLlm())
    plugin.apply(second)
    await until(() => statSync(file))
    const again = JSON.parse(readFileSync(file, 'utf8'))
    assert.notEqual(again.key, endpoint.key, 'each start makes a new key')
    assert.equal((await get(again.port, endpoint.key)).status, 401, 'the old key no longer opens it')
    await second.unload()
  } finally {
    if (was === undefined) delete process.env.XDG_STATE_HOME
    else process.env.XDG_STATE_HOME = was
    rmSync(dir, { recursive: true, force: true })
  }
})

test('when the endpoint file cannot be written, the listener is closed and the failure is logged', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-bridge-host-'))
  const was = process.env.XDG_STATE_HOME
  // a file where the state folder should be: the folder cannot be made
  writeFileSync(join(dir, 'unfold-bridge'), '')
  process.env.XDG_STATE_HOME = dir
  // the servers apply starts, seen through http.Server's listen
  const started = []
  const listen = Server.prototype.listen
  Server.prototype.listen = function (...args) {
    started.push(this)
    return listen.apply(this, args)
  }
  try {
    const ctx = fakeContext(fakeLlm())
    plugin.apply(ctx)
    await until(() => ctx.logs.some(([format]) => /not serving/.test(format)))
    assert.equal(started.length, 1)
    assert.equal(started[0].listening, false, 'the listener is closed')
    await ctx.unload()
  } finally {
    Server.prototype.listen = listen
    if (was === undefined) delete process.env.XDG_STATE_HOME
    else process.env.XDG_STATE_HOME = was
    rmSync(dir, { recursive: true, force: true })
  }
})

test('an unload before the server is up still leaves nothing behind', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-bridge-host-'))
  const was = process.env.XDG_STATE_HOME
  process.env.XDG_STATE_HOME = dir
  try {
    const ctx = fakeContext(fakeLlm())
    plugin.apply(ctx)
    await ctx.unload()
    await new Promise(resolve => setTimeout(resolve, 50))
    assert.throws(() => statSync(join(dir, 'unfold-bridge', 'endpoint.json')), { code: 'ENOENT' })
  } finally {
    if (was === undefined) delete process.env.XDG_STATE_HOME
    else process.env.XDG_STATE_HOME = was
    rmSync(dir, { recursive: true, force: true })
  }
})
