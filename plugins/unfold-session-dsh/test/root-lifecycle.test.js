import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { apply } from '../index.js'
import { fakeDsh } from './fake-dsh.js'
import { within } from './wait.js'

const deferred = () => {
  let resolve
  const promise = new Promise((done) => { resolve = done })
  return { promise, resolve }
}
const own = { env: { AUTOMAGIK_HOST: 'unfold' }, observer: { start() {}, stop() {} } }

test('root readiness rejects on unload when creation dependencies never bind', async () => {
  const dsh = fakeDsh()
  const inject = dsh.ctx.inject
  dsh.ctx.inject = (names, callback) => { if (!names.includes('workspaceRegistry')) inject(names, callback) }
  apply(dsh.ctx, {}, own)
  const ready = dsh.provided.unfoldSession.ready
  dsh.dispose()
  await assert.rejects(within(ready, 1000, 'unloaded root readiness'), /unloaded/)
})

for (const stage of ['stat', 'workspace', 'session']) {
  test(`unload while ${stage} waits rejects readiness and stops later preparation steps`, async () => {
    const dsh = fakeDsh()
    const inject = dsh.ctx.inject
    const entered = deferred()
    const gate = deferred()
    const calls = []
    const waitAt = async (name, result) => {
      calls.push(name)
      if (stage === name) { entered.resolve(); await gate.promise }
      return result
    }
    dsh.ctx.inject = (names, callback) => inject(names, (ctx) => callback({
      ...ctx,
      get: () => ({ stat: () => waitAt('stat', false) }),
      workspaceRegistry: { create: () => waitAt('workspace', { id: 'workspace' }) },
      sessionController: { ...ctx.sessionController, create: () => waitAt('session', {}) },
    }))
    const dir = mkdtempSync(join(tmpdir(), 'unfold-root-cancel-'))
    try {
      apply(dsh.ctx, { workspaceDir: dir }, own)
      const ready = dsh.provided.unfoldSession.ready
      await within(entered.promise, 1000, `${stage} preparation entered`)
      dsh.dispose()
      await assert.rejects(within(ready, 1000, 'unloaded root readiness'), /unloaded/)
      const before = [...calls]
      gate.resolve()
      await new Promise((resolve) => setImmediate(resolve))
      await assert.rejects(ready, /unloaded/)
      assert.deepEqual(calls, before, 'unload prevents the next initialization stage')
    } finally {
      gate.resolve()
      dsh.dispose()
      rmSync(dir, { recursive: true, force: true })
    }
  })
}

test('root preparation failure rejects readiness for waiting Bot binding', async () => {
  const dsh = fakeDsh()
  const inject = dsh.ctx.inject
  dsh.ctx.inject = (names, callback) => inject(names, (ctx) => callback({
    ...ctx, get: () => ({ stat: async () => { throw new Error('synthetic persistence failure') } }),
  }))
  apply(dsh.ctx, {}, own)
  await assert.rejects(within(dsh.provided.unfoldSession.ready, 1000, 'failed root readiness'), /synthetic persistence failure/)
  dsh.dispose()
})
