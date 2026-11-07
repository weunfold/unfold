import { test } from 'node:test'
import assert from 'node:assert/strict'
import { request } from 'node:http'
import { isDeepSeekModel, startBridge, takePort } from '../src/host/bridge.js'
import { TEXT, assistant, failure, fakeLlm } from './fake-llm.js'
import { hold, onFreePort } from './ports.js'

const KEY = 'test-key-not-a-secret-0123456789abcdef'

async function bridge(llm = fakeLlm(), options = {}) {
  const logs = []
  const b = await startBridge({ llm, key: KEY, assistant, log: line => logs.push(line), ...options })
  return { ...b, llm, logs }
}

// One HTTP exchange with the bridge; `host` and `headers` replace what a client sends.
function call(port, { method = 'GET', path = '/v1/models', key = KEY, body, headers = {}, host } = {}) {
  return new Promise((resolve, reject) => {
    const data = body === undefined ? undefined : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body))
    const req = request({
      host: '127.0.0.1', port, method, path, agent: false,
      headers: {
        Host: host ?? `127.0.0.1:${port}`,
        ...(key ? { Authorization: `Bearer ${key}` } : {}),
        ...(data ? { 'Content-Type': 'application/json', 'Content-Length': data.length } : {}),
        ...headers,
      },
    }, res => {
      let text = ''
      res.setEncoding('utf8')
      res.on('data', chunk => { text += chunk })
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, text }))
    })
    req.on('error', reject)
    req.end(data)
  })
}

const events = text => text.split('\n\n').filter(Boolean).map(frame => {
  assert.ok(frame.startsWith('data: '), `an SSE data frame: ${frame}`)
  const data = frame.slice('data: '.length)
  return data === '[DONE]' ? data : JSON.parse(data)
})

const chat = (extra = {}) => ({
  model: 'deepseek-official/deepseek-v4-flash',
  stream: true,
  messages: [
    { role: 'system', content: 'You are Unfold.' },
    { role: 'user', content: 'Oi' },
    { role: 'assistant', content: 'Oi! Tudo bem?' },
    { role: 'user', content: [{ type: 'text', text: 'Diga olá ao Ana.' }] },
  ],
  ...extra,
})

test('binds 127.0.0.1 only, on a port of its own', async () => {
  const b = await bridge()
  try {
    assert.equal(b.address, '127.0.0.1')
    assert.ok(b.port > 0)
  } finally { await b.close() }
})

test('/v1/models lists every provider route but DeepSeek\'s as provider/model, with names, ready and levels', async () => {
  const llm = fakeLlm({ providers: [{ id: 'deepseek-official', name: 'DeepSeek' }, { id: 'deepseek', name: 'DeepSeek' }, { id: 'broken', name: 'Broken' }, { id: 'codex', name: 'ChatGPT' }] })
  const b = await bridge(llm)
  try {
    const res = await call(b.port)
    assert.equal(res.status, 200)
    assert.match(res.headers['content-type'], /^application\/json/)
    // Unfold calls DeepSeek directly: its routes are never a second DeepSeek row.
    assert.deepEqual(JSON.parse(res.text), {
      object: 'list',
      data: [
        { id: 'codex/gpt-5.5', object: 'model', owned_by: 'codex', name: 'GPT-5.5', provider_name: 'ChatGPT', ready: true, levels: [] },
      ],
    })
  } finally { await b.close() }
})

test('a wrong key, no key, or another scheme is refused with 401 and no model is called', async () => {
  const b = await bridge()
  try {
    for (const key of ['wrong-key-0123456789abcdef', KEY.slice(0, -1), `${KEY}x`, null]) {
      for (const path of ['/v1/models', '/v1/chat/completions']) {
        const res = await call(b.port, { method: path.endsWith('models') ? 'GET' : 'POST', path, key, body: path.endsWith('models') ? undefined : chat() })
        assert.equal(res.status, 401, `${path} with ${key ? 'a wrong key' : 'no key'}`)
        assert.deepEqual(JSON.parse(res.text).error.code, 'unauthorized')
      }
    }
    const basic = await call(b.port, { key: null, headers: { Authorization: `Basic ${Buffer.from(`x:${KEY}`).toString('base64')}` } })
    assert.equal(basic.status, 401)
    assert.equal(b.llm.calls.length, 0)
  } finally { await b.close() }
})

