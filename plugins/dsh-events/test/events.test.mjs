import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { SessionProjectionRegistry } from '@deepseek-ai/dsh-session-projection'
import * as plugin from '../index.mjs'
import { loadCatalog } from '../src/catalog.mjs'
import { AgentdObserver, MAX_FRAME } from '../src/observer.mjs'
import { PROJECTION_KEY, fold, initialState, projectionDefinition } from '../src/projection.mjs'
import { EventWindow, patch, redactText } from '../src/window.mjs'
import { needRth, rthFile } from '../../../test/rth-checkout.mjs'

const catalog = loadCatalog()
const fixture = (name) => new URL(`./fixtures/${name}.jsonl`, import.meta.url)
const settle = () => new Promise((r) => setImmediate(r))

/** Every envelope of a recorded bench case, in both directions, in wire order. */
function envelopes(name) {
  return readFileSync(fixture(name), 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line))
    .filter((rec) => (rec.rec === 'recv' || rec.rec === 'sent') && rec.env)
    .map((rec) => rec.env)
}

const audio = (i, { response = 'r1', cause = 'g:decision', bytes = 4800 } = {}) => ({
  v: 2, id: `g:${response}:${i}`, producer: 'g', seq: i, t_ns: i, source: 'guest', kind: 'agent_audio', cause,
  response_id: response, chunk: i, sample_offset: i * (bytes / 2), sample_rate: 24000, format: 's16le', data: Buffer.alloc(bytes).toString('base64'),
})

