import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fetchStream, sseHub } from '../index.mjs'

const within = (p, ms, what) => Promise.race([p, new Promise((_, reject) => setTimeout(() => reject(new Error(`${what} did not settle in ${ms} ms`)), ms))])

test('fetchStream: an abort settles a pending read promptly and removes the hub client', async () => {
  const hub = sseHub()
  const abort = new AbortController()
  const reader = fetchStream(hub, new Request('http://h/s', { signal: abort.signal }), 'snapshot', { n: 1 }, 1).body.getReader()
  assert.match(new TextDecoder().decode((await reader.read()).value), /^id: 1\nevent: snapshot\n/)
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
  await body.cancel()
  assert.equal(hub.clients.size, 0)
  abort.abort() // a late abort after cancel is a no-op
})

test('fetchStream: hub close (unload) ends the body after its goodbye frame', async () => {
  const hub = sseHub()
  const reader = fetchStream(hub, new Request('http://h/s'), 'snapshot', {}).body.getReader()
  await reader.read()
  const pending = reader.read()
  hub.close('unloaded')
  assert.match(new TextDecoder().decode((await within(pending, 100, 'the goodbye read')).value), /event: resnapshot/)
  assert.deepEqual(await within(reader.read(), 100, 'the end'), { value: undefined, done: true })
  assert.equal(hub.clients.size, 0)
})
