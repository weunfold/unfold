import { test } from 'node:test'
import assert from 'node:assert/strict'
import { request } from 'node:http'
import { startBridge } from '../src/host/bridge.js'
import { CLIENT_ROUTE, clientRoutes } from '../src/host/client-routes.js'
import { dshRoutes } from '../src/host/dsh-routes.js'
import { createWatch } from '../src/host/watch.js'
import { fakeLlm } from './fake-llm.js'

const KEY = 'key-not-a-secret-view-routes-0001'
const TOKEN = 'tok-not-a-secret-view-routes-0001'
const SUBS = [{ id: 's1', match: { session_ids: ['sess-x'] }, kinds: ['turn_done', 'approval_waiting', 'status'] }]

// The bridge with its work view, and the DSH client routes on a fake connection.fetch that
// admits only a request carrying DSH's browser cookie, as DSH admits its own /api.
async function setup({ heartbeatMs = 40, controlMax, send, sendTimeoutMs } = {}) {
  let n = 0
  const watch = createWatch({ names: { title: (s, id) => ({ 'sess-x': 'Builder', 'sess-y': 'Other' })[s?.id ?? id], workspace: () => 'Site' }, newId: () => `conn-${++n}`, ...(controlMax ? { controlMax } : {}) })
  watch.listed([{ sessionId: 'sess-x' }, { sessionId: 'sess-y' }])
  const routes = dshRoutes({ watch, heartbeatMs })
  const logs = []
  const bridge = await startBridge({ llm: fakeLlm(), key: KEY, assistant: m => m, log: line => logs.push(line), dsh: () => routes })
  const prompts = []
  const fetches = new Map()
  const stop = clientRoutes({
    watch,
    register: route => { fetches.set(route.path, route); return () => fetches.delete(route.path) },
    prompt: async (sessionId, text) => { prompts.push({ sessionId, text }); if (send) await send() },
    log: line => logs.push(line),
    ...(sendTimeoutMs ? { sendTimeoutMs } : {}),
  })
  // DSH's admission in front of the route: no cookie, 401; a route answers only its methods.
  const client = async (method, path, { body, type = 'application/json', cookie = true } = {}) => {
    const [pathname] = path.split('?')
    const route = fetches.get(`${CLIENT_ROUTE}${pathname}`)
    if (!cookie) return { status: 401, json: { error: { code: 'unauthorized' } } }
    if (!route || !route.methods.includes(method)) return { status: 404, json: null }
    const res = await route.fetch(new Request(`http://127.0.0.1:4800${CLIENT_ROUTE}${path}`, { method, headers: type ? { 'content-type': type } : {}, ...(body !== undefined ? { body: typeof body === 'string' ? body : JSON.stringify(body) } : {}) }))
    return { status: res.status, json: await res.json().catch(() => null) }
  }
  const streams = []
  const close = async () => { for (const s of streams) s.close(); stop(); routes.close(); await bridge.close() }
  return { watch, bridge, prompts, client, logs, close, fetches, streams }
}

const call = (port, method, path, { body, key = KEY, host = `127.0.0.1:${port}`, origin } = {}) => new Promise((resolve, reject) => {
  const text = body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body)
  const headers = { Host: host, ...(key ? { Authorization: `Bearer ${key}` } : {}), ...(origin ? { Origin: origin } : {}), ...(text ? { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) } : {}) }
  request({ host: '127.0.0.1', port, method, path, agent: false, headers }, res => {
    let out = ''
    res.on('data', chunk => { out += chunk })
    res.on('end', () => resolve({ status: res.statusCode, json: (() => { try { return JSON.parse(out) } catch { return out } })() }))
  }).on('error', reject).end(text)
})

// Opens Unfold's event stream and collects its frames until closed.
function stream(port, subscriptions = SUBS) {
  const text = JSON.stringify({ subscriptions })
  const frames = []
  let raw = ''
  let req
  const opened = new Promise((resolve, reject) => {
    req = request({ host: '127.0.0.1', port, method: 'POST', path: '/v1/dsh/events', agent: false, headers: { Host: `127.0.0.1:${port}`, Authorization: `Bearer ${KEY}`, 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) } }, res => {
      res.setEncoding('utf8')
      res.on('data', chunk => {
        raw += chunk
        let at
        while ((at = raw.indexOf('\n\n')) >= 0) {
          const block = raw.slice(0, at)
          raw = raw.slice(at + 2)
          if (block.startsWith(':')) frames.push({ comment: block.slice(1).trim() })
          else {
            const event = /^event: (.*)$/m.exec(block)?.[1]
            const data = /^data: (.*)$/m.exec(block)?.[1]
            frames.push({ event, data: data === undefined ? undefined : JSON.parse(data) })
          }
        }
      })
      resolve(res)
    }).on('error', reject)
    req.end(text)
  })
  return { frames, opened, close: () => req.destroy() }
}