test('the catalog names every event kind the protocol defines', needRth('crates/rth-protocol/src/lib.rs'), () => {
  const src = readFileSync(rthFile('crates/rth-protocol/src/lib.rs'), 'utf8') // rth's protocol
  const body = src.slice(src.indexOf('pub enum Event {'), src.indexOf('\n}\n', src.indexOf('pub enum Event {')))
  const kinds = [...body.matchAll(/^\s{4}([A-Z][A-Za-z]+)\s*\{/gm)].map((m) => m[1].replace(/(?<!^)([A-Z])/g, '_$1').toLowerCase())
  assert.ok(kinds.length >= 14, `parsed ${kinds.length} kinds`)
  const missing = kinds.filter((k) => !catalog.entry(k))
  assert.deepEqual(missing, [], 'add these wire kinds to catalog/events.v3.json')
})

test('a cancelled reply reads as plain facts with its causal chain', () => {
  const w = new EventWindow(catalog)
  for (const env of envelopes('tc-dev-05-cancel-after-first-delta')) w.push(env)
  const names = w.rows.map((r) => r.name)
  assert.deepEqual(names, [
    'agent_available', 'connection_requested', 'connection_accepted', 'message_submitted', 'decision_made',
    'message_started', 'message_text_added', 'message_stop_requested', 'message_finished',
  ])
  const finished = w.rows.at(-1)
  assert.match(finished.summary, / stopped$/)
  assert.equal(finished.v, 2, 'rendered rows keep the wire version')
  const { rows, unresolved } = w.chain(finished.id)
  assert.deepEqual(rows.map((r) => r.name), ['message_finished', 'decision_made', 'message_submitted'])
  assert.equal(unresolved, null, 'with both directions recorded, the chain is complete')
  const decision = w.rows.find((r) => r.name === 'decision_made')
  assert.deepEqual(w.causedBy(decision.id).map((r) => r.name), ['message_started', 'message_text_added', 'message_finished'])
  assert.equal(w.unknown.size, 0)
})

test('the live source omits host frames, and the chain says where it stops', () => {
  const w = new EventWindow(catalog)
  for (const env of envelopes('tc-dev-05-cancel-after-first-delta').filter((e) => e.source === 'guest')) w.push(env)
  const finished = w.rows.at(-1)
  const { rows, unresolved } = w.chain(finished.id)
  assert.deepEqual(rows.map((r) => r.name), ['message_finished', 'decision_made'])
  assert.equal(unresolved.reason, 'not_observed', 'the trigger was a host frame the observer never sees')
  assert.match(unresolved.id, /-a1:2$/)
})

test('text of one reply coalesces into one row; two replies stay apart', () => {
  const w = new EventWindow(catalog)
  const envs = envelopes('tc-dev-03-second-input-reactive')
  for (const env of envs) w.push(env)
  const deltas = envs.filter((e) => e.kind === 'agent_delta')
  const textRows = w.rows.filter((r) => r.name === 'message_text_added')
  assert.equal(textRows.length, new Set(deltas.map((e) => e.response_id)).size)
  assert.equal(textRows.reduce((n, r) => n + r.count, 0), deltas.length)
  for (const row of textRows) {
    const full = deltas.filter((e) => e.response_id === row.fields.response_id).map((e) => e.text).join('')
    assert.equal(row.text, full.slice(-240))
  }
})

test('audio coalesces into counters and the pane never keeps samples', () => {
  const w = new EventWindow(catalog)
  for (let i = 0; i < 5; i++) w.push(audio(i))
  assert.equal(w.rows.length, 1)
  assert.equal(w.rows[0].audio.chunks, 5)
  assert.equal(Math.round(w.rows[0].audio.ms), 500)
  assert.deepEqual([w.rows[0].audio.sample_start, w.rows[0].audio.sample_end, w.rows[0].audio.gaps], [0, 12000, 0])
  assert.equal(w.rows[0].fields.data, undefined)
})

test('10,000 audio chunks in one reply keep every collection and the row payload bounded', () => {
  const w = new EventWindow(catalog, { limit: 3, idSample: 8, effectsPerCause: 4 })
  w.push({ v: 2, id: 'g:decision', producer: 'g', seq: 0, t_ns: 0, source: 'guest', kind: 'decision', selected: 'respond' })
  const updates = []
  w.onChange((change, payload) => change === 'updated' && updates.push(JSON.stringify(payload).length))
  for (let i = 0; i < 10_000; i++) w.push(audio(i, { bytes: 480 }))
  const row = w.rows.find((r) => r.name === 'message_audio_added')
  assert.equal(w.rows.length, 2)
  assert.deepEqual([row.count, row.audio.chunks, row.audio.gaps], [10_000, 10_000, 0])
  assert.deepEqual([row.first_chunk ?? row.audio.first_chunk, row.audio.last_chunk, row.audio.sample_end], [0, 9_999, 10_000 * 240])
  assert.deepEqual([row.ids.length, row.ids_omitted, row.last_id], [8, 9_992, 'g:r1:9999'])
  assert.ok(w.byId.size <= 2 + 8 + 1, `byId ${w.byId.size}`)
  assert.equal(w.effects.size, 1)
  assert.equal(w.effects.get('g:decision').keys.size, 1, 'a coalesced row is one effect, not one per chunk')
  assert.equal(w.open.size, 1)
  assert.ok(JSON.stringify(row).length < 2048, `row JSON ${JSON.stringify(row).length} bytes`)
  assert.ok(Math.max(...updates) < 1024, 'SSE sends a bounded patch, never the growing row')
  assert.ok(updates.at(-1) - updates[100] < 40, 'the patch for chunk 10,000 grows only by digit width over chunk 100')
})

test('many evicted replies leave no lineage, effects or open rows behind', () => {
  const w = new EventWindow(catalog, { limit: 3, effectsPerCause: 4 })
  for (let r = 0; r < 200; r++) {
    const decision = `g:d${r}`
    w.push({ v: 2, id: decision, producer: 'g', seq: r * 100, t_ns: 0, source: 'guest', kind: 'decision', selected: 'respond' })
    for (let i = 0; i < 50; i++) w.push({ ...audio(i, { response: `r${r}`, cause: decision, bytes: 48 }), seq: r * 100 + i + 1 })
    if (r % 2) w.push({ v: 2, id: `g:done${r}`, producer: 'g', seq: r * 100 + 60, t_ns: 0, source: 'guest', kind: 'agent_done', cause: decision, response_id: `r${r}`, status: 'completed' })
  }
  assert.equal(w.rows.length, 3)
  const retained = new Set(w.rows)
  assert.ok([...w.byId.values()].every((row) => retained.has(row)), 'byId names only retained rows')
  assert.ok(w.byId.size <= 3 * 10, `byId ${w.byId.size}`)
  assert.ok(w.effects.size <= 3, `effects ${w.effects.size}`)
  assert.ok([...w.effects.values()].every((e) => [...e.keys].every((k) => w.byId.has(k))), 'effects name only retained rows')
  assert.ok(w.open.size <= 1, `open ${w.open.size}`)
  assert.equal(w.effects.has('g:d0'), false)
  assert.equal(w.chain('g:r0:3').rows.length, 0)
  const old = w.chain(w.rows.find((r) => r.name === 'message_audio_added')?.id ?? w.rows.at(-1).id)
  assert.ok(old.unresolved === null || ['evicted', 'missing'].includes(old.unresolved.reason))
})

test('a microphone stream coalesces into one row that its end closes', () => {
  const w = new EventWindow(catalog)
  const mic = (i) => ({ v: 2, id: `h:${i}`, producer: 'h', seq: i, t_ns: i, source: 'host', kind: 'user_audio', stream_id: 's1', chunk: i, sample_offset: i * 240, sample_rate: 24000, format: 's16le', data: Buffer.alloc(480).toString('base64') })
  for (let i = 0; i < 3; i++) w.push(mic(i))
  assert.deepEqual([w.rows.length, w.rows[0].name, w.rows[0].audio.chunks, w.open.size], [1, 'input_audio_added', 3, 1])
  w.push({ v: 2, id: 'h:9', producer: 'h', seq: 9, t_ns: 9, source: 'host', kind: 'user_audio_end', stream_id: 's1', chunks: 3, samples: 720, dropped_chunks: 0 })
  assert.equal(w.open.size, 0)
  w.push(mic(4))
  assert.equal(w.rows.length, 3, 'audio after the end starts a new row')
})

test('unknown kinds pass through flagged; heartbeats are status, not rows', () => {
  const w = new EventWindow(catalog)
  w.push({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'frame_seen', camera: 'front' })
  w.push({ v: 2, id: 'g:~', producer: 'g', seq: 0, t_ns: 2, source: 'guest', kind: 'heartbeat', uptime_ms: 5, attachments: 1 })
  assert.equal(w.rows.length, 1)
  assert.equal(w.rows[0].known, false)
  assert.equal(w.rows[0].name, 'frame_seen')
  assert.deepEqual([...w.unknown], ['frame_seen'])
  assert.equal(w.status.name, 'agent_status_reported')
})

test('the window is bounded and forgets dropped ids', () => {
  const w = new EventWindow(catalog, { limit: 3 })
  for (let i = 1; i <= 10; i++) w.push({ v: 2, id: `g:${i}`, producer: 'g', seq: i, t_ns: i, source: 'guest', kind: 'log', level: 'info', msg: `n${i}`, cause: i > 1 ? `g:${i - 1}` : undefined })
  assert.equal(w.rows.length, 3)
  assert.equal(w.dropped, 7)
  assert.equal(w.byId.size, 3)
  assert.equal(w.chain('g:2').rows.length, 0)
  assert.deepEqual(w.chain('g:10').unresolved, { id: 'g:7', reason: 'evicted' })
  assert.equal(w.effects.size, 3, 'edges of evicted rows are pruned')
})

test('the aggregate fold is pure and its schemas validate', () => {
  const def = projectionDefinition({ catalog })
  const s0 = def.init({}, 0)
  assert.equal(def.apply(s0, { type: 'todo/upsert', data: {} }), s0, 'irrelevant events keep the reference')
  let s = s0
  for (const env of envelopes('tc-dev-05-cancel-after-first-delta')) s = def.apply(s, { type: 'rth/event', data: env })
  assert.deepEqual(s0, initialState(), 'never mutated')
  assert.deepEqual(s.finished, { completed: 0, stopped: 1, failed: 0 })
  assert.deepEqual(s.open, {})
  assert.equal(s.lastDecision.selected, 'respond')
  assert.deepEqual(def.stateSchema.parse(s), s, 'a folded state round-trips through its schema')
  assert.throws(() => def.stateSchema.parse({ ...s, events: -1 }))
  assert.equal(def.wire.view(s), s)
  const hb = { v: 2, id: 'g:~', seq: 0, kind: 'heartbeat', source: 'guest', uptime_ms: 1, attachments: 0 }
  assert.equal(fold(s, hb, catalog), s, 'ephemeral events are not history')
})

/** A Session the real registry can read: `seq` is the next sequence number. */
function fakeSession() {
  const events = []
  return {
    events,
    header: { id: 's1' },
    inheritedEventCount: 0,
    get seq() { return events.length },
    eventAt: (seq) => events[seq],
    snapshotEvents: (from = 0, to = events.length) => events.slice(from, to),
  }
}

test('the projection runs in the real dsh-session-projection registry: view, change, checkpoint, restore, dispose', async () => {
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  await settle()
  const registry = ctx.get('sessionProjections')
  const dispose = registry.register(projectionDefinition({ catalog }))
  const session = fakeSession()
  const changes = []
  registry.onChanged((s, key, value, seq) => changes.push({ key, seq, events: value.events }))
  const commit = (type, data) => {
    const event = { seq: session.events.length, type, data }
    session.events.push(event)
    ctx.emit('session/event', session, event)
  }
  const envs = envelopes('tc-dev-05-cancel-after-first-delta')
  envs.forEach((env, i) => {
    commit('rth/event', env)
    if (i === 3) commit('todo/upsert', { id: 'x' })
  })
  let expected = initialState()
  for (const env of envs) expected = fold(expected, env, catalog)

  const snap = registry.snapshot(session)
  assert.equal(snap.asOfSeq, envs.length)
  assert.deepEqual(snap.values[PROJECTION_KEY], expected)
  assert.equal(changes.length, envs.length, 'one change per RTH event; the unrelated event changes nothing')
  assert.deepEqual(changes.at(-1), { key: PROJECTION_KEY, seq: envs.length, events: envs.length })

  // Persist a checkpoint halfway, then restore from it over the stored tail.
  const half = fakeSession()
  half.events.push(...session.events.slice(0, 5))
  const rows = JSON.parse(JSON.stringify(registry.checkpoint(half)))
  const floor = registry.restoreFloor(rows)
  const restored = registry.restore(rows, session.events.slice(floor), floor, session.header, 0)
  assert.deepEqual(restored.snapshot.values[PROJECTION_KEY], expected)
  assert.equal(restored.snapshot.asOfSeq, envs.length)
  assert.deepEqual(registry.viewCheckpoint(restored.checkpoint)[PROJECTION_KEY], expected)
  const corrupt = { [PROJECTION_KEY]: { ...rows[PROJECTION_KEY], val: { ...rows[PROJECTION_KEY].val, events: 'many' } } }
  assert.equal(registry.viewCheckpoint(corrupt)[PROJECTION_KEY], undefined, 'the state schema rejects a malformed row')

  await dispose()
  assert.equal(registry.registrations.has(PROJECTION_KEY), false)
  assert.deepEqual(registry.snapshot(session).values, {})
})

/**
 * A TCP stand-in for agentd that writes `chunks` (gapMs apart) and reports what the observer
 * did once the connection closes. Without `onAttach` the Attach is read and discarded, so the
 * half-close completes; every run has a hard timeout and always stops the observer and server.
 */
async function feed(chunks, { gapMs = 25, onAttach, timeoutMs = 5000 } = {}) {
  const got = []
  const states = []
  let observer
  let server
  let timer
  try {
    return await new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error(`feed timed out after ${timeoutMs} ms`)), timeoutMs)
      server = net.createServer(async (sock) => {
        sock.on('error', () => {})
        sock.on('close', () => resolve({ got, states, rejected: observer.rejected, producer: observer.producer }))
        if (onAttach) sock.once('data', (line) => onAttach(sock, JSON.parse(line)))
        else sock.resume()
        for (const c of chunks) {
          if (sock.destroyed) break
          sock.write(c)
          await new Promise((r) => setTimeout(r, gapMs))
        }
        if (!onAttach) sock.end()
      })
      server.listen(0, '127.0.0.1', () => {
        observer = new AgentdObserver({ port: server.address().port, onEnvelope: (e) => got.push(e), onState: (s, d) => states.push([s, d]) }).start()
      })
    })
  } finally {
    clearTimeout(timer)
    observer?.stop()
    server?.close()
  }
}