test('a browser cannot reach it: another Host, any Origin, and preflights are refused, and no CORS header is sent', async () => {
  const b = await bridge()
  try {
    for (const host of ['localhost', `localhost:${b.port}`, 'evil.example', `127.0.0.1:${b.port + 1}`]) {
      const res = await call(b.port, { host })
      assert.equal(res.status, 403, `Host ${host}`)
    }
    const origin = await call(b.port, { headers: { Origin: 'https://evil.example' } })
    assert.equal(origin.status, 403)
    const preflight = await call(b.port, { method: 'OPTIONS', path: '/v1/chat/completions', key: null, headers: { Origin: 'https://evil.example', 'Access-Control-Request-Method': 'POST' } })
    assert.ok(preflight.status === 401 || preflight.status === 403)
    for (const res of [origin, preflight]) {
      assert.ok(!Object.keys(res.headers).some(name => name.startsWith('access-control-')), 'no CORS headers')
    }
    assert.equal(b.llm.calls.length, 0)
  } finally { await b.close() }
})

test('a chat streams as OpenAI chunks: the role, the text deltas, the finish, the usage, then [DONE]', async () => {
  const b = await bridge()
  try {
    const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ stream_options: { include_usage: true } }) })
    assert.equal(res.status, 200)
    assert.match(res.headers['content-type'], /^text\/event-stream/)
    const ev = events(res.text)
    assert.equal(ev.at(-1), '[DONE]')
    const chunks = ev.slice(0, -1)
    const id = chunks[0].id
    assert.match(id, /^chatcmpl-/)
    for (const c of chunks) {
      assert.equal(c.object, 'chat.completion.chunk')
      assert.equal(c.id, id)
      assert.equal(c.model, 'deepseek-official/deepseek-v4-flash')
    }
    assert.deepEqual(chunks.map(c => c.choices), [
      [{ index: 0, delta: { role: 'assistant', content: 'Olá' }, finish_reason: null }],
      [{ index: 0, delta: { content: ', Ana.' }, finish_reason: null }],
      [{ index: 0, delta: {}, finish_reason: 'stop' }],
      [],
    ])
    // DSH's input count is uncached input only; OpenAI's prompt_tokens includes the cached part.
    assert.deepEqual(chunks.at(-1).usage, { prompt_tokens: 15, completion_tokens: 4, total_tokens: 19 })
  } finally { await b.close() }
})

test('without include_usage there is no usage chunk', async () => {
  const b = await bridge()
  try {
    const ev = events((await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat() })).text)
    assert.equal(ev.at(-1), '[DONE]')
    assert.ok(ev.slice(0, -1).every(c => c.usage === undefined && c.choices.length === 1))
  } finally { await b.close() }
})

test('the request becomes one llm.stream call: system text, user inputs, assistant history, and the sampling fields', async () => {
  const b = await bridge()
  try {
    await call(b.port, {
      method: 'POST', path: '/v1/chat/completions',
      body: chat({ temperature: 0.7, max_tokens: 300, stop: '\n\nUser:', top_p: 0.95, chat_template_kwargs: { enable_thinking: false }, stream_options: { include_usage: true } }),
    })
    const [options] = b.llm.calls
    assert.equal(options.provider, 'deepseek-official')
    assert.equal(options.model, 'deepseek-v4-flash')
    assert.equal(options.system, 'You are Unfold.')
    assert.deepEqual(options.messages, [
      { role: 'user', content: [{ type: 'text', text: 'Oi' }] },
      { id: 'm-Oi! Tudo bem?', role: 'assistant', content: [{ type: 'text', text: 'Oi! Tudo bem?' }], source: { kind: 'model', provider: 'deepseek-official', model: 'deepseek-v4-flash' } },
      { role: 'user', content: [{ type: 'text', text: 'Diga olá ao Ana.' }] },
    ])
    assert.equal(options.temperature, 0.7)
    assert.equal(options.maxTokens, 300)
    assert.deepEqual(options.stop, ['\n\nUser:'])
    assert.ok(options.signal instanceof AbortSignal)
    assert.equal(options.tools, undefined)
  } finally { await b.close() }
})

