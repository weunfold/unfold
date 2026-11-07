// The SSE hub is @automagik/unfold-sse-dsh's index.js, carried byte for byte: each plugin installs on
// its own, so it keeps its own copy. Change the hub there, copy it here, and move the pin.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

const PINNED = 'a969339dcce3085ea65242dda857c272a0cc3261dea7fadafd8331d07fce2ae2'
const sha = (url) => createHash('sha256').update(readFileSync(url)).digest('hex')

test('src/sse.js is the unfold-sse-dsh hub, byte for byte', () => {
  assert.equal(sha(new URL('../src/sse.js', import.meta.url)), PINNED)
  const sibling = new URL('../../unfold-sse-dsh/index.js', import.meta.url)
  if (existsSync(sibling)) assert.equal(sha(sibling), PINNED, 'the hub moved: copy apps/unfold-sse-dsh/index.js to src/sse.js and move the pin')
})
