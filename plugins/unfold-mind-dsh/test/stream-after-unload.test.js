// A stream request that reaches the route after Mind unloaded (its hub already closed, the route
// not yet unregistered) ends at once instead of joining a hub nothing will ever write to.
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { Context } from '@deepseek-ai/cordis'
import { loadCatalog } from './support/catalog.js'
import { EventWindow } from './support/window.js'
import * as plugin from '../index.js'

const within = (promise, ms, what) => {
  let timer
  const limit = new Promise((_, reject) => (timer = setTimeout(() => reject(new Error(`${what}: not in ${ms} ms`)), ms)))
  return Promise.race([promise, limit]).finally(() => clearTimeout(timer))
}

test('a stream opened after unload ends', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'unfold-mind-unload-'))
  const ctx = new Context()
  const routes = []
  ctx.provide('connection', { fetch: { register: (route) => (routes.push(route), async () => {}) } })
  ctx.provide('unfoldEvents', { window: new EventWindow(loadCatalog(), { limit: 10 }), catalog: loadCatalog(), snapshot: () => ({ link: { state: 'attached', detail: 'observer' } }) })
  const fiber = ctx.plugin(plugin, { dataDir: dir })
  try {
    await fiber
    const stream = routes.find((r) => r.path === '/api/unfold-mind/stream')
    await fiber.dispose()
    const abort = new AbortController()
    const res = await stream.fetch(new Request('http://h/api/unfold-mind/stream', { signal: abort.signal }))
    const reader = res.body.getReader()
    const ended = (async () => {
      for (;;) if ((await reader.read()).done) return true
    })()
    try {
      assert.equal(await within(ended, 1000, 'the stream ends'), true)
    } finally {
      abort.abort()
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