test('a model id may hold slashes after its provider; max_completion_tokens counts as max_tokens', async () => {
  const llm = fakeLlm()
  const b = await bridge(llm)
  try {
    await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ model: 'openrouter/anthropic/claude-x', max_completion_tokens: 64 }) })
    assert.equal(llm.calls[0].provider, 'openrouter')
    assert.equal(llm.calls[0].model, 'anthropic/claude-x')
    assert.equal(llm.calls[0].maxTokens, 64)
  } finally { await b.close() }
})

test('what the bridge does not serve is refused with 400 before any model call: tools, tool messages, a bad model, bad JSON', async () => {
  const b = await bridge()
  try {
    const bad = [
      chat({ tools: [{ type: 'function', function: { name: 'x', parameters: {} } }] }),
      chat({ messages: [{ role: 'tool', content: 'r', tool_call_id: 'c1' }] }),
      chat({ messages: [{ role: 'assistant', content: null, tool_calls: [{ id: 'c1', type: 'function', function: { name: 'x', arguments: '{}' } }] }] }),
      chat({ messages: [{ role: 'user', content: [{ type: 'image_url', image_url: { url: 'data:,' } }] }] }),
      chat({ messages: [] }),
      chat({ model: 'no-provider' }),
      chat({ model: '/deepseek-v4-flash' }),
      chat({ model: 7 }),
      '{not json',
    ]
    for (const body of bad) {
      const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body })
      assert.equal(res.status, 400, JSON.stringify(body))
      assert.equal(JSON.parse(res.text).error.type, 'invalid_request_error')
    }
    assert.equal(b.llm.calls.length, 0)
  } finally { await b.close() }
})

test('an unknown route is 404 and a body over the limit is 413', async () => {
  const b = await bridge(fakeLlm(), { maxBody: 1024 })
  try {
    assert.equal((await call(b.port, { path: '/v1/embeddings' })).status, 404)
    assert.equal((await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ messages: [{ role: 'user', content: 'x'.repeat(2048) }] }) })).status, 413)
    assert.equal(b.llm.calls.length, 0)
  } finally { await b.close() }
})

test('a failure before the first token is an HTTP status with DSH\'s stable code and never the provider\'s words', async () => {
  const cases = [['RATE_LIMIT', 429], ['QUOTA', 429], ['AUTH', 502], ['MISSING_CREDENTIAL', 502], ['NO_ADAPTER', 404], ['CONTEXT_WINDOW_EXCEEDED', 400], ['SOMETHING_NEW', 502]]
  const b = await bridge(fakeLlm({ streams: cases.map(([code]) => failure(code, `secret detail for ${code}`)) }))
  try {
    for (const [code, status] of cases) {
      const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat() })
      assert.equal(res.status, status, code)
      const body = JSON.parse(res.text)
      assert.equal(body.error.code, code)
      assert.doesNotMatch(res.text, /secret detail/)
    }
  } finally { await b.close() }
})

test('a failure after the first token ends the stream with an error event and no [DONE]', async () => {
  const b = await bridge(fakeLlm({ streams: [[TEXT[0], TEXT[1], ...failure('RATE_LIMIT', 'secret detail')]] }))
  try {
    const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat() })
    assert.equal(res.status, 200)
    const ev = events(res.text)
    assert.notEqual(ev.at(-1), '[DONE]')
    assert.equal(ev.at(-1).error.code, 'RATE_LIMIT')
    assert.doesNotMatch(res.text, /secret detail/)
  } finally { await b.close() }
})

