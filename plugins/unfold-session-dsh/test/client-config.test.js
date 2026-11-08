import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { fetchRoutes } from '../index.js'

test('client discovers custom root and API paths before registering its UI', async () => {
  const routes = fetchRoutes({}, '/api/custom-unfold', 'custom-root')
  const discovery = routes.find((r) => r.path === '/api/unfold-session/config')
  assert.ok(discovery, 'configuration remains discoverable at a stable path')
  const config = await (await discovery.fetch()).json()
  const source = readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  const start = source.indexOf('export async function apply(ctx) {')
  const end = source.indexOf('  ctx.effect(', start)
  assert.ok(start >= 0, 'UI awaits its configuration')
  const scope = { fetch: async (path) => { assert.equal(path, 'api/unfold-session/config'); return Response.json(config) } }
  vm.createContext(scope)
  await vm.runInContext(`let ROOT_SESSION, STATUS, LIVE; ${source.slice(start, end).replace('export ', '')}
    return { session: ROOT_SESSION, status: STATUS, live: LIVE }
  }; apply({})`, scope).then((actual) => {
    assert.deepEqual(JSON.parse(JSON.stringify(actual)), { session: 'custom-root', status: 'api/custom-unfold/status', live: 'api/custom-unfold/live' })
  })
})
