import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, writeFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { AcceptLedger, initSpend, SPEND_SCHEMA } from '../src/delegate/accept-ledger.mjs'
const layer = { dir:'worker',cap_pusd:700_000_000_000,max_attempts:1,attempt_bound_pusd:618_000_000_000,model:'gpt-5.4-mini',context_window:400000,options:{maxSteps:1,attemptsPerStep:1,maxOutputTokens:2000,reasoningEffort:'off',routeMaxRetries:0},prices:{input:1500000,cached:150000,output:9000000} }
const meta = id => ({attemptId:id,fields:{}})
const zero = {inputTokens:0,outputTokens:0}
function fixture(overrides={}) {
  const dir=join(mkdtempSync(join(tmpdir(),'spend-')), 'ledger')
  const l={...layer,...overrides}
  const sha=initSpend(dir,{schema:SPEND_SCHEMA,accept_id:'fictional-accept',stop:'stop.json',layers:{worker:l}})
  const open=(hooks={})=>AcceptLedger.open({control:join(dir,'control.json'),controlSha256:sha,layer:'worker',expect:{model:l.model,options:l.options},hooks}).ledger
  return {dir,open}
}
test('a stop visible before the claim prevents admission',()=>{
  const {dir,open}=fixture()
  const a=open({beforeClaim:()=>writeFileSync(join(dir,'stop.json'),JSON.stringify({layer:'realtime',reason:'failure'}))})
  const result=a.admit(meta('a'))
  assert.equal(existsSync(join(dir,'stop.json')),true)
  assert.equal(result.ok,false,JSON.stringify(result))
})
test('a foreign attempt cannot settle a local reservation',()=>{
  const {open}=fixture(); const a=open(); const first=a.admit(meta('owned'))
  const result=a.settle({...first.attempt,attempt_id:'another-acceptance/foreign'}, {usage:zero})
  assert.equal(result.ok,false,JSON.stringify({result,read:a.read()}))
})
test('a replayed foreign settlement does not free budget',()=>{
  const {dir,open}=fixture({max_attempts:2}); const a=open(); a.admit(meta('owned'))
  assert.equal(a.admit(meta('second')).ok,false,'positive: two reservations do not fit')
  writeFileSync(join(dir,'worker/attempt-000.settle.json'),JSON.stringify({n:0,accept_id:'foreign',layer:'realtime',attempt_id:'foreign',provider_response_id:'other',usage:zero,cost_pusd:0}))
  const result=open().admit(meta('second'))
  assert.equal(result.ok,false,JSON.stringify(result))
})
test('negative usage is malformed, not a refund',()=>{
  const {open}=fixture(); const a=open(); const first=a.admit(meta('owned'))
  const result=a.settle(first.attempt,{usage:{inputTokens:-100,outputTokens:0}})
  assert.equal(result.ok,false,JSON.stringify(result))
})
test('a zero-cost replay keeps the one-attempt allowance',()=>{
  const {open}=fixture(); const a=open(); const first=a.admit(meta('owned'))
  assert.equal(first.ok,true); assert.equal(a.settle(first.attempt,{usage:zero}).ok,true)
  assert.equal(a.settle(first.attempt,{usage:zero}).ok,true)
  assert.equal(open().admit(meta('second')).ok,false)
})

// ── Reservation, metadata and replay controls ──
import { readdirSync, readFileSync as readF, rmSync } from 'node:fs'

test('a stop seen after the claim keeps the reservation and refuses dispatch', () => {
  const { dir, open } = fixture()
  const a = open({ beforeClaim: () => writeFileSync(join(dir, 'stop.json'), '{}') })
  const r = a.admit(meta('a'))
  assert.deepEqual([r.ok, r.reserved], [false, true])
  assert.ok(readdirSync(join(dir, 'worker')).includes('attempt-000.json'), 'the reservation stays')
})

test('metadata cannot overwrite the authoritative fields', () => {
  const { dir, open } = fixture()
  const r = open().admit({ attemptId: 'owned', fields: { bound_pusd: 0, n: 9, accept_id: 'x', attempt_id: 'y' } })
  assert.equal(r.ok, true)
  const row = JSON.parse(readF(join(dir, 'worker', 'attempt-000.json'), 'utf8'))
  assert.deepEqual([row.n, row.attempt_id, row.bound_pusd, row.accept_id], [0, 'owned', 618_000_000_000, 'fictional-accept'])
})

test('a fatal record on replay, and ledger I/O failure, stop the layer', () => {
  const { dir, open } = fixture()
  writeFileSync(join(dir, 'worker', 'anomaly-x.json'), '{"bound_pusd":618000000000}')
  assert.equal(open().admit(meta('a')).failed, true)
  assert.equal(existsSync(join(dir, 'stop.json')), true)
  const g = fixture()
  const l = g.open()
  rmSync(join(g.dir, 'worker'), { recursive: true })
  const r = l.admit(meta('a'))
  assert.equal(r.failed, true)
  assert.match(r.reason, /ledger unreadable/)
})

test('a replayed settlement whose cost is not its usage at the pinned prices fails', () => {
  const { dir, open } = fixture({ max_attempts: 2 })
  const a = open()
  const first = a.admit(meta('owned'))
  assert.equal(a.settle(first.attempt, { usage: { inputTokens: 10, outputTokens: 1 } }).ok, true)
  const p = join(dir, 'worker', 'attempt-000.settle.json')
  const row = JSON.parse(readF(p, 'utf8'))
  rmSync(p)
  writeFileSync(p, JSON.stringify({ ...row, cost_pusd: 0 }))
  assert.equal(open().admit(meta('second')).failed, true)
})