test('a request without stream gets one chat.completion', async () => {
  const b = await bridge()
  try {
    const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ stream: false }) })
    assert.equal(res.status, 200)
    const body = JSON.parse(res.text)
    assert.equal(body.object, 'chat.completion')
    assert.deepEqual(body.choices, [{ index: 0, message: { role: 'assistant', content: 'Olá, Ana.' }, finish_reason: 'stop' }])
    assert.deepEqual(body.usage, { prompt_tokens: 15, completion_tokens: 4, total_tokens: 19 })
  } finally { await b.close() }
})

test('a client that hangs up aborts the model call', async () => {
  let release
  const hold = new Promise(resolve => { release = resolve })
  const llm = fakeLlm({ hold })
  const b = await bridge(llm)
  try {
    await new Promise((resolve, reject) => {
      const data = Buffer.from(JSON.stringify(chat()))
      const req = request({ host: '127.0.0.1', port: b.port, method: 'POST', path: '/v1/chat/completions', agent: false, headers: { Host: `127.0.0.1:${b.port}`, Authorization: `Bearer ${KEY}`, 'Content-Type': 'application/json', 'Content-Length': data.length } }, res => {
        res.once('data', () => { req.destroy(); resolve() })
      })
      req.on('error', () => {})
      req.on('timeout', reject)
      req.end(data)
    })
    for (let i = 0; i < 50 && !llm.calls[0]?.signal.aborted; i++) await new Promise(resolve => setTimeout(resolve, 10))
    assert.equal(llm.calls[0].signal.aborted, true)
  } finally { release(); await b.close() }
})

test('the log names method, path, status and time, and never the key, a prompt or a reply', async () => {
  const fixture_secret_0 = globalThis.crypto.randomUUID().replaceAll("-", "")

  const b = await bridge()
  try {
    await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat() })
    await call(b.port, { key: fixture_secret_0 })
    const all = b.logs.join('\n')
    assert.match(all, /POST \/v1\/chat\/completions 200 \d+ ms/)
    assert.match(all, /GET \/v1\/models 401 \d+ ms/)
    for (const secret of [KEY, fixture_secret_0, 'You are Unfold', 'Ana', 'Olá', 'Tudo bem']) assert.ok(!all.includes(secret), secret)
  } finally { await b.close() }
})

test('close stops listening', async () => {
  const b = await bridge()
  await b.close()
  await assert.rejects(call(b.port), { code: 'ECONNREFUSED' })
})


test('UNFOLD_BRIDGE_PORT is read once and leaves the env: a port number, else the text as given', () => {
  const env = { UNFOLD_BRIDGE_PORT: '41712', HOME: '/home/x' }
  assert.equal(takePort(env), 41712)
  assert.deepEqual(env, { HOME: '/home/x' }, 'gone from the env after the read')
  assert.equal(takePort(env), undefined, 'a second read finds nothing')
  // set means named: an empty value is no port, never a random one
  for (const value of ['', '0', '00', 'abc', '4171x', '-1', '1.5', ' 41712']) {
    const named = { UNFOLD_BRIDGE_PORT: value }
    assert.equal(takePort(named), value, JSON.stringify(value))
    assert.deepEqual(named, {}, JSON.stringify(value))
  }
})

test('a refused named port is logged as it was given, never as NaN', async () => {
  for (const [value, shown] of [['0', '(0)'], ['00', '(00)'], ['abc', '(abc)'], ['', '(empty)']]) {
    const refusal = await bridge(fakeLlm(), { port: takePort({ UNFOLD_BRIDGE_PORT: value }) }).then(() => null, error => error)
    assert.equal(refusal?.code, 'bad_port', JSON.stringify(value))
    assert.ok(refusal.message.endsWith(shown), `${JSON.stringify(value)}: ${refusal.message}`)
    assert.ok(!refusal.message.includes('NaN'), refusal.message)
  }
})

test('a named port is bound exactly, on 127.0.0.1', async () => {
  const { b, port } = await onFreePort(async port => ({ b: await bridge(fakeLlm(), { port }), port }))
  try {
    assert.equal(b.port, port)
    assert.equal(b.address, '127.0.0.1')
    assert.equal((await call(port)).status, 200)
  } finally { await b.close() }
})

