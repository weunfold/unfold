// src/sse.js is this plugin's copy of the hub in apps/unfold-sse-dsh (each plugin installs on its
// own, so it carries the hub rather than depending on it). The copy stays byte for byte the same.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const sha256 = (url) => createHash('sha256').update(readFileSync(url)).digest('hex')

test('src/sse.js is the unfold-sse-dsh hub, unchanged', () => {
  assert.equal(sha256(new URL('../src/sse.js', import.meta.url)), sha256(new URL('../../unfold-sse-dsh/index.js', import.meta.url)), 'copy apps/unfold-sse-dsh/index.js to src/sse.js')
})