/** A valid hello frame of exactly `bytes` UTF-8 bytes (newline excluded), padded with `fill`. */
function helloOf(bytes, fill = 'x') {
  const base = { v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'hello', agent: 'rth-agentd', pad: '' }
  const room = bytes - Buffer.byteLength(JSON.stringify(base))
  const unit = Buffer.byteLength(fill)
  base.pad = fill.repeat(Math.floor(room / unit)) + 'x'.repeat(room % unit)
  const line = JSON.stringify(base)
  assert.equal(Buffer.byteLength(line), bytes)
  return line
}

test('the frame cap is MAX_FRAME UTF-8 bytes: exact boundary passes, one more byte does not', async () => {
  const at = await feed([`${helloOf(MAX_FRAME)}\n`])
  assert.deepEqual([at.got.length, at.got[0].kind, at.rejected], [1, 'hello', 0])
  const over = await feed([`${helloOf(MAX_FRAME + 1)}\n`])
  assert.deepEqual([over.got.length, over.rejected], [0, 1])
})

test('an oversize frame split across reads is refused before it is parsed', async () => {
  const line = Buffer.from(`${helloOf(80_069)}\n`)
  const { got, rejected, states } = await feed([line.subarray(0, 45_000), line.subarray(45_000)])
  assert.deepEqual([got.length, rejected], [0, 1])
  assert.ok(states.some(([s, d]) => s === 'error' && /80069 bytes exceeds 65536/.test(d)))
})

test('multibyte text is measured in bytes and survives a split mid-character', async () => {
  const wide = helloOf(MAX_FRAME + 2, '€')
  assert.ok(wide.length <= MAX_FRAME, 'fits in UTF-16 units, so a string-length check would pass it')
  const over = await feed([`${wide}\n`])
  assert.deepEqual([over.got.length, over.rejected], [0, 1])
  const line = Buffer.from(`${helloOf(1000, '€')}\n`)
  const cut = line.indexOf(Buffer.from('€')) + 1
  const ok = await feed([line.subarray(0, cut), line.subarray(cut)])
  assert.equal(ok.got.length, 1)
  assert.match(ok.got[0].pad, /^€+x*$/)
})

test('attached means agentd granted observer to our attachment', async () => {
  const hello = `${JSON.stringify({ v: 2, id: 'g:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'hello' })}\n`
  const ready = (attach, role) => `${JSON.stringify({ v: 2, id: 'g:2', producer: 'g', seq: 2, t_ns: 2, source: 'guest', kind: 'ready', attachment_id: attach.attachment_id, role })}\n`
  const promoted = await feed([hello], { onAttach: (sock, attach) => sock.write(ready(attach, 'controller')) })
  assert.ok(!promoted.states.some(([s]) => s === 'attached'), 'a controller grant is not an observer attachment')
  assert.ok(promoted.states.some(([s, d]) => s === 'error' && /granted controller/.test(d)))
  const other = await feed([hello], { onAttach: (sock) => sock.write(ready({ attachment_id: 'someone-else' }, 'observer'), () => sock.end()) })
  assert.ok(!other.states.some(([s]) => s === 'attached'), "another attachment's grant is not ours")
})