test('a named port that is taken refuses the start: no other port is tried', async () => {
  const holder = await hold()
  const started = []
  try {
    await assert.rejects(bridge(fakeLlm(), { port: holder.port }).then(b => { started.push(b); return b }), { code: 'EADDRINUSE' })
    assert.deepEqual(started, [])
  } finally { for (const b of started) await b.close(); await holder.close() }
})

test('a named port outside 1024-65535, or not a port at all, refuses the start', async () => {
  for (const port of [NaN, 80, 1023, 65536, 41712.5, -1]) {
    await assert.rejects(bridge(fakeLlm(), { port }), { code: 'bad_port' }, String(port))
  }
})

const LEVELS = { 'deepseek-official/deepseek-v4-flash': ['off', 'low', 'high', 'max'], 'codex/gpt-5.5': ['minimal', 'low', 'high'] }

test('/v1/models lists each model\'s real levels and whether its provider\'s key is set', async () => {
  const llm = fakeLlm({ levels: LEVELS })
  const b = await bridge(llm, { missingKeys: async providers => new Set(providers.filter(id => id === 'codex')) })
  try {
    assert.deepEqual(JSON.parse((await call(b.port)).text).data, [
      { id: 'codex/gpt-5.5', object: 'model', owned_by: 'codex', name: 'GPT-5.5', provider_name: 'ChatGPT', ready: false, levels: ['minimal', 'low', 'high'] },
    ])
  } finally { await b.close() }
})

test('the thinking level goes through as asked; none asked is the off level, never an adapter default', async () => {
  const b = await bridge(fakeLlm({ levels: LEVELS }))
  const effortOf = async (extra, model) => {
    const before = b.llm.calls.length
    const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ ...(model ? { model } : {}), ...extra }) })
    return { status: res.status, effort: b.llm.calls.length > before ? (Object.hasOwn(b.llm.calls.at(-1), 'reasoningEffort') ? b.llm.calls.at(-1).reasoningEffort : 'unset') : 'no call', text: res.text }
  }
  try {
    assert.equal((await effortOf({ reasoning_effort: 'high' })).effort, 'high')
    assert.equal((await effortOf({ reasoning_effort: 'MAX' })).effort, 'max')
    assert.equal((await effortOf({})).effort, 'off')
    assert.equal((await effortOf({ reasoning_effort: 'none' })).effort, 'off')
    // Thinking off the chat-template way wins over a level.
    assert.equal((await effortOf({ reasoning_effort: 'high', chat_template_kwargs: { enable_thinking: false } })).effort, 'off')
    // A model without an off level gets its lowest, explicitly: never the adapter's default.
    assert.equal((await effortOf({}, 'codex/gpt-5.5')).effort, 'minimal')
    // A level the model does not have is refused before any call.
    const refused = await effortOf({ reasoning_effort: 'medium' })
    assert.equal(refused.status, 400)
    assert.equal(refused.effort, 'no call')
    assert.match(JSON.parse(refused.text).error.message, /not a level of deepseek-official\/deepseek-v4-flash/)
    assert.equal((await effortOf({ reasoning_effort: 7 })).status, 400)
  } finally { await b.close() }
  // A model with no level control gets none.
  const plain = await bridge()
  try {
    await call(plain.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ reasoning_effort: 'high' }) })
    assert.equal(Object.hasOwn(plain.llm.calls[0], 'reasoningEffort'), false)
  } finally { await plain.close() }
})

