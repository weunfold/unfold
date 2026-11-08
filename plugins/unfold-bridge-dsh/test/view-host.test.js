import { test } from 'node:test'
import assert from 'node:assert/strict'
import { request } from 'node:http'
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import * as plugin from '../index.js'
import { fakeLlm } from './fake-llm.js'

// The host context with DSH's session services: `inject(names, fn)` runs the work view's child
// with fakes of the services it reads, and records the hooks and routes it registers.
function fakeHost() {
  const disposers = []
  const hooks = new Map()
  const routes = []
  const prompts = []
  const sessions = new Map([['sess-x', { id: 'sess-x' }]])
  const child = {
    logger: { info() {}, warn() {} },
    on(name, fn) { hooks.set(name, fn) },
    effect(execute) { disposers.push(execute()) },
    sessionController: {
      list: async () => ({ items: [{ sessionId: 'sess-x', cwd: '/work/site', running: false, updatedAt: 1 }, { sessionId: 'sess-quiet', cwd: '/work/other', running: true, updatedAt: 2 }] }),
      prompt: async (req, signal) => { prompts.push({ req, aborted: signal.aborted }) },
    },
    sessions: { get: id => sessions.get(id) },
    sessionTitle: { get: session => (session.id === 'sess-x' ? { title: 'Builder' } : undefined) },
    workspaceRegistry: { list: () => [{ id: 'w1', path: '/work/site', title: 'Site', sessionIds: ['sess-x'] }] },
    connection: { fetch: { register: route => { routes.push(route); return () => {} } } },
  }
  const ctx = {
    llm: fakeLlm(),
    logger: { info() {}, warn() {} },
    effect(execute) { disposers.push(execute()) },
    inject(names, fn) {
      ctx.injected = names
      fn(child)
    },
    async unload() { for (const dispose of disposers.reverse()) await dispose?.() },
  }
  return { ctx, hooks, routes, prompts }
}

const post = (port, key, path, body) => new Promise((resolve, reject) => {
  const text = JSON.stringify(body)
  request({ host: '127.0.0.1', port, method: 'POST', path, agent: false, headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${key}`, 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) } }, res => {
    let out = ''
    res.on('data', chunk => { out += chunk })
    res.on('end', () => resolve({ status: res.statusCode, json: JSON.parse(out) }))
  }).on('error', reject).end(text)
})

async function until(check) {
  for (let i = 0; i < 200; i++) {
    try { if (check()) return } catch {}
    await new Promise(resolve => setTimeout(resolve, 10))
  }
  throw new Error('timed out')
}

test('the work view wires DSH host hooks, names and routes, and sends through sessionController.prompt', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-bridge-view-'))
  const was = process.env.XDG_STATE_HOME
  process.env.XDG_STATE_HOME = dir
  const file = join(dir, 'unfold-bridge', 'endpoint.json')
  const { ctx, hooks, routes, prompts } = fakeHost()
  try {
    plugin.apply(ctx)
    assert.deepEqual(ctx.injected, plugin.VIEW_INJECT)
    assert.deepEqual([...hooks.keys()].sort(), ['agent/created', 'agent/request-error', 'agent/status', 'session/event'])
    assert.deepEqual(routes.map(r => r.path).sort(), ['/drafts/cancel', '/drafts/send', '/subscriptions', '/undo', '/view'].map(p => `/api/unfold-bridge${p}`).sort())
    await until(() => statSync(file))
    const { port, key } = JSON.parse(readFileSync(file, 'utf8'))

    const decided = { kind: 'retry' }
    let nexts = 0
    assert.equal(hooks.get('agent/request-error')({ agent: { id: 'sess-x', session: { id: 'sess-x' } }, failure: { code: 'RATE_LIMIT' } }, () => { nexts++; return decided }), decided, 'DSH handling goes on')
    assert.equal(nexts, 1)
    hooks.get('session/event')({ id: 'sess-x' }, { type: 'approval/asked', data: { id: 'ap1', toolName: 'bash', callId: 'call-1', reason: 'escalate' } })
    hooks.get('agent/status')({ agent: { id: 'sess-x', session: { id: 'sess-x' } }, status: 'running' })

    const subs = [{ id: 's1', match: { names: ['Builder'] }, kinds: ['approval_waiting'] }]
    const agents = await post(port, key, '/v1/dsh/agents', { subscriptions: subs })
    assert.equal(agents.status, 200)
    const x = agents.json.agents.find(a => a.session_id === 'sess-x')
    assert.deepEqual([x.name, x.workspace, x.status], ['Builder', 'Site', 'running'])
    const quiet = agents.json.agents.find(a => a.session_id === 'sess-quiet')
    assert.deepEqual([quiet.name, quiet.workspace, quiet.status], ['session sess-qui', 'other', 'running'], 'fallback name, folder name, listed as running')
    assert.deepEqual(agents.json.approvals.map(a => [a.call_id, a.tool]), [['call-1', 'bash']])

    // A draft registered over the key, sent by an admitted tap through the client route.
    const events = request({ host: '127.0.0.1', port, method: 'POST', path: '/v1/dsh/events', agent: false, headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${key}`, 'Content-Type': 'application/json' } })
    let hello = ''
    events.on('response', res => res.on('data', chunk => { hello += chunk }))
    events.end(JSON.stringify({ subscriptions: subs }))
    await until(() => hello.includes('event: hello'))
    const conn_id = JSON.parse(/^data: (.*)$/m.exec(hello)[1]).conn_id
    const token = 'tok-not-a-secret-view-host-01'
    assert.equal((await post(port, key, '/v1/dsh/drafts', { conn_id, generation: 1, effect_id: 'eff-1', token, session_id: 'sess-x', text: 'Next: the footer.' })).status, 201)
    const send = routes.find(r => r.path === '/api/unfold-bridge/drafts/send')
    const res = await send.fetch(new Request('http://127.0.0.1:4800/api/unfold-bridge/drafts/send?effect_id=eff-1', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ token }) }))
    assert.equal(res.status, 200)
    assert.equal(prompts.length, 1)
    assert.equal(prompts[0].req.sessionId, 'sess-x')
    assert.equal(prompts[0].req.mode, 'queue')
    assert.deepEqual(prompts[0].req.content, [{ type: 'text', text: 'Next: the footer.' }])
    assert.equal(typeof prompts[0].req.requestId, 'string')
    events.destroy()
  } finally {
    await ctx.unload()
    if (was === undefined) delete process.env.XDG_STATE_HOME
    else process.env.XDG_STATE_HOME = was
    rmSync(dir, { recursive: true, force: true })
  }
})
