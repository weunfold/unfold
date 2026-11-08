import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { fetchRoutes, routes } from '../index.js'

test('configured settings routes reach the browser through stable discovery', async () => {
  const args = [{}, '/api/custom-settings', {}]
  const discovery = fetchRoutes(...args).find((r) => r.path === '/api/unfold-settings/config')
  assert.ok(discovery, 'stable configuration route is registered')
  const config = await (await discovery.fetch()).json()
  assert.deepEqual(config, {route:'/api/custom-settings'})
  const bare = routes(...args).find((r) => r.path === discovery.path)
  let status, body
  await bare.handler({method:'GET'}, {writeHead:(s)=>{status=s},end:(b)=>{body=b}})
  assert.equal(status, 200)
  assert.deepEqual(JSON.parse(body), config)
  const source = readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  const start = source.indexOf('export async function apply(ctx) {')
  const end = source.indexOf('  ctx.effect(', start)
  assert.ok(start >= 0, 'UI awaits configuration before registering')
  const scope = {fetch:async(path)=>{assert.equal(path,'api/unfold-settings/config');return Response.json(config)}}
  vm.createContext(scope)
  const actual = await vm.runInContext(`let ROUTE; ${source.slice(start,end).replace('export ','')} return ROUTE }; apply({})`,scope)
  assert.equal(actual, 'api/custom-settings')
})