test('"none", what Unfold\'s agent always sends on this route, is the model\'s off level, else its lowest; never a 400', async () => {
  const levels = {
    // A DeepSeek-style adapter: off is one of its levels, and its default thinks.
    'deepseek-official/deepseek-v4-flash': ['off', 'low', 'high', 'max'],
    // A codex-style adapter whose off level is called none.
    'codex/gpt-5.5': ['none', 'low', 'medium', 'high'],
    // A codex-style adapter with no off level.
    'codex/gpt-5.5-mini': ['minimal', 'low', 'high'],
  }
  const llm = fakeLlm({ levels, models: { codex: [{ provider: 'codex', id: 'gpt-5.5', name: 'GPT-5.5' }, { provider: 'codex', id: 'gpt-5.5-mini', name: 'GPT-5.5 mini' }] } })
  const b = await bridge(llm)
  try {
    const sent = []
    for (const model of Object.keys(levels)) {
      const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat({ model, reasoning_effort: 'none', max_tokens: 400 }) })
      assert.equal(res.status, 200, model)
      const options = b.llm.calls.at(-1)
      sent.push([model, Object.hasOwn(options, 'reasoningEffort') ? options.reasoningEffort : 'unset'])
    }
    assert.deepEqual(sent, [['deepseek-official/deepseek-v4-flash', 'off'], ['codex/gpt-5.5', 'none'], ['codex/gpt-5.5-mini', 'minimal']])
  } finally { await b.close() }
})

test('/v1/models leaves out DeepSeek models another route serves; a chat to a DeepSeek route is still served', async () => {
  const llm = fakeLlm({
    providers: [{ id: 'openrouter', name: 'OpenRouter' }, { id: 'deepseek-official', name: 'DeepSeek' }, { id: 'cloudflare-workers-ai', name: 'Workers AI' }, { id: 'amazon-bedrock', name: 'Bedrock' }],
    models: {
      openrouter: [{ id: 'deepseek/deepseek-chat', name: 'DeepSeek V4' }, { id: 'anthropic/claude-x', name: 'Claude X' }, { id: 'notdeepseek-x', name: 'Not DeepSeek' }, { id: 'foo/deepseekish', name: 'DeepSeekish' }],
      'deepseek-official': [{ id: 'deepseek-v4-flash', name: 'DeepSeek V4 Flash' }],
      // The catalog's other shapes of a DeepSeek model, under providers that are not DeepSeek.
      'cloudflare-workers-ai': [{ id: '@cf/deepseek-ai/deepseek-r1-distill-qwen-32b', name: 'DeepSeek R1 Distill' }],
      'amazon-bedrock': [{ id: 'deepseek.v3.2', name: 'DeepSeek V3.2' }],
    },
  })
  const b = await bridge(llm)
  try {
    // A DeepSeek model leaves the list in every shape; a name that merely holds the letters stays.
    assert.deepEqual(JSON.parse((await call(b.port)).text).data.map(model => model.id), ['openrouter/anthropic/claude-x', 'openrouter/notdeepseek-x', 'openrouter/foo/deepseekish'])
    // An app that saved a DeepSeek route before this still gets its answer.
    const res = await call(b.port, { method: 'POST', path: '/v1/chat/completions', body: chat() })
    assert.equal(res.status, 200)
    assert.equal(llm.calls.at(-1).provider, 'deepseek-official')
  } finally { await b.close() }
})

test('a DeepSeek model is one whose own id has `deepseek` as a word, never a substring', () => {
  // The listing passes a provider's bare model id, before its route is put in front: an
  // aggregator's, and the shapes DSH's catalog has under other providers.
  assert.equal(isDeepSeekModel('deepseek/deepseek-chat'), true)
  assert.equal(isDeepSeekModel('DeepSeek/DeepSeek-V4'), true)
  assert.equal(isDeepSeekModel('deepseek-ai/DeepSeek-V4-Flash'), true)
  assert.equal(isDeepSeekModel('deepseek.v3.2'), true)
  assert.equal(isDeepSeekModel('@cf/deepseek-ai/deepseek-r1-distill-qwen-32b'), true)
  assert.equal(isDeepSeekModel('deepseek-v4-flash'), true)
  // A name that only holds the word is not one.
  assert.equal(isDeepSeekModel('notdeepseek-x'), false)
  assert.equal(isDeepSeekModel('foo/deepseekish'), false)
  assert.equal(isDeepSeekModel('openrouter/notdeepseek/model'), false)
  assert.equal(isDeepSeekModel('anthropic/claude-x'), false)
})
