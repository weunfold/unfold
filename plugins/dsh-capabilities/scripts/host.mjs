// Host dsh-capabilities behind a plain HTTP server (a stand-in for DSH's webServer) on
// 127.0.0.1:7799 and print every map it pushes. Pair with RTH_BENCH_NOTIFY to see a bench run land.
import http from 'node:http'
import { Context } from '@deepseek-ai/cordis'
import * as plugin from '../index.mjs'

const routes = new Map()
const ctx = new Context()
ctx.provide('webServer', { register: (r) => (routes.set(r.path, r), () => routes.delete(r.path)) })
await ctx.plugin(plugin, { transport: 'webServer' })
const server = http.createServer((req, res) => {
  const r = routes.get(req.url)
  if (!r) return res.writeHead(404).end()
  r.handler(req, res)
})
server.listen(7799, '127.0.0.1', () => {
  console.log('listening 7799')
  http.get('http://127.0.0.1:7799/api/rth-capabilities/stream', (res) => {
    res.setEncoding('utf8')
    res.on('data', (chunk) => {
      for (const m of chunk.matchAll(/data: (.*)\n/g)) {
        const map = JSON.parse(m[1])
        console.log(new Date().toISOString(), 'map pushed: runs', map.runs, 'latest', map.latest_run, JSON.stringify(map.summary))
      }
    })
  })
})