test('the observer attaches read-only, receives pushed events, and reconnects', async () => {
  const envs = envelopes('tc-dev-05-cancel-after-first-delta').filter((e) => e.source === 'guest')
  envs[2] = { ...envs[2], received: { by: 'agentd', t_ns: 7 } }
  const attaches = []
  const server = net.createServer((sock) => {
    sock.setEncoding('utf8')
    sock.write(`${JSON.stringify(envs[0])}\n`)
    sock.once('data', (line) => {
      const attach = JSON.parse(line)
      attaches.push(attach)
      const ready = { ...envs[1], attachment_id: attach.attachment_id, role: 'observer' }
      for (const env of [ready, ...envs.slice(2)]) sock.write(`${JSON.stringify(env)}\n`)
      if (attaches.length === 1) sock.end()
    })
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const got = []
  const states = []
  let observer
  const second = new Promise((resolve) => {
    observer = new AgentdObserver({
      port: server.address().port,
      onEnvelope: (e) => (got.push(e), attaches.length === 2 && got.length === envs.length * 2 && resolve()),
      onState: (s) => states.push(s),
    })
  })
  observer.start()
  await second
  observer.stop()
  server.close()
  assert.equal(attaches[0].role, 'observer')
  assert.equal(attaches[0].kind, 'attach')
  assert.notEqual(attaches[0].attachment_id, attaches[1].attachment_id)
  assert.ok(got.every((e) => e.observed?.by === observer.producer), 'our receipt is `observed`')
  assert.deepEqual(got[2].received, { by: 'agentd', t_ns: 7 }, 'the original received stamp is kept')
  assert.equal(catalog.describe(got[2]).received.by, 'agentd')
  assert.ok(states.includes('attached') && states.includes('waiting'))
})

/** The plugin under real Cordis with the real projection registry, its routes on a real HTTP server. */
async function loaded(config = {}) {
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  const registered = []
  ctx.provide('webServer', { register: (route) => (registered.push(route), () => registered.splice(registered.indexOf(route), 1)) })
  const fiber = ctx.plugin(plugin, { agentd: { enabled: false }, transport: 'webServer', ...config })
  await fiber
  await settle()
  const server = http.createServer((req, res) => {
    const route = registered.find((r) => r.path === req.url)
    if (!route) return res.writeHead(404).end()
    route.handler(req, res)
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const service = ctx.get('rthEvents')
  return { ctx, fiber, registered, service, hub: service.streams, port: server.address().port, close: () => server.closeAllConnections() || server.close() }
}

/** A real SSE reader over HTTP; `next(pred)` resolves with the first matching event. */
function reader(port, path = '/api/rth-events/stream') {
  const events = []
  const waiters = new Set()
  const client = { events, ended: false }
  client.next = (pred, ms = 5000) => new Promise((resolve, reject) => {
    const hit = events.find(pred)
    if (hit) return resolve(hit)
    const w = { pred, resolve, timer: setTimeout(() => (waiters.delete(w), reject(new Error('no matching event'))), ms) }
    waiters.add(w)
  })
  client.req = http.get({ host: '127.0.0.1', port, path }, (res) => {
    let buf = ''
    res.setEncoding('utf8')
    res.on('data', (chunk) => {
      buf += chunk
      for (let i; (i = buf.indexOf('\n\n')) >= 0; buf = buf.slice(i + 2)) {
        const text = buf.slice(0, i)
        const ev = { event: /^event: (.*)$/m.exec(text)[1], data: JSON.parse(/^data: (.*)$/m.exec(text)[1]) }
        events.push(ev)
        for (const w of [...waiters]) if (w.pred(ev)) (waiters.delete(w), clearTimeout(w.timer), w.resolve(ev))
      }
    })
    res.on('close', () => (client.ended = true))
  })
  client.req.on('error', () => {})
  return client
}

/** A client that sends its request and then never reads a byte (so it never sees its own close). */
function stalled(port, path = '/api/rth-events/stream') {
  const sock = net.connect(port, '127.0.0.1', () => sock.write(`GET ${path} HTTP/1.1\r\nHost: test\r\n\r\n`))
  sock.pause()
  sock.on('error', () => {})
  return sock
}

/** Resolves once the hub holds `n` clients, re-checking on each new connection's first turn. */
const clientsReach = (hub, n) => new Promise((resolve) => {
  const check = () => (hub.clients.size >= n ? resolve() : setTimeout(check, 10))
  check()
})

const note = (i, msg = 'x'.repeat(1000)) => ({ v: 2, id: `n:${i}`, producer: 'n', seq: i, t_ns: i, source: 'guest', kind: 'log', level: 'info', msg })
const tick = () => new Promise((r) => setTimeout(r, 1))

test('loads as a Cordis plugin: routes, projection, link state, patches and unload', async () => {
  const { fiber, registered, service, hub, port, ctx, close } = await loaded()
  assert.deepEqual(registered.map((r) => r.path), ['/api/rth-events', '/api/rth-events/stream', '/api/rth-presence', '/api/rth-presence/stream'])
  assert.ok(ctx.get('sessionProjections').registrations.has(PROJECTION_KEY), 'registered into the real registry')
  const pane = reader(port)
  const other = reader(port)
  await pane.next((e) => e.event === 'snapshot')
  await other.next((e) => e.event === 'snapshot')
  other.req.destroy()
  for (const env of envelopes('tc-dev-05-cancel-after-first-delta')) service.ingest(env)
  assert.equal(service.snapshot().aggregates.finished.stopped, 1, 'closing a pane leaves ingestion alive')
  await pane.next((e) => e.event === 'added' && e.data.name === 'message_finished')

  service.setLink('attached', 'observer')
  assert.deepEqual((await pane.next((e) => e.event === 'link')).data, { state: 'attached', detail: 'observer' })
  assert.equal(service.snapshot().source.host_frames, false, 'the snapshot says the live source omits host frames')

  service.ingest(audio(0, { response: 'rA' }))
  service.ingest(audio(1, { response: 'rA' }))
  const updated = await pane.next((e) => e.event === 'updated' && e.data.count === 2)
  assert.deepEqual(updated.data, JSON.parse(JSON.stringify(patch(service.window.rows.at(-1)))))
  assert.equal(updated.data.fields, undefined, 'an update is a patch, not the row')

  await fiber.dispose()
  assert.equal(registered.length, 0)
  assert.equal((await pane.next((e) => e.event === 'resnapshot')).data.reason, 'unloaded', 'unload ends open streams')
  assert.equal(hub.clients.size, 0)
  assert.equal(ctx.get('sessionProjections').registrations.has(PROJECTION_KEY), false)
  const before = pane.events.length
  service.ingest(audio(2, { response: 'rA' }))
  await tick()
  assert.equal(pane.events.length, before, 'no listener outlives the plugin')
  close()
})

test('a baseline larger than the socket high-water mark arrives, then live updates, with no reconnect', async () => {
  const { fiber, service, hub, port, close } = await loaded()
  for (let i = 0; i < 200; i++) service.ingest(note(i))
  assert.ok(Buffer.byteLength(JSON.stringify(service.snapshot())) > 200_000)
  const pane = reader(port)
  const snapshot = await pane.next((e) => e.event === 'snapshot')
  assert.equal(snapshot.data.rows.length, 200)
  assert.equal(hub.clients.size, 1, 'backpressure on the baseline is not a stall')
  await tick()
  service.ingest(note(200, 'later'))
  await pane.next((e) => e.event === 'added' && e.data.fields.msg === 'later')
  assert.deepEqual([pane.events.filter((e) => e.event === 'resnapshot').length, pane.ended, hub.clients.size], [0, false, 1])
  await fiber.dispose()
  close()
})

test('a stalled client is dropped under its bounds while ingestion and a healthy client go on', async () => {
  const stream = { maxPendingBytes: 256 * 1024, maxPendingEvents: 10_000, drainTimeoutMs: 500 }
  const { fiber, service, hub, port, close } = await loaded({ stream })
  const healthy = reader(port)
  await healthy.next((e) => e.event === 'snapshot')
  const dead = stalled(port)
  await clientsReach(hub, 2)
  const [, deadClient] = hub.clients
  let blockedAt = null // when the stalled client last started waiting for drain
  let i = 0
  while (!hub.drops.stalled && i < 40_000) {
    for (let k = 0; k < 20; k++) service.ingest(note(i++))
    if (!deadClient.waiting) blockedAt = null
    else blockedAt ??= Date.now()
    await tick()
  }
  const waited = Date.now() - blockedAt
  assert.equal(hub.drops.stalled, 1, `the stalled client was never dropped after ${i} rows`)
  assert.ok(deadClient.res.destroyed, 'the stalled connection is cut')
  assert.ok(waited <= stream.drainTimeoutMs + 1000, `dropped ${waited} ms after it stopped draining`)
  assert.ok(deadClient.queued <= stream.maxPendingBytes + 4096, `${deadClient.queued} bytes queued at drop`)
  service.ingest(note(i, 'after'))
  await healthy.next((e) => e.event === 'added' && e.data.fields.msg === 'after')
  assert.deepEqual([healthy.events.filter((e) => e.event === 'resnapshot').length, hub.clients.size], [0, 1])
  assert.equal(service.window.rows.length, Math.min(2000, i + 1), 'ingestion never waited on a client')
  await fiber.dispose()
  dead.destroy()
  close()
})

test('unload cancels a pending drain and closes the waiting client', async () => {
  const stream = { maxPendingBytes: 512 * 1024 * 1024, maxPendingEvents: 1_000_000, drainTimeoutMs: 600_000 }
  const { fiber, service, hub, port, close } = await loaded({ stream })
  const dead = stalled(port)
  await clientsReach(hub, 1)
  const waiting = () => [...hub.clients].some((c) => c.waiting && c.queue.length)
  for (let i = 0; !waiting() && i < 40_000; ) {
    for (let k = 0; k < 20; k++) service.ingest(note(i++))
    await tick()
  }
  assert.ok(waiting(), 'the client never fell behind')
  const [pending] = hub.clients
  await fiber.dispose()
  assert.deepEqual([hub.clients.size, hub.drops.unloaded, pending.res.destroyed], [0, 1, true])
  // A drain timer left behind would hold this test file open for ten minutes.
  dead.destroy()
  close()
})

/** Reads SSE frames off a Fetch Response body, as DSH's node:http bridge would pull it. */
function fetchReader(response) {
  const events = []
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buf = ''
  return {
    events,
    async next(pred) {
      for (;;) {
        const hit = events.find(pred)
        if (hit) return hit
        const { value, done } = await reader.read()
        if (done) return undefined
        buf += decoder.decode(value, { stream: true })
        for (let i; (i = buf.indexOf('\n\n')) >= 0; buf = buf.slice(i + 2)) {
          const text = buf.slice(0, i)
          events.push({ event: /^event: (.*)$/m.exec(text)[1], data: JSON.parse(/^data: (.*)$/m.exec(text)[1]) })
        }
      }
    },
  }
}

test('DSH transport: routes go to connection.fetch (behind its fence), not a bare /api webServer route', async () => {
  const ctx = new Context()
  await ctx.plugin(SessionProjectionRegistry)
  const fetchRoutes = []
  const bare = []
  ctx.provide('webServer', { register: (route) => (bare.push(route), () => {}) })
  ctx.provide('connection', {
    fetch: { register: (route) => (fetchRoutes.push(route), async () => fetchRoutes.splice(fetchRoutes.indexOf(route), 1)) },
  })
  const fiber = ctx.plugin(plugin, { agentd: { enabled: false } })
  await fiber
  await settle()
  assert.deepEqual(fetchRoutes.map((r) => [r.path, r.methods]), [['/api/rth-events', ['GET']], ['/api/rth-events/stream', ['GET']], ['/api/rth-presence', ['GET']], ['/api/rth-presence/stream', ['GET']], ['/api/rth-presence/growth', ['GET']]])
  assert.equal(bare.length, 0, 'nothing bypasses Connection')
  const service = ctx.get('rthEvents')
  const abort = new AbortController()
  const response = await fetchRoutes[1].fetch(new Request('http://h/api/rth-events/stream', { signal: abort.signal }))
  assert.equal(response.headers.get('content-type'), 'text/event-stream')
  const pane = fetchReader(response)
  assert.equal((await pane.next((e) => e.event === 'snapshot')).data.rows.length, 0)
  service.ingest(note(1, 'pushed'))
  assert.equal((await pane.next((e) => e.event === 'added')).data.fields.msg, 'pushed')
  assert.equal(service.streams.clients.size, 1)
  abort.abort()
  assert.equal(service.streams.clients.size, 0, 'the browser going away removes only its client')
  await fiber.dispose()
  assert.equal(fetchRoutes.length, 0, 'unload removes the routes')
})

// Tail redaction: the joined reply text is redacted as a whole BEFORE the 240-char tail cut, so
// no cut and no delta split shows a credential in part.
const TOKEN = 'Q7vK9mPx2Lw8Rt5Ny3Hb'
const delta = (i, text, response = 'rT') => ({ v: 2, id: `g:${response}:d${i}`, producer: 'g', seq: 1000 + i, t_ns: i, source: 'guest', kind: 'agent_delta', cause: 'g:decision', response_id: response, text })
const fragments = (s, n = 4) => Array.from({ length: s.length - n + 1 }, (_, i) => s.slice(i, i + n))
const leaks = (text) => fragments(TOKEN).filter((f) => text.includes(f))

test('a Bearer token is never shown in part, wherever the 240-char tail cut or a delta split falls', () => {
  const words = (n) => Array.from({ length: n }, (_, i) => ['lorem', 'ipsum', 'dolor', 'sit'][i % 4]).join(' ')
  // Every cut position from before "Bearer " to past the token's end, and several delta sizes.
  for (let after = 200; after <= 250; after++) {
    for (const size of [1, 3, 7, 29]) {
      const full = `${words(40)} Bearer ${TOKEN} ${'z'.repeat(after)}`
      const w = new EventWindow(catalog, { limit: 10 })
      const seen = []
      w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
      for (let i = 0; i * size < full.length; i++) w.push(delta(i, full.slice(i * size, (i + 1) * size)))
      const row = w.rows.find((r) => r.name === 'message_text_added')
      assert.ok(row.text.length <= 240)
      assert.deepEqual(leaks(row.text), [], `cut after ${after}, deltas of ${size}: ${row.text.slice(0, 40)}`)
      assert.deepEqual(leaks(row.summary), [])
      assert.deepEqual(seen.flatMap(leaks), [], 'no emitted row or patch ever carried part of it')
      assert.deepEqual(leaks(JSON.stringify(w.rows)), [], 'the private raw text is never on a row')
    }
  }
})

test('the cut just after "Bearer " and a token split across two deltas both read as redacted', () => {
  const w = new EventWindow(catalog, { limit: 10 })
  // The 240-char tail starts exactly after "Bearer ": the token must not survive without its keyword.
  const tailAfterBearer = `start Bearer ${TOKEN}${' .'.repeat((240 - TOKEN.length) / 2)}`
  w.push(delta(0, tailAfterBearer.slice(0, 10)))
  w.push(delta(1, tailAfterBearer.slice(10, 15)))
  w.push(delta(2, tailAfterBearer.slice(15)))
  const row = w.rows[0]
  assert.deepEqual(leaks(row.text), [])
  assert.match(redactText(`x Bearer ${TOKEN}`), /Bearer \[redacted\]$/)
  // Split mid-token across deltas: the second half never follows a redacted first half.
  const v = new EventWindow(catalog, { limit: 10 })
  v.push(delta(0, `see Bearer ${TOKEN.slice(0, 9)}`, 'rS'))
  v.push(delta(1, `${TOKEN.slice(9)} done`, 'rS'))
  assert.equal(v.rows[0].text, 'see Bearer [redacted] done')
  assert.deepEqual(leaks(v.rows[0].fields.text), [], 'nor the first delta as emitted')
})

test('the bounded private raw text never starts with an orphaned credential value', () => {
  const w = new EventWindow(catalog, { limit: 10 })
  // An unbroken value longer than the private bound: the head cut cannot keep its keyword, so it
  // keeps nothing of it rather than an orphan.
  w.push(delta(0, 'Bearer '))
  for (let i = 1; i <= 40; i++) w.push(delta(i, `${TOKEN}`.repeat(25)))
  assert.deepEqual(leaks(w.rows[0].text), [], w.rows[0].text.slice(0, 60))
  // A long reply whose bound cut lands between "Authorization: Bearer" and its value.
  const u = new EventWindow(catalog, { limit: 10 })
  u.push(delta(0, 'Authorization: Bearer ', 'rU'))
  u.push(delta(1, `${TOKEN} `, 'rU'))
  for (let i = 2; i < 400; i++) u.push(delta(i, 'word '.repeat(10), 'rU'))
  assert.deepEqual(leaks(u.rows[0].text), [])
  // The bound cut leaves only whitespace after the keyword: "Bearer" is kept for the value that follows.
  const k = new EventWindow(catalog, { limit: 10 })
  k.push(delta(0, `${'y'.repeat(16380)} Bearer${' '.repeat(20)}`, 'rK'))
  k.push(delta(1, `${TOKEN} after`, 'rK'))
  assert.deepEqual(leaks(k.rows[0].text), [], k.rows[0].text.slice(-60))
  assert.match(k.rows[0].text, /Bearer \[redacted\] after$/)
  // Only whitespace after the bound cut (the keyword itself is beyond the bound): the value that
  // follows is dropped with the run, never shown orphaned.
  const j = new EventWindow(catalog, { limit: 10 })
  j.push(delta(0, `Bearer${' '.repeat(16400)}`, 'rJ'))
  j.push(delta(1, `${TOKEN} after`, 'rJ'))
  assert.deepEqual(leaks(j.rows[0].text), [], j.rows[0].text.slice(-60))
  const h = new EventWindow(catalog, { limit: 10 }) // the value arrives with a leading blank, in parts
  h.push(delta(0, `Bearer${' '.repeat(16400)}`, 'rH'))
  h.push(delta(1, ` ${TOKEN.slice(0, 8)}`, 'rH'))
  h.push(delta(2, `${TOKEN.slice(8)} after`, 'rH'))
  assert.deepEqual(leaks(h.rows[0].text), [], h.rows[0].text.slice(-60))
  assert.equal(h.rows[0].text, ' after')
  // Ordinary text is untouched, and closing the reply drops the private raw text.
  const t = new EventWindow(catalog, { limit: 10 })
  t.push(delta(0, 'hello ', 'rV'))
  t.push(delta(1, 'world', 'rV'))
  assert.equal(t.rows[0].text, 'hello world')
  t.push({ v: 2, id: 'g:done', producer: 'g', seq: 5000, t_ns: 9, source: 'guest', kind: 'agent_done', response_id: 'rV', status: 'completed' })
  assert.equal(t.raw.size, 0)
})

// An unfinished QUOTED credential value is hidden while it streams, and the private-buffer trim
// never cuts inside one.
const QTOKEN = 'abcdefgh'
function quotedRig() {
  const w = new EventWindow(catalog, { limit: 10 })
  const seen = []
  w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  let i = 0
  const push = (text) => w.push(delta(i++, text, 'rQ'))
  return { w, seen, push, shown: () => w.rows[0].text }
}

test('an open quoted value is hidden in every emission until its closing quote', () => {
  const r = quotedRig()
  r.push(`MY_TOKEN="prefix ${QTOKEN}`) // the closing quote has not arrived
  assert.equal(r.shown(), 'MY_TOKEN=[redacted]')
  r.push('"')
  assert.equal(r.shown(), 'MY_TOKEN=[redacted]')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN) || p.includes('prefix')), [], 'no added/updated payload ever carried the value')
  // the same across many small deltas, and for a JSON-style key with a spaced value
  const s = quotedRig()
  for (const part of ['{"api_key": "sk-', 'live first ', 'half ', QTOKEN, '", "ok": true}']) s.push(part)
  assert.equal(s.shown(), '{"api_key": "[redacted]", "ok": true}')
  assert.deepEqual(s.seen.filter((p) => p.includes(QTOKEN) || p.includes('first half')), [])
})

test('the private-buffer trim never cuts inside a quoted credential value', () => {
  const r = quotedRig()
  r.push('ordinary start ')
  r.push(`MY_TOKEN="prefix ${' '.repeat(17000)}${QTOKEN}"`)
  assert.ok(!r.shown().includes(QTOKEN), r.shown())
  assert.ok(![...r.w.raw.values()].some((v) => v.includes(QTOKEN) && !v.includes('MY_TOKEN=')), 'never kept without its key')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN)), [])
  // an OPEN quoted value longer than the bound keeps hiding what follows until it closes
  const o = quotedRig()
  o.push('MY_TOKEN="')
  for (let k = 0; k < 20; k++) o.push(`${'v'.repeat(1000)} `)
  o.push(`${QTOKEN} tail"`)
  o.push(' after')
  assert.ok(!o.shown().includes(QTOKEN) && !o.shown().includes('vvvv'), o.shown())
  assert.match(o.shown(), / after$/)
  assert.deepEqual(o.seen.filter((p) => p.includes(QTOKEN) || p.includes('vvvv')), [])
})

