import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { fetchStream, frame, sseHub } from '../index.js'

const within = (p, ms, what) => Promise.race([p, new Promise((_, reject) => setTimeout(() => reject(new Error(`${what} did not settle in ${ms} ms`)), ms))])

test('fetchStream: an abort settles a pending read promptly and removes the hub client', async () => {
  const hub = sseHub()
  const abort = new AbortController()
  const reader = fetchStream(hub, new Request('http://h/s', { signal: abort.signal }), 'snapshot', { n: 1 }, 1).body.getReader()
  assert.match(new TextDecoder().decode((await within(reader.read(), 100, 'the snapshot read')).value), /^id: 1\nevent: snapshot\n/)
  assert.equal(hub.clients.size, 1)
  const pending = reader.read()
  abort.abort()
  assert.deepEqual(await within(pending, 100, 'the pending read'), { value: undefined, done: true })
  assert.equal(hub.clients.size, 0)
  hub.send('added', { n: 2 }) // nothing to write to, and nothing throws
})

test('fetchStream: a request that arrives aborted gets a closed body and no hub client', async () => {
  const hub = sseHub()
  const abort = new AbortController()
  abort.abort()
  const reader = fetchStream(hub, new Request('http://h/s', { signal: abort.signal }), 'snapshot', {}).body.getReader()
  assert.deepEqual(await within(reader.read(), 50, 'the first read'), { value: undefined, done: true })
  assert.equal(hub.clients.size, 0)
})

test('fetchStream: a consumer cancel removes the hub client and the abort listener', async () => {
  const hub = sseHub()
  const abort = new AbortController()
  const body = fetchStream(hub, new Request('http://h/s', { signal: abort.signal }), 'snapshot', {}).body
  assert.equal(hub.clients.size, 1)
  await within(body.cancel(), 100, 'the cancel')
  assert.equal(hub.clients.size, 0)
  abort.abort() // a late abort after cancel is a no-op
})

test('fetchStream: hub close (unload) ends the body after its goodbye frame', async () => {
  const hub = sseHub()
  const reader = fetchStream(hub, new Request('http://h/s'), 'snapshot', {}).body.getReader()
  await within(reader.read(), 100, 'the snapshot read')
  const pending = reader.read()
  hub.close('unloaded')
  assert.match(new TextDecoder().decode((await within(pending, 100, 'the goodbye read')).value), /event: resnapshot/)
  assert.deepEqual(await within(reader.read(), 100, 'the end'), { value: undefined, done: true })
  assert.equal(hub.clients.size, 0)
})

test('fetchStream: a stream opened after hub close (a route not yet unregistered) gets the goodbye and ends', async () => {
  const hub = sseHub()
  hub.close('unloaded')
  const reader = fetchStream(hub, new Request('http://h/s'), 'snapshot', {}).body.getReader()
  assert.match(new TextDecoder().decode((await within(reader.read(), 100, 'the goodbye read')).value), /event: resnapshot\ndata: {"reason":"unloaded"}/)
  assert.deepEqual(await within(reader.read(), 100, 'the end'), { value: undefined, done: true })
  assert.equal(hub.clients.size, 0)
})

