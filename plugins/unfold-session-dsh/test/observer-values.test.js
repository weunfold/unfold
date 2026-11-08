import test from 'node:test'
import assert from 'node:assert/strict'
import { AgentdObserver } from '../src/observer.js'

test('observer rejects JSON values that are not envelopes without disrupting the handshake', () => {
  const seen = [], writes = [], destroyed = []
  const observer = new AgentdObserver({ onEnvelope: (env) => seen.push(env) })
  const socket = { write: (data) => writes.push(data), destroy: (error) => destroyed.push(error) }
  const invalid = ['null', 'false', '0', '"text"', '[]', '{}', 'not JSON']
  for (const phase of ['hello', 'ready', 'stream']) {
    for (const line of invalid) assert.equal(observer.frame(socket, line, phase), phase)
  }
  assert.equal(observer.rejected, invalid.length * 3)
  assert.deepEqual(seen, [])
  assert.deepEqual(writes, [])
  assert.deepEqual(destroyed, [])
  const hello = { v: 2, source: 'guest', kind: 'hello', id: 'guest:1', producer: 'guest', seq: 1 }
  assert.equal(observer.frame(socket, JSON.stringify(hello), 'hello'), 'ready')
  assert.equal(writes.length, 1)
  assert.equal(JSON.parse(writes[0]).role, 'observer')
  assert.equal(seen[0].id, hello.id)
  assert.equal(observer.frame(socket, JSON.stringify({v:2, source:'guest', kind:'ready', attachment_id:observer.attachmentId, role:'observer'}), 'ready'), 'stream')
  assert.equal(observer.state, 'attached')
})