test('quoted controls: ordinary quoted text is untouched and the private state stays bounded', () => {
  const r = quotedRig()
  r.push('she said "hello there" and ')
  r.push('left "an open quote')
  assert.equal(r.shown(), 'she said "hello there" and left "an open quote')
  const b = quotedRig()
  for (let k = 0; k < 60; k++) b.push(`line ${k} with "quotes" ${'w'.repeat(500)} `)
  assert.ok(b.w.raw.get(b.w.rows[0].key).length <= 16 * 1024, 'the private raw text stays bounded')
  assert.ok(b.w.orphan.size <= 1)
})

// JSON escapes. Inside a quoted credential value `\"` and `\\` do not end it; only an unescaped
// closing quote does, across delta boundaries (a backslash ending one delta) and across the
// private-buffer trim.
test('an escaped quote inside a JSON api_key value does not release its tail', () => {
  const r = quotedRig()
  const source = JSON.stringify({ api_key: `prefix " ${QTOKEN}` })
  r.push(source)
  assert.equal(r.shown(), '{"api_key":"[redacted]"}')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN) || p.includes('prefix')), [])
})

test('a backslash ending one delta escapes the quote that starts the next', () => {
  const r = quotedRig()
  r.push('{"api_key": "prefix \\') // the value so far ends with one backslash
  assert.equal(r.shown(), '{"api_key": "[redacted]')
  r.push(`" ${QTOKEN}`) // that quote is escaped: still inside the value
  assert.equal(r.shown(), '{"api_key": "[redacted]')
  r.push('", "ok": 1}') // the real terminator
  assert.equal(r.shown(), '{"api_key": "[redacted]", "ok": 1}')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN) || p.includes('prefix')), [])
})

