// The bridge: DSH's own model routes, served on 127.0.0.1 as the OpenAI chat-completions
// subset Unfold's agent speaks. Two routes: GET /v1/models and POST /v1/chat/completions.
// Once DSH's session services are up, the same server also serves Unfold's work-view routes
// under /v1/dsh/ (src/host/dsh-routes.js), behind the same checks.
// Every request needs this start's key, the exact Host the bridge listens on, and no Origin,
// so neither a web page nor another computer reaches it. It runs no tools and takes text only:
// the caller's agent decides what is sent, and DSH's llm service sends it with the person's own
// sign-ins, which never leave DSH. The log has method, path, status and time, never a key, a
// prompt or a reply.
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { createServer } from 'node:http'
import { Refusal as ViewRefusal } from './watch.js'

const MAX_BODY = 4 * 1024 * 1024
const JSON_TYPE = 'application/json; charset=utf-8'

// DSH's stable failure codes as HTTP statuses; any other code is the route's own failure.
const STATUS = {
  RATE_LIMIT: 429,
  QUOTA: 429,
  ACCOUNT_QUOTA: 429,
  NO_ADAPTER: 404,
  CONTEXT_WINDOW_EXCEEDED: 400,
}
const FINISH = { stop: 'stop', 'max-tokens': 'length' }

class Refusal extends Error {
  constructor(status, code, message, type = 'invalid_request_error') {
    super(message)
    Object.assign(this, { status, code, type })
  }
}
const invalid = message => new Refusal(400, 'invalid_request', message)

const digest = text => createHash('sha256').update(text).digest()

// Unfold's own DSH names the port in UNFOLD_BRIDGE_PORT, from its port registry, and trusts an
// endpoint file only on that port. It is taken out of the env at the read, like the state folder.
// undefined when unset, else the number it holds, or the text as given when it holds none: an
// empty value and 0 are none, since a named port is never a random one. startBridge refuses
// anything but 1024-65535, and its refusal shows the text.
export function takePort(env = process.env) {
  if (!Object.hasOwn(env, 'UNFOLD_BRIDGE_PORT')) return undefined
  const text = env.UNFOLD_BRIDGE_PORT
  delete env.UNFOLD_BRIDGE_PORT
  return /^\d+$/.test(text) && Number(text) !== 0 ? Number(text) : text
}
const isPort = port => Number.isInteger(port) && port >= 1024 && port <= 65535
const shown = port => (port === '' ? 'empty' : String(port))

// `assistant` makes a durable assistant message for the history (dsh-llm's
// createAssistantMessage); `missingKeys(providers)` resolves to the provider ids whose key is
// not set (./ready.js), so /v1/models can say which models can run; `log` gets one line per request; `dsh` returns the work-view routes
// once they are up, else undefined. `port` 0 takes any free port; a named one is bound exactly or
// the start is refused (EADDRINUSE when it is taken): no other port is tried.
export async function startBridge({ llm, key, assistant, missingKeys = async () => new Set(), log = () => {}, maxBody = MAX_BODY, host = '127.0.0.1', dsh = () => undefined, port = 0 }) {
  if (port !== 0 && !isPort(port)) throw Object.assign(new Error(`the port is not one from 1024 to 65535 (${shown(port)})`), { code: 'bad_port' })
  const want = digest(`Bearer ${key}`)
  const authorized = header => typeof header === 'string' && timingSafeEqual(digest(header), want)
  let bound = 0

  const server = createServer(async (req, res) => {
    const started = Date.now()
    const path = (req.url ?? '').split('?')[0]
    res.on('close', () => log(`${req.method} ${path} ${res.statusCode} ${Date.now() - started} ms`))
    try {
      if (req.headers.host !== `${host}:${bound}` || req.headers.origin !== undefined) throw new Refusal(403, 'forbidden', 'not served to this origin', 'permission_error')
      if (!authorized(req.headers.authorization)) throw new Refusal(401, 'unauthorized', 'a valid key is required', 'authentication_error')
      if (path === '/v1/models' && req.method === 'GET') return sendJson(res, 200, await models(llm, missingKeys))
      if (path === '/v1/chat/completions' && req.method === 'POST') return await complete(llm, assistant, parseChat(await readBody(req, maxBody)), req, res)
      if (path.startsWith('/v1/dsh/')) {
        const routes = dsh()
        if (!routes) throw new Refusal(404, 'not_found', 'the work view is not up: DSH has no session services yet')
        return await routes.serve(req, res, path, () => readJson(req, maxBody), sendJson)
      }
      throw new Refusal(404, 'not_found', `${req.method} ${path} is not served`)
    } catch (error) {
      const refusal = error instanceof Refusal ? error
        : error instanceof ViewRefusal ? new Refusal(error.status, error.code, error.message)
          : new Refusal(500, 'bridge_error', 'the bridge failed', 'api_error')
      // A body left unread is drained, and the connection closes after the answer.
      if (!req.complete) {
        req.resume()
        if (!res.headersSent) res.setHeader('Connection', 'close')
      }
      if (!res.headersSent) sendJson(res, refusal.status, { error: { message: refusal.message, type: refusal.type, code: refusal.code } })
      else res.end()
    }
  })

  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(port, host, () => {
      server.off('error', reject)
      resolve()
    })
  })
  bound = server.address().port
  return {
    address: server.address().address,
    port: bound,
    close: () => new Promise(resolve => {
      server.close(() => resolve())
      server.closeAllConnections()
    }),
  }
}