async function until(check) {
  for (let i = 0; i < 300; i++) {
    if (check()) return
    await new Promise(resolve => setTimeout(resolve, 10))
  }
  throw new Error('timed out')
}

async function connected(port, subscriptions, track) {
  const s = stream(port, subscriptions)
  track?.push(s)
  await s.opened
  await until(() => s.frames.some(f => f.event === 'hello'))
  return { ...s, conn_id: s.frames.find(f => f.event === 'hello').data.conn_id }
}

const draft = (conn_id, extra = {}) => ({ conn_id, generation: 1, effect_id: 'eff-1', token: TOKEN, session_id: 'sess-x', text: 'Ship the page.\n  Then — rest. ✓', ...extra })

test('every work-view route refuses a missing or wrong key (401) and a non-loopback Host or an Origin (403)', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const port = t.bridge.port
    const routes = [['POST', '/v1/dsh/events', { subscriptions: [] }], ['POST', '/v1/dsh/agents', { subscriptions: [] }], ['POST', '/v1/dsh/drafts', {}], ['DELETE', '/v1/dsh/drafts/eff-1'], ['PUT', '/v1/dsh/view', {}]]
    for (const [method, path, body] of routes) {
      assert.equal((await call(port, method, path, { body, key: null })).status, 401, `${method} ${path} no key`)
      assert.equal((await call(port, method, path, { body, key: 'wrong-not-a-secret' })).status, 401, `${method} ${path} wrong key`)
      assert.equal((await call(port, method, path, { body, host: `localhost:${port}` })).status, 403, `${method} ${path} host`)
      assert.equal((await call(port, method, path, { body, origin: 'http://127.0.0.1:4800' })).status, 403, `${method} ${path} origin`)
    }
  } finally { await t.close() }
})

test('the stream opens with hello, filters to the subscriptions, and beats on idle', { timeout: 10_000 }, async () => {
  const t = await setup({ heartbeatMs: 30 })
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    assert.equal(s.frames[0].event, 'hello')
    t.watch.sessionEvent({ id: 'sess-y' }, { type: 'assistant/message', data: { message: { content: [{ type: 'text', text: 'Y secret plan' }] } } })
    t.watch.sessionEvent({ id: 'sess-y' }, { type: 'turn/end', data: { reason: { kind: 'completed' } } })
    t.watch.sessionEvent({ id: 'sess-x' }, { type: 'assistant/message', data: { message: { content: [{ type: 'text', text: 'X done' }] } } })
    t.watch.sessionEvent({ id: 'sess-x' }, { type: 'turn/end', data: { reason: { kind: 'completed' } } })
    await until(() => s.frames.some(f => f.event === 'event'))
    await until(() => s.frames.some(f => f.comment === 'hb'))
    const events = s.frames.filter(f => f.event === 'event').map(f => f.data)
    assert.deepEqual(events.map(e => [e.session_id, e.kind, e.summary]), [['sess-x', 'turn_done', 'X done']])
    assert.equal(JSON.stringify(s.frames).includes('Y secret plan'), false)
    const agents = (await call(t.bridge.port, 'POST', '/v1/dsh/agents', { body: { subscriptions: SUBS } })).json
    assert.equal(agents.agents.find(a => a.session_id === 'sess-y').last_summary, undefined)
    assert.equal(agents.agents.find(a => a.session_id === 'sess-x').last_summary, 'X done')
    assert.equal((await call(t.bridge.port, 'POST', '/v1/dsh/events', { body: { subscriptions: [{ id: 'a', match: {}, kinds: ['turn_done'] }] } })).status, 400)
    s.close()
  } finally { await t.close() }
})

test('an admitted send passes the registered text byte for byte, once, and emits one sent event', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    const reg = await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    assert.equal(reg.status, 201)
    assert.deepEqual(reg.json, { effect_id: 'eff-1', session_id: 'sess-x', name: 'Builder', workspace: 'Site' })
    const sent = await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })
    assert.equal(sent.status, 200)
    assert.deepEqual(t.prompts, [{ sessionId: 'sess-x', text: 'Ship the page.\n  Then — rest. ✓' }])
    assert.equal((await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })).status, 404, 'a second tap sends nothing')
    assert.equal(t.prompts.length, 1)
    await until(() => s.frames.some(f => f.event === 'control'))
    assert.deepEqual(s.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'sent', effect_id: 'eff-1' }])
    assert.equal((await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id, { generation: 2 }) })).status, 409, 'a sent id is not registered again')
    s.close()
  } finally { await t.close() }
})