test('an even run of backslashes before a quote is a real terminator', () => {
  const r = quotedRig()
  r.push(JSON.stringify({ api_key: `${QTOKEN}\\\\`, note: 'visible' })) // value ends in two backslashes
  assert.equal(r.shown(), '{"api_key":"[redacted]","note":"visible"}')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN)), [])
  const s = quotedRig() // the same with the backslash pair split across deltas
  s.push(`{"api_key":"${QTOKEN}\\`)
  s.push('\\", "note": "visible"}')
  assert.equal(s.shown(), '{"api_key":"[redacted]", "note": "visible"}')
  assert.deepEqual(s.seen.filter((p) => p.includes(QTOKEN)), [])
})

test('escapes are honoured across the 16 KiB trim, closed or still open', () => {
  const r = quotedRig() // a closed value with an escaped quote, crossing the bound
  r.push('ordinary start ')
  r.push(`MY_TOKEN="prefix ${' '.repeat(17000)}\\" ${QTOKEN}" tail`)
  assert.equal(r.shown().trim(), 'tail')
  assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN)), [])
  const o = quotedRig() // an open value over the bound whose last delta ends in a backslash
  o.push(`MY_TOKEN="${' '.repeat(17000)}x\\`)
  o.push(`" ${QTOKEN}`) // escaped: must stay withheld
  assert.ok(!o.shown().includes(QTOKEN), o.shown())
  o.push('" after')
  assert.equal(o.shown(), ' after')
  assert.deepEqual(o.seen.filter((p) => p.includes(QTOKEN)), [])
})