function sendJson(res, status, body) {
  res.writeHead(status, { 'Content-Type': JSON_TYPE, 'Cache-Control': 'no-store' })
  res.end(JSON.stringify(body))
}

function readBody(req, maxBody) {
  if (Number(req.headers['content-length'] ?? 0) > maxBody) return Promise.reject(new Refusal(413, 'too_large', 'the request is too large'))
  return new Promise((resolve, reject) => {
    const parts = []
    let size = 0
    req.on('data', chunk => {
      size += chunk.length
      if (size > maxBody) reject(new Refusal(413, 'too_large', 'the request is too large'))
      else parts.push(chunk)
    })
    req.on('end', () => resolve(Buffer.concat(parts).toString('utf8')))
    req.on('error', reject)
  })
}

// A work-view body: one JSON object.
async function readJson(req, maxBody) {
  let body
  try { body = JSON.parse(await readBody(req, maxBody)) } catch (error) {
    if (error instanceof Refusal) throw error
    throw new Refusal(400, 'body_refused', 'the body is not JSON')
  }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) throw new Refusal(400, 'body_refused', 'the body is not a JSON object')
  return body
}

// DeepSeek is not listed: not DeepSeek's provider routes (`deepseek`, `deepseek-official`, …),
// and not a DeepSeek model another route serves (an aggregator's `deepseek/deepseek-chat`).
// Unfold calls DeepSeek directly, so DeepSeek is one row in Unfold, never a second one through
// here. A chat to such a route is still served, for an app that saved one before this.
export const isDeepSeekRoute = id => /^deepseek(?:-|$)/i.test(String(id))
// A DeepSeek model of another route has `deepseek` as a word in its provider's own model id:
// at the start or after a separator, and not followed by a letter (`deepseek/deepseek-chat`,
// `deepseek-ai/DeepSeek-V4-Flash`, `deepseek.v3.2`, `@cf/deepseek-ai/…`, `deepseek-v4-flash`).
// A name that only holds the letters is not one: `notdeepseek-x`, `foo/deepseekish`.
export const isDeepSeekModel = id => /(?:^|[^a-z0-9])deepseek(?![a-z])/i.test(String(id))
// The level ids that mean no thinking, in the vocabularies DSH's adapters use.
const OFF = new Set(['off', 'none'])

// A model's thinking levels, least first, as its adapter lists them; none when it has no level
// control or cannot say.
async function levelsOf(llm, provider, model) {
  const info = await Promise.resolve(llm.resolveModelInfo?.(provider, model)).catch(() => undefined)
  return (info?.reasoning?.efforts ?? []).map(effort => effort?.id).filter(id => typeof id === 'string' && id !== '')
}