test('a send body with text, a wrong token, a non-JSON body, an unknown draft or no browser session sends nothing', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    const send = (opts, path = '/drafts/send?effect_id=eff-1') => t.client('POST', path, opts)
    assert.deepEqual(await send({ body: { token: TOKEN, text: 'rm -rf ~' } }), { status: 400, json: { error: { code: 'body_refused' } } })
    assert.deepEqual(await send({ body: { token: 'tok-wrong-not-a-secret-0' } }), { status: 403, json: { error: { code: 'bad_token' } } })
    assert.deepEqual(await send({ body: `token=${TOKEN}`, type: 'text/plain' }), { status: 415, json: { error: { code: 'unsupported_media' } } })
    assert.deepEqual(await send({ body: 'not json' }), { status: 400, json: { error: { code: 'body_refused' } } })
    assert.equal((await send({ body: { token: TOKEN } }, '/drafts/send?effect_id=nope')).status, 404)
    assert.equal((await send({ body: { token: TOKEN } }, '/drafts/send')).status, 404)
    assert.equal((await send({ body: { token: TOKEN }, cookie: false })).status, 401)
    assert.deepEqual(t.prompts, [])
    assert.equal(t.watch.snapshot().drafts.length, 1, 'the draft is still there')
    s.close()
  } finally { await t.close() }
})

test('no route the key alone reaches sends a prompt', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const port = t.bridge.port
    const s = await connected(port, undefined, t.streams)
    await call(port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    await call(port, 'POST', '/v1/dsh/agents', { body: { subscriptions: SUBS } })
    await call(port, 'PUT', '/v1/dsh/view', { body: { conn_id: s.conn_id, generation: 2, subscriptions: SUBS, log_tail: [], advice: [] } })
    for (const path of ['/v1/dsh/drafts/eff-1/send', '/v1/dsh/send', '/v1/dsh/prompt', '/v1/dsh/sessions/sess-x/prompt', '/api/unfold-bridge/drafts/send?effect_id=eff-1']) {
      const res = await call(port, 'POST', path, { body: { token: TOKEN, text: 'x', session_id: 'sess-x' } })
      assert.equal(res.status, 404, path)
    }
    await call(port, 'DELETE', '/v1/dsh/drafts/eff-1')
    assert.deepEqual(t.prompts, [])
    s.close()
  } finally { await t.close() }
})

test('a view or draft from an old connection or a lower generation is refused (409)', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const port = t.bridge.port
    const a = await connected(port, undefined, t.streams)
    const b = await connected(port, undefined, t.streams)
    assert.equal((await call(port, 'POST', '/v1/dsh/drafts', { body: draft(a.conn_id) })).status, 409)
    assert.equal((await call(port, 'PUT', '/v1/dsh/view', { body: { conn_id: b.conn_id, generation: 3, subscriptions: [], log_tail: [], advice: [] } })).status, 200)
    const low = await call(port, 'PUT', '/v1/dsh/view', { body: { conn_id: b.conn_id, generation: 2, subscriptions: [], log_tail: [], advice: [] } })
    assert.deepEqual([low.status, low.json.error.code], [409, 'stale_generation'])
    a.close()
    b.close()
  } finally { await t.close() }
})

// A restarted agentd starts its generations again on its new connection; the old connection's
// highest view must not lock it out.
test('a new connection takes views from generation 1 again', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const port = t.bridge.port
    const a = await connected(port, undefined, t.streams)
    assert.equal((await call(port, 'PUT', '/v1/dsh/view', { body: { conn_id: a.conn_id, generation: 5, subscriptions: [], log_tail: [], advice: [] } })).status, 200)
    const b = await connected(port, undefined, t.streams)
    assert.equal((await call(port, 'PUT', '/v1/dsh/view', { body: { conn_id: b.conn_id, generation: 1, subscriptions: [], log_tail: ['b'], advice: [] } })).status, 200)
    assert.deepEqual(t.watch.snapshot().view.log_tail, ['b'])
  } finally { await t.close() }
})