test('the package names no port and no address: a stream is only ever served on a route its host gives it', () => {
  const src = readFileSync(new URL('../index.js', import.meta.url), 'utf8')
  // The two fixed ports the first Unfold plugins used (agentd observer, host owner), spelled so
  // that a scan for them finds only shipped code.
  for (const port of [77_00, 77_81]) assert.doesNotMatch(src, new RegExp(`\\b${port}\\b`))
  assert.doesNotMatch(src, /127\.0\.0\.1|localhost|\[::1\]|0\.0\.0\.0/)
  assert.doesNotMatch(src, /\bport\b/i)
  assert.doesNotMatch(src, /node:net|node:http\b|listen\(/)
})

/** A Node-style response whose socket takes `room` writes before it reports backpressure. */
const slowRes = (room) => {
  const res = new EventEmitter()
  Object.assign(res, {
    room,
    written: [],
    ended: null,
    destroyed: false,
    writeHead() {},
    write(text) {
      res.written.push(text)
      return res.written.length < res.room
    },
    end(text) {
      res.ended = text ?? ''
    },
    destroy() {
      res.destroyed = true
    },
  })
  return res
}

test('sseHub: backpressure queues events until drain, then writes them in order', () => {
  const hub = sseHub()
  const res = slowRes(1)
  hub.open({}, res, 'snapshot', { n: 0 }, 0)
  hub.send('added', { n: 1 }, 1)
  hub.send('added', { n: 2 }, 2)
  assert.equal(res.written.length, 1, 'only the baseline went out before drain')
  res.written.length = 0
  res.room = Infinity // the socket drained and has room again
  res.emit('drain')
  assert.deepEqual(res.written.map((t) => t.match(/^id: (\d+)/)[1]), ['1', '2'])
  assert.equal(hub.clients.size, 1)
  hub.close()
})

test('sseHub: a client over its pending bound is cut, counted as stalled, and the others go on', () => {
  const hub = sseHub({ maxPendingEvents: 2 })
  const slow = slowRes(1)
  const fast = slowRes(Infinity)
  hub.open({}, slow, 'snapshot', {}, 0)
  hub.open({}, fast, 'snapshot', {}, 0)
  for (let n = 1; n <= 3; n++) hub.send('added', { n }, n)
  assert.equal(slow.destroyed, true, 'a client still waiting for drain is cut, not sent a goodbye')
  assert.deepEqual(hub.drops, { stalled: 1 })
  assert.equal(hub.clients.size, 1)
  assert.equal(fast.written.length, 4)
  hub.close('unloaded')
  assert.match(fast.ended, /^event: resnapshot\ndata: {"reason":"unloaded"}\n\n$/)
  assert.deepEqual(hub.drops, { stalled: 1, unloaded: 1 })
})

test('sseHub: a stalled client is cut once its queued bytes pass maxPendingBytes, far below the event bound', () => {
  const hub = sseHub({ maxPendingBytes: 4096 })
  const slow = slowRes(1) // the baseline write already reports backpressure
  hub.open({}, slow, 'snapshot', {}, 0)
  const kib = 'x'.repeat(1000)
  assert.ok(frame('added', { kib }, 4).length * 4 > 4096 && frame('added', { kib }, 3).length * 3 < 4096)
  for (let n = 1; n <= 3; n++) hub.send('added', { kib }, n)
  assert.equal(hub.clients.size, 1, 'three queued frames are under the bound')
  assert.equal(slow.destroyed, false)
  hub.send('added', { kib }, 4)
  assert.equal(slow.destroyed, true, 'the fourth frame takes the queue past 4096 bytes, with 4 events queued')
  assert.deepEqual(hub.drops, { stalled: 1 })
  assert.equal(hub.clients.size, 0)
})

test('sseHub: with the default bounds, 1 KiB events stall a client out on bytes before the 10,000-event bound', () => {
  const hub = sseHub()
  const slow = slowRes(1)
  hub.open({}, slow, 'snapshot', {}, 0)
  const kib = 'x'.repeat(1000)
  let n = 0
  while (hub.clients.size && n < 2_000) hub.send('added', { kib }, ++n)
  assert.equal(hub.clients.size, 0)
  assert.ok(n > 900 && n < 1_100, `dropped after ${n} events, near 1 MiB`)
  assert.deepEqual(hub.drops, { stalled: 1 })
})

test('sseHub: a client that never drains is cut as stalled at drainTimeoutMs; one that drains in time stays', async () => {
  const hub = sseHub({ drainTimeoutMs: 40 })
  const stuck = slowRes(1)
  const drains = slowRes(1)
  hub.open({}, stuck, 'snapshot', {}, 0)
  hub.open({}, drains, 'snapshot', {}, 0)
  await new Promise((r) => setTimeout(r, 10))
  assert.equal(stuck.destroyed, false, 'not cut before the deadline')
  drains.room = Infinity
  drains.emit('drain')
  await new Promise((r) => setTimeout(r, 90))
  assert.equal(stuck.destroyed, true, 'no drain within the deadline cuts the client')
  assert.equal(drains.destroyed, false)
  assert.deepEqual(hub.drops, { stalled: 1 })
  assert.equal(hub.clients.size, 1)
  hub.close()
})

test('frame: an id line only when an id is given', () => {
  assert.equal(frame('e', { a: 1 }), 'event: e\ndata: {"a":1}\n\n')
  assert.equal(frame('e', null, 0), 'id: 0\nevent: e\ndata: null\n\n')
})
