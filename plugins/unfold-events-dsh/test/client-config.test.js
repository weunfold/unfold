import test from 'node:test'
import assert from 'node:assert/strict'
import vm from 'node:vm'
import { readFileSync } from 'node:fs'
import { fetchRoutes, routes } from '../index.js'

test('configured events routes reach the browser through stable discovery', async () => {
  const args = [{}, '/api/custom-events', {}, '/api/custom-presence']
  const discovery = fetchRoutes(...args).find((r) => r.path === '/api/unfold-events/config')
  assert.ok(discovery, 'stable configuration route is registered')
  const config = await (await discovery.fetch()).json()
  assert.deepEqual(config, {route:'/api/custom-events',presenceRoute:'/api/custom-presence'})
  const bare = routes(...args).find((r) => r.path === discovery.path)
  let status, body
  await bare.handler({method:'GET'}, {writeHead:(s)=>{status=s},end:(b)=>{body=b}})
  assert.equal(status, 200)
  assert.deepEqual(JSON.parse(body), config)
  const source = readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  const start = source.indexOf('export async function apply(ctx) {')
  const end = source.indexOf('  ctx.effect(', start)
  assert.ok(start >= 0, 'UI awaits configuration before registering')
  const scope = {fetch:async(path)=>{if(path==='api/unfold-session/config') return Response.json({route:'/api/custom-session'}); assert.equal(path,'api/unfold-events/config');return Response.json(config)}}
  vm.createContext(scope)
  const actual = await vm.runInContext(`let STREAM, PRESENCE_STREAM, SESSION_ROUTE; ${source.slice(start,end).replace('export ','')} return {stream:STREAM,presence:PRESENCE_STREAM,session:SESSION_ROUTE} }; apply({})`,scope)
  assert.deepEqual(JSON.parse(JSON.stringify(actual)), {stream:'api/custom-events/stream',presence:'api/custom-presence/stream',session:'api/custom-session'})
})