test('a row toggle and a Send made while Unfold is away arrive in order after the next connect; the send happens once', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const port = t.bridge.port
    const a = await connected(port, undefined, t.streams)
    await call(port, 'POST', '/v1/dsh/drafts', { body: draft(a.conn_id) })
    a.close()
    await until(() => !t.watch.snapshot().connected)
    assert.equal((await t.client('POST', '/subscriptions', { body: { session_id: 'sess-y' } })).status, 200)
    assert.equal((await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })).status, 200)
    assert.equal((await t.client('DELETE', '/subscriptions?id=s1', { body: {} })).status, 200)
    assert.equal((await t.client('POST', '/undo?change_id=ch-1', { body: {} })).status, 200)
    assert.equal(t.prompts.length, 1)
    const b = await connected(port, undefined, t.streams)
    await until(() => b.frames.filter(f => f.event === 'control').length === 4)
    assert.deepEqual(b.frames.map(f => f.event).slice(0, 5), ['hello', 'control', 'control', 'control', 'control'])
    assert.deepEqual(b.frames.filter(f => f.event === 'control').map(f => f.data), [
      { kind: 'sub_add', session_id: 'sess-y' },
      { kind: 'sent', effect_id: 'eff-1' },
      { kind: 'sub_remove', id: 's1' },
      { kind: 'sub_undo', change_id: 'ch-1' },
    ])
    assert.equal((await call(port, 'POST', '/v1/dsh/drafts', { body: draft(b.conn_id) })).status, 409, 'the sent draft is not registered again')
    assert.equal(t.prompts.length, 1)
    b.close()
  } finally { await t.close() }
})

test('a full control queue gives 503 and sends nothing', { timeout: 10_000 }, async () => {
  const t = await setup({ controlMax: 1 })
  try {
    const port = t.bridge.port
    const a = await connected(port, undefined, t.streams)
    await call(port, 'POST', '/v1/dsh/drafts', { body: draft(a.conn_id) })
    a.close()
    await until(() => !t.watch.snapshot().connected)
    assert.equal((await t.client('POST', '/subscriptions', { body: { session_id: 'sess-y' } })).status, 200)
    assert.deepEqual(await t.client('POST', '/subscriptions', { body: { session_id: 'sess-x' } }), { status: 503, json: { error: { code: 'agentd_unavailable' } } })
    assert.equal((await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })).status, 503)
    assert.deepEqual(t.prompts, [])
  } finally { await t.close() }
})

test('the view stream carries agents, the view and drafts with tokens, and only from DSH-admitted routes', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    assert.deepEqual([...t.fetches.keys()].sort(), ['/drafts/cancel', '/drafts/send', '/subscriptions', '/undo', '/view'].map(p => `${CLIENT_ROUTE}${p}`).sort())
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    const ac = new AbortController()
    const res = await t.fetches.get(`${CLIENT_ROUTE}/view`).fetch(new Request(`http://127.0.0.1:4800${CLIENT_ROUTE}/view`, { signal: ac.signal }))
    assert.equal(res.headers.get('content-type'), 'text/event-stream')
    const reader = res.body.getReader()
    const first = new TextDecoder().decode((await reader.read()).value)
    const view = JSON.parse(/^data: (.*)$/m.exec(first)[1])
    assert.deepEqual(view.drafts.map(d => [d.effect_id, d.token, d.name, d.text]), [['eff-1', TOKEN, 'Builder', 'Ship the page.\n  Then — rest. ✓']])
    assert.equal(first.includes(KEY), false, 'the key never reaches the client stream')
    ac.abort()
    s.close()
  } finally { await t.close() }
})

test('the log names no key and no token', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: 'tok-wrong-not-a-secret-0' } })
    await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })
    await call(t.bridge.port, 'POST', '/v1/dsh/agents', { body: { subscriptions: [] }, key: 'wrong-not-a-secret' })
    s.close()
    await until(() => t.logs.some(line => line.includes('/v1/dsh/events')))
    const all = t.logs.join('\n')
    assert.ok(t.logs.length > 0)
    for (const secret of [KEY, TOKEN, 'tok-wrong-not-a-secret-0']) assert.equal(all.includes(secret), false)
  } finally { await t.close() }
})

test('without DSH session services the work-view routes are not served (404) and the model routes still are', { timeout: 10_000 }, async () => {
  const bridge = await startBridge({ llm: fakeLlm(), key: KEY, assistant: m => m })
  try {
    assert.equal((await call(bridge.port, 'POST', '/v1/dsh/agents', { body: { subscriptions: [] } })).status, 404)
    assert.equal((await call(bridge.port, 'GET', '/v1/models')).status, 200)
  } finally { await bridge.close() }
})

