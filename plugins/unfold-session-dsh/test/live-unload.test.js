// The live view's stream on unload: one open before it gets the goodbye frame and ends, and one
// that reaches the route after it (the route not unregistered yet) ends at once.
import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { apply } from '../index.js'
import { fakeDsh } from './fake-dsh.js'
import { within } from './wait.js'

function rig(t) {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-live-unload-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  const dsh = fakeDsh()
  apply(dsh.ctx, { workspaceDir: join(dir, 'workspace') }, { env: { AUTOMAGIK_HOST: 'unfold', AUTOMAGIK_HOME: dir }, observer: { start() {}, stop() {} } })
  const live = dsh.routes.find((route) => route.path.endsWith('/live'))
  assert.ok(live, 'the live route is registered')
  const open = async () => {
    const abort = new AbortController()
    t.after(() => abort.abort())
    const reader = (await live.fetch(new Request('http://h/api/unfold-session/live', { signal: abort.signal }))).body.getReader()
    const decoder = new TextDecoder()
    let frames = ''
    const ended = (async () => {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) return frames
        frames += decoder.decode(value)
      }
    })()
    return { ended, frames: () => frames }
  }
  return { dsh, open }
}

test('a live stream open at unload gets the goodbye frame and ends', async (t) => {
  const r = rig(t)
  const stream = await r.open()
  await within(new Promise((resolve) => setImmediate(resolve)), 1000, 'the snapshot frame')
  r.dsh.dispose()
  const frames = await within(stream.ended, 1000, 'the open live stream ends on unload')
  assert.match(frames, /event: resnapshot\ndata: {"reason":"unloaded"}/)
})

test('a live stream opened after unload ends at once', async (t) => {
  const r = rig(t)
  r.dsh.dispose()
  const stream = await r.open()
  const frames = await within(stream.ended, 1000, 'the live stream opened after unload ends')
  assert.match(frames, /event: resnapshot/)
})
