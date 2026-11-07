// src/sse.js is this plugin's copy of the hub in apps/unfold-sse-dsh, and src/call-view.js its copy
// of the call view in apps/unfold-events-dsh (each plugin installs on its own, so it carries them
// rather than depending on them). The copies stay byte for byte the same.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

const sha256 = (url) => createHash('sha256').update(readFileSync(url)).digest('hex')
// The hub's index.js as it landed in apps/unfold-sse-dsh, and the call view in apps/unfold-events-dsh.
const HUB_SHA256 = 'a969339dcce3085ea65242dda857c272a0cc3261dea7fadafd8331d07fce2ae2'
const CALL_VIEW_SHA256 = '9bce669cef0bcea3cc0dca3fb18b241942f07076c503ce407425fba501bd541a'

test('src/sse.js is the unfold-sse-dsh hub, unchanged', () => {
  const copy = sha256(new URL('../src/sse.js', import.meta.url))
  assert.equal(copy, HUB_SHA256, 'copy apps/unfold-sse-dsh/index.js to src/sse.js')
  const hub = new URL('../../unfold-sse-dsh/index.js', import.meta.url)
  if (existsSync(hub)) assert.equal(copy, sha256(hub), 'copy apps/unfold-sse-dsh/index.js to src/sse.js')
})

test('src/call-view.js is the unfold-events-dsh call view, unchanged', () => {
  const copy = sha256(new URL('../src/call-view.js', import.meta.url))
  assert.equal(copy, CALL_VIEW_SHA256, 'copy apps/unfold-events-dsh/src/call-view.js to src/call-view.js')
  const theirs = new URL('../../unfold-events-dsh/src/call-view.js', import.meta.url)
  if (existsSync(theirs)) assert.equal(copy, sha256(theirs), 'copy apps/unfold-events-dsh/src/call-view.js to src/call-view.js')
})