test('a send outcome is recorded even when the control queue fills during the send', { timeout: 10_000 }, async () => {
  let release
  const t = await setup({ controlMax: 1, send: () => new Promise(resolve => { release = resolve }) })
  try {
    const a = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(a.conn_id) })
    a.close()
    await until(() => !t.watch.snapshot().connected)
    const sending = t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })
    await until(() => release !== undefined)
    assert.equal((await t.client('POST', '/subscriptions', { body: { session_id: 'sess-y' } })).status, 200, 'the toggle fills the queue')
    release()
    assert.equal((await sending).status, 200)
    const b = await connected(t.bridge.port, undefined, t.streams)
    await until(() => b.frames.filter(f => f.event === 'control').length === 2)
    assert.deepEqual(b.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'sub_add', session_id: 'sess-y' }, { kind: 'sent', effect_id: 'eff-1' }])
  } finally { await t.close() }
})

test('a failed send outcome is recorded even when the control queue fills during the send', { timeout: 10_000 }, async () => {
  let fail
  const t = await setup({ controlMax: 1, send: () => new Promise((_resolve, reject) => { fail = reject }) })
  try {
    const a = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(a.conn_id) })
    a.close()
    await until(() => !t.watch.snapshot().connected)
    const sending = t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })
    await until(() => fail !== undefined)
    assert.equal((await t.client('POST', '/subscriptions', { body: { session_id: 'sess-y' } })).status, 200, 'the toggle fills the queue')
    fail(new Error('refused'))
    assert.deepEqual(await sending, { status: 502, json: { error: { code: 'send_failed' } } })
    const b = await connected(t.bridge.port, undefined, t.streams)
    await until(() => b.frames.filter(f => f.event === 'control').length === 2)
    assert.deepEqual(b.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'sub_add', session_id: 'sess-y' }, { kind: 'cancelled', effect_id: 'eff-1', reason: 'send_failed' }])
  } finally { await t.close() }
})

test('a send whose prompt throws answers 502, tells Unfold cancelled with send_failed, and logs no token', { timeout: 10_000 }, async () => {
  const t = await setup({ send: async () => { throw new Error(`refused ${TOKEN}`) } })
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    assert.deepEqual(await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } }), { status: 502, json: { error: { code: 'send_failed' } } })
    await until(() => s.frames.some(f => f.event === 'control'))
    assert.deepEqual(s.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'cancelled', effect_id: 'eff-1', reason: 'send_failed' }])
    assert.equal((await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })).status, 404, 'never sent again')
    assert.equal(t.prompts.length, 1)
    assert.equal(t.logs.join('\n').includes(TOKEN), false)
  } finally { await t.close() }
})

test('a send with no answer in time answers 504 send_unknown, and the draft is never sent again', { timeout: 10_000 }, async () => {
  const t = await setup({ send: () => new Promise(() => {}), sendTimeoutMs: 100 })
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    assert.deepEqual(await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } }), { status: 504, json: { error: { code: 'send_unknown' } } })
    await until(() => s.frames.some(f => f.event === 'control'))
    assert.deepEqual(s.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'cancelled', effect_id: 'eff-1', reason: 'send_unknown' }])
    assert.equal((await t.client('POST', '/drafts/send?effect_id=eff-1', { body: { token: TOKEN } })).status, 404)
    assert.equal((await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id, { generation: 2 }) })).status, 409)
    assert.equal(t.prompts.length, 1)
  } finally { await t.close() }
})

test('a cancel with a wrong token is refused (403) and the draft stays', { timeout: 10_000 }, async () => {
  const t = await setup()
  try {
    const s = await connected(t.bridge.port, undefined, t.streams)
    await call(t.bridge.port, 'POST', '/v1/dsh/drafts', { body: draft(s.conn_id) })
    assert.deepEqual(await t.client('POST', '/drafts/cancel?effect_id=eff-1', { body: { token: 'tok-wrong-not-a-secret-0' } }), { status: 403, json: { error: { code: 'bad_token' } } })
    assert.equal(t.watch.snapshot().drafts.length, 1)
    assert.equal((await t.client('POST', '/drafts/cancel?effect_id=eff-1', { body: { token: TOKEN } })).status, 200)
    await until(() => s.frames.some(f => f.event === 'control'))
    assert.deepEqual(s.frames.filter(f => f.event === 'control').map(f => f.data), [{ kind: 'cancelled', effect_id: 'eff-1' }])
  } finally { await t.close() }
})