// Every model of every provider route but DeepSeek's, as `provider/model`, with `ready` (its
// provider's key is set: a model whose key is missing fails every call) and `levels` (the
// thinking levels it really has, least first; empty when it has none). A route whose catalog
// fails is left out, as DSH's own pickers do.
async function models(llm, missingKeys) {
  const providers = llm.listProviders().filter(provider => !isDeepSeekRoute(provider.id))
  const missing = await Promise.resolve(missingKeys(providers.map(provider => provider.id))).catch(() => new Set())
  const data = []
  for (const provider of providers) {
    let listed
    try { listed = (await llm.listModels(provider.id)).filter(model => !isDeepSeekModel(model.id)) } catch { continue }
    const levels = await Promise.all(listed.map(model => levelsOf(llm, provider.id, model.id)))
    listed.forEach((model, at) => {
      data.push({ id: `${provider.id}/${model.id}`, object: 'model', owned_by: provider.id, name: model.name ?? model.id, provider_name: provider.name ?? provider.id, ready: !missing.has(provider.id), levels: levels[at] })
    })
  }
  return { object: 'list', data }
}

// The level a call runs at, always explicit for a model that has levels, so no adapter default
// decides. A model with no level control gets none, never a guessed one. With levels: the one
// asked for when the model has it. No thinking asked (`none`, which Unfold's agent sends on this
// route, `off`, `chat_template_kwargs.enable_thinking: false`, or no level at all) is the model's
// own off level (`off` or `none`), else its lowest. Any other level the model does not have is
// refused before any call.
async function effortFor(llm, chat) {
  const levels = await levelsOf(llm, chat.provider, chat.id)
  if (levels.length === 0) return undefined
  if (chat.effort === undefined || OFF.has(chat.effort)) return levels.find(id => OFF.has(id)) ?? levels[0]
  if (levels.includes(chat.effort)) return chat.effort
  throw invalid(`reasoning_effort ${chat.effort} is not a level of ${chat.model} (its levels: ${levels.join(', ')})`)
}

// The text of a message's content: a string, or text parts only.
function textOf(content) {
  if (typeof content === 'string') return content
  if (Array.isArray(content) && content.every(part => part?.type === 'text' && typeof part.text === 'string')) return content.map(part => part.text).join('')
  throw invalid('only text content is served')
}

// The request as one llm.stream call's options (without the history's assistant messages,
// which need the route: `turns` keeps them as text).
function parseChat(raw) {
  let body
  try { body = JSON.parse(raw) } catch { throw invalid('the body is not JSON') }
  if (body === null || typeof body !== 'object' || Array.isArray(body)) throw invalid('the body is not a JSON object')
  const slash = typeof body.model === 'string' ? body.model.indexOf('/') : -1
  if (slash <= 0 || slash === body.model.length - 1) throw invalid('model must be provider/model, as GET /v1/models lists it')
  if ((Array.isArray(body.tools) && body.tools.length > 0) || (Array.isArray(body.functions) && body.functions.length > 0)) throw invalid('tools are not served')
  if (!Array.isArray(body.messages) || body.messages.length === 0) throw invalid('messages must be a non-empty array')
  const system = []
  const turns = []
  for (const message of body.messages) {
    const role = message?.role
    if (role === 'system' || role === 'developer') system.push(textOf(message.content))
    else if (role === 'user') turns.push({ role, text: textOf(message.content) })
    else if (role === 'assistant' && !message.tool_calls && !message.function_call) turns.push({ role, text: textOf(message.content) })
    else throw invalid(`a ${role ?? 'message'} without text is not served`)
  }
  if (turns.length === 0) throw invalid('messages need a user turn')
  const maxTokens = body.max_completion_tokens ?? body.max_tokens
  if (maxTokens !== undefined && !(Number.isInteger(maxTokens) && maxTokens > 0)) throw invalid('max_tokens must be a positive integer')
  if (body.temperature !== undefined && typeof body.temperature !== 'number') throw invalid('temperature must be a number')
  if (body.reasoning_effort !== undefined && body.reasoning_effort !== null && (typeof body.reasoning_effort !== 'string' || body.reasoning_effort.trim() === '')) throw invalid('reasoning_effort must be a level name')
  // Thinking switched off the chat-template way reads as the off level.
  const effort = body.chat_template_kwargs?.enable_thinking === false ? 'off' : (typeof body.reasoning_effort === 'string' ? body.reasoning_effort.trim().toLowerCase() : undefined)
  const stop = typeof body.stop === 'string' ? [body.stop] : body.stop
  if (stop !== undefined && stop !== null && !(Array.isArray(stop) && stop.every(s => typeof s === 'string'))) throw invalid('stop must be a string or strings')
  return {
    model: body.model,
    provider: body.model.slice(0, slash),
    id: body.model.slice(slash + 1),
    stream: body.stream === true,
    usage: body.stream_options?.include_usage === true,
    system: system.join('\n\n'),
    turns,
    ...(body.temperature !== undefined ? { temperature: body.temperature } : {}),
    ...(maxTokens !== undefined ? { maxTokens } : {}),
    ...(stop?.length ? { stop } : {}),
    ...(effort !== undefined ? { effort } : {}),
  }
}