// A credential key's quoted value is secret as a WHOLE, whatever it starts with ("Bearer ",
// "Basic ", or an already-redacted marker).
const PREFIXES = ['Bearer prefix ', 'Basic prefix ', '[redacted] ']

test('a quoted credential value is redacted whole, whatever its prefix', () => {
  for (const prefix of PREFIXES) {
    const r = quotedRig()
    r.push(JSON.stringify({ api_key: prefix + QTOKEN }))
    assert.equal(r.shown(), '{"api_key":"[redacted]"}', prefix)
    assert.deepEqual(r.seen.filter((p) => p.includes(QTOKEN) || p.includes('prefix')), [], prefix)
  }
})

test('a whole quoted value, the same while streaming in chunks and across the 16 KiB bound', () => {
  for (const prefix of PREFIXES) {
    const s = quotedRig() // streamed: prefix, value and terminator in separate deltas
    for (const part of ['{"api_key": "', prefix.slice(0, 4), prefix.slice(4), QTOKEN.slice(0, 3), `${QTOKEN.slice(3)}", "ok": 1}`]) s.push(part)
    assert.equal(s.shown(), '{"api_key": "[redacted]", "ok": 1}', prefix)
    assert.deepEqual(s.seen.filter((p) => p.includes(QTOKEN) || p.includes('prefix')), [], prefix)
    const b = quotedRig() // a closed value crossing the bound
    b.push('ordinary start ')
    b.push(`{"api_key": "${prefix}${' '.repeat(17000)}${QTOKEN}"} tail`)
    assert.equal(b.shown().trim(), 'tail', prefix) // the trim resumes at the next whole token after the value
    assert.deepEqual(b.seen.filter((p) => p.includes(QTOKEN)), [], prefix)
    const o = quotedRig() // still open past the bound, then closed
    o.push(`{"api_key": "${prefix}${' '.repeat(17000)}`)
    o.push(QTOKEN)
    o.push('" after')
    assert.equal(o.shown(), ' after', prefix)
    assert.deepEqual(o.seen.filter((p) => p.includes(QTOKEN)), [], prefix)
  }
})

test('whole-value controls: a bare Authorization scheme and ordinary prose keep their wording', () => {
  const r = quotedRig()
  r.push(`Authorization: Bearer ${QTOKEN} and then words`)
  assert.equal(r.shown(), 'Authorization: Bearer [redacted] and then words')
  const t = quotedRig()
  t.push('she said "plain words" here')
  assert.equal(t.shown(), 'she said "plain words" here')
})

// Every text field this window emits on /api/rth-events (a log's summary and msg, a typed
// message, a transcript, a rejection reason, a fault detail, unknown kinds' fields, the
// ephemeral status) carries the shared credential patterns' redaction, server-side.
test('log summaries and every other emitted text field are redacted before they leave the window', () => {
  const SECRET = 'Zq9LeakCheck77'
  const w = new EventWindow(catalog, { limit: 50 })
  const seen = []
  w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  let n = 0
  const e = (kind, f) => ({ v: 2, id: `g:lr${++n}`, producer: 'g', seq: 9000 + n, t_ns: n, source: 'guest', kind, ...f })
  w.push(e('log', { level: 'info', msg: `calling with Authorization: Bearer ${SECRET} now` }))
  w.push(e('log', { level: 'warn', msg: `env MY_TOKEN="${SECRET} x" and {"api_key": "${SECRET}"}` }))
  w.push(e('user_text', { request_id: 'q1', text: `my key is AIza${SECRET}abcdefghijkl` }))
  w.push(e('input_transcript', { text: `token: ${SECRET}` }))
  w.push(e('journal_fault', { detail: `write failed for api_key=${SECRET}` }))
  w.push(e('some_future_kind', { note: `Bearer ${SECRET}`, nested: { deeper: [`token=${SECRET}`] } }))
  w.push(e('heartbeat', { uptime_ms: 1, attachments: 1, note: `Basic ${SECRET}` }))
  const everything = [...seen, JSON.stringify(w.rows), JSON.stringify(w.status)].join('\n')
  assert.ok(!everything.includes(SECRET), everything.split('\n').filter((s) => s.includes(SECRET)).join('\n').slice(0, 400))
  const log = w.rows.find((r) => r.name === 'note_added')
  assert.equal(log.summary, 'info: calling with Authorization: Bearer [redacted] now')
  assert.equal(log.fields.msg, 'calling with Authorization: Bearer [redacted] now')
  assert.equal(log.fields.level, 'info', 'non-secret fields are kept as they were')
  assert.equal(w.rows.find((r) => r.wire === 'some_future_kind').fields.nested.deeper[0], 'token=[redacted]')
})

