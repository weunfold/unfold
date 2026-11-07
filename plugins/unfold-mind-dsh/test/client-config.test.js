import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { fetchRoutes } from '../index.js'

test('Mind discovers a configured API base before registering its UI', async () => {
  const routes = fetchRoutes({}, '/api/custom-mind', {})
  const discovery = routes.find((r) => r.path === '/api/unfold-mind/config')
  assert.ok(discovery, 'configuration remains discoverable at a stable path')
  const config = await (await discovery.fetch()).json()
  const source = readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  const start = source.indexOf('export async function apply(ctx) {')
  const end = source.indexOf('  ctx.effect(', start)
  assert.ok(start >= 0, 'UI awaits its configuration')
  const scope = { fetch: async (path) => { assert.equal(path, 'api/unfold-mind/config'); return Response.json(config) } }
  vm.createContext(scope)
  const base = await vm.runInContext(`let BASE; ${source.slice(start, end).replace('export ', '')} return BASE }; apply({})`, scope)
  assert.equal(base, 'api/custom-mind')
  for (const path of ['/stream', '/event', '/suggestions']) {
    assert.ok(routes.some((r) => r.path === `/${base}${path}`), `${path} is served at the client base`)
  }
})