const usageOf = u => {
  const prompt = (u.inputTokens ?? 0) + (u.cacheReadTokens ?? 0) + (u.cacheWriteTokens ?? 0)
  const completion = u.outputTokens ?? 0
  return { prompt_tokens: prompt, completion_tokens: completion, total_tokens: prompt + completion }
}

// A failure keeps only DSH's stable code: the provider's own words can carry account details.
function failed(reason) {
  const code = /^[A-Z][A-Z0-9_]{0,63}$/.test(reason.failure?.code ?? '') ? reason.failure.code : 'UPSTREAM_ERROR'
  return new Refusal(STATUS[code] ?? 502, code, `the DSH route failed (${code})`, 'api_error')
}

async function complete(llm, assistant, chat, req, res) {
  const reasoningEffort = await effortFor(llm, chat)
  const abort = new AbortController()
  res.on('close', () => { if (!res.writableFinished) abort.abort() })
  const messages = chat.turns.map(turn => turn.role === 'user'
    ? { role: 'user', content: [{ type: 'text', text: turn.text }] }
    : assistant({ content: [{ type: 'text', text: turn.text }], source: { provider: chat.provider, model: chat.id } }))
  const stream = llm.stream({
    provider: chat.provider,
    model: chat.id,
    messages,
    ...(chat.system ? { system: chat.system } : {}),
    ...(chat.temperature !== undefined ? { temperature: chat.temperature } : {}),
    ...(chat.maxTokens !== undefined ? { maxTokens: chat.maxTokens } : {}),
    ...(chat.stop ? { stop: chat.stop } : {}),
    ...(reasoningEffort !== undefined ? { reasoningEffort } : {}),
    signal: abort.signal,
  })
  const id = `chatcmpl-${randomBytes(12).toString('hex')}`
  const created = Math.floor(Date.now() / 1000)
  const frame = (choices, extra = {}) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created, model: chat.model, choices, ...extra })}\n\n`
  let usage
  let text = ''
  let opened = false
  // The status is decided by the first token or the finish, so a route that fails at once
  // answers with an HTTP error, not a stream.
  const open = () => {
    if (opened) return
    opened = true
    res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8', 'Cache-Control': 'no-store' })
  }
  for await (const chunk of stream) {
    if (chunk.type === 'text-delta' && chunk.text) {
      if (chat.stream) {
        const first = !opened
        open()
        res.write(frame([{ index: 0, delta: first ? { role: 'assistant', content: chunk.text } : { content: chunk.text }, finish_reason: null }]))
      }
      text += chunk.text
    } else if (chunk.type === 'usage') {
      usage = usageOf(chunk.usage)
    } else if (chunk.type === 'finish') {
      const { reason } = chunk
      if (reason.kind === 'aborted' && abort.signal.aborted) return res.end()
      if (reason.kind === 'error' || reason.kind === 'aborted') {
        const refusal = failed(reason)
        if (!opened) throw refusal
        res.end(`data: ${JSON.stringify({ error: { message: refusal.message, type: refusal.type, code: refusal.code } })}\n\n`)
        return
      }
      const finish = FINISH[reason.kind] ?? 'stop'
      if (!chat.stream) {
        return sendJson(res, 200, { id, object: 'chat.completion', created, model: chat.model, choices: [{ index: 0, message: { role: 'assistant', content: text }, finish_reason: finish }], ...(usage ? { usage } : {}) })
      }
      if (!opened) {
        open()
        res.write(frame([{ index: 0, delta: { role: 'assistant', content: '' }, finish_reason: null }]))
      }
      res.write(frame([{ index: 0, delta: {}, finish_reason: finish }]))
      if (chat.usage && usage) res.write(frame([], { usage }))
      res.end('data: [DONE]\n\n')
      return
    }
    // reasoning and tool-call deltas are not served (text only)
  }
  // a stream always ends with a finish; one that does not is the route's failure
  throw failed({})
}