test('canary: a log secret appears nowhere in the /api/rth-events snapshot route or any SSE frame', async () => {
  const CANARY = 'CanaryLogValue9X2'
  const { fiber, service, hub, port, close } = await loaded()
  const pane = reader(port)
  try {
  await pane.next((e) => e.event === 'snapshot')
  let n = 0
  const e = (kind, f) => ({ v: 2, id: `c:${++n}`, producer: 'c', seq: n, t_ns: n, source: 'guest', kind, ...f })
  service.ingest(e('log', { level: 'info', msg: `retrying with api_key=${CANARY}` }))
  service.ingest(e('log', { level: 'warn', msg: `{"authorization": "Bearer ${CANARY}"}` }))
  service.ingest(e('user_text', { request_id: 'q', text: `token: ${CANARY}` }))
  service.ingest(e('agent_delta', { response_id: 'rC', text: `see MY_TOKEN="${CANARY.slice(0, 6)}` }))
  service.ingest(e('agent_delta', { response_id: 'rC', text: `${CANARY.slice(6)}" ok` }))
  service.ingest(e('heartbeat', { uptime_ms: 1, attachments: 1, note: `Basic ${CANARY}` }))
  await pane.next((ev) => ev.event === 'status')
  const route = await new Promise((resolve) => {
    http.get({ host: '127.0.0.1', port, path: '/api/rth-events' }, (res) => {
      let body = ''
      res.setEncoding('utf8')
      res.on('data', (c) => (body += c))
      res.on('end', () => resolve(body))
    })
  })
  const frames = pane.events.map((ev) => JSON.stringify(ev))
  assert.ok(pane.events.some((ev) => ev.event === 'added'), 'the stream carried the rows')
  assert.ok(!route.includes(CANARY), 'the snapshot route body')
  assert.ok(!JSON.stringify(service.snapshot()).includes(CANARY), 'the serialized snapshot')
  assert.deepEqual(frames.filter((f) => f.includes(CANARY)), [], 'every SSE frame')
  assert.ok(route.includes('api_key=[redacted]'), 'the log is still shown, redacted')
  } finally {
    // Always released, so a regression fails fast instead of holding the test file open.
    pane.req.destroy()
    close()
    await fiber.dispose()
  }
})

// No credential FRAGMENT survives the 160-char summary clip, and a secret-NAMED key is masked
// wherever it nests (key-aware, like dsh-mind / dsh-settings).
const fragmentsOf = (secret, n = 5) => Array.from({ length: secret.length - n + 1 }, (_, i) => secret.slice(i, i + n))
const fragmentIn = (secret, text) => fragmentsOf(secret).find((f) => text.includes(f)) ?? null

test('a credential cut by the summary clip never leaves a fragment, at any cut position', () => {
  const KEY = 'AIzaSyntheticReviewSecret1234567890'
  const forms = [(k) => `${'x'.repeat(k)} ${KEY}`, (k) => `${'x'.repeat(k)} api_key=${KEY}`, (k) => `${'x'.repeat(k)} Authorization: Bearer ${KEY}`]
  for (const form of forms) {
    for (let k = 100; k <= 160; k++) {
      const w = new EventWindow(catalog, { limit: 5 })
      const seen = []
      w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
      w.push({ v: 2, id: `g:r1:${k}`, producer: 'g', seq: k, t_ns: k, source: 'guest', kind: 'log', level: 'info', msg: form(k) })
      const all = [...seen, JSON.stringify(w.rows)].join('\n')
      assert.equal(fragmentIn(KEY, all), null, `cut at ${k}: ${w.rows[0].summary.slice(-30)}`)
    }
  }
})

test('secret-named keys are masked in nested objects, arrays and the status; protocol ids are kept', () => {
  const w = new EventWindow(catalog, { limit: 10 })
  const seen = []
  w.onChange((change, payload) => seen.push(JSON.stringify(payload)))
  w.push({ v: 2, id: 'g:r2:1', producer: 'g', seq: 1, t_ns: 1, source: 'guest', kind: 'some_future_kind', response_id: 'r-keep', nested: { api_key: 'SyntheticKeyValue123', list: [{ password: 'SyntheticPassValue456' }, 'plain'] } })
  w.push({ v: 2, id: 'g:r2:2', producer: 'g', seq: 2, t_ns: 2, source: 'guest', kind: 'log', level: 'info', msg: 'ok', credential: { secret: 'SyntheticCredValue789' } })
  w.push({ v: 2, id: 'g:r2:3', producer: 'g', seq: 3, t_ns: 3, source: 'guest', kind: 'heartbeat', uptime_ms: 1, attachments: 1, auth: { cookie: 'SyntheticCrumbValue000' } })
  const all = [...seen, JSON.stringify(w.rows), JSON.stringify(w.status)].join('\n')
  for (const v of ['SyntheticKeyValue123', 'SyntheticPassValue456', 'SyntheticCredValue789', 'SyntheticCrumbValue000']) assert.equal(fragmentIn(v, all), null, v)
  const row = w.rows[0]
  assert.equal(row.fields.nested.api_key, '[redacted]')
  assert.equal(row.fields.nested.list[1], 'plain', 'non-secret values are kept')
  assert.equal(row.fields.response_id, 'r-keep', 'protocol identities are kept')
  assert.equal(w.rows[1].fields.credential, '[redacted]')
  // audio is still counted from its data, and never emitted
  w.push({ v: 2, id: 'g:r2:a', producer: 'g', seq: 4, t_ns: 4, source: 'guest', kind: 'agent_audio', response_id: 'rA', chunk: 0, sample_offset: 0, sample_rate: 24000, format: 's16le', data: Buffer.alloc(4800).toString('base64') })
  const a = w.rows.find((r) => r.audio)
  assert.equal(a.audio.bytes, 4800)
  assert.equal(a.fields.data, undefined)
})

test('fragment and nested-key canary on the real /api/rth-events snapshot route and SSE: no fragment anywhere', async () => {
  const KEY = 'AIzaSyntheticReviewSecret1234567890'
  const NESTED = 'SyntheticKeyValue123'
  const { fiber, service, port, close } = await loaded()
  const pane = reader(port)
  try {
    await pane.next((e) => e.event === 'snapshot')
    let n = 0
    const e = (kind, f) => ({ v: 2, id: `rr:${++n}`, producer: 'rr', seq: n, t_ns: n, source: 'guest', kind, ...f })
    for (let k = 130; k <= 158; k += 4) service.ingest(e('log', { level: 'info', msg: `${'x'.repeat(k)} ${KEY}` }))
    service.ingest(e('some_future_kind', { nested: { api_key: NESTED } }))
    service.ingest(e('heartbeat', { uptime_ms: 1, attachments: 1, nested: { token: NESTED } }))
    await pane.next((ev) => ev.event === 'status')
    const route = await new Promise((resolve) => {
      http.get({ host: '127.0.0.1', port, path: '/api/rth-events' }, (res) => {
        let body = ''
        res.setEncoding('utf8')
        res.on('data', (c) => (body += c))
        res.on('end', () => resolve(body))
      })
    })
    const frames = pane.events.map((ev) => JSON.stringify(ev)).join('\n')
    for (const [what, text] of [['snapshot route', route], ['serialized snapshot', JSON.stringify(service.snapshot())], ['SSE frames', frames]]) {
      assert.equal(fragmentIn(KEY, text), null, `${what}: key fragment`)
      assert.equal(fragmentIn(NESTED, text), null, `${what}: nested key value`)
    }
  } finally {
    pane.req.destroy()
    close()
    await fiber.dispose()
  }
})
