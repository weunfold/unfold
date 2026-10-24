// Through the real producer
// chain (EventWindow -> MindStream -> moments -> nowLine), "Quiet" only when nothing is open or
// uncertain. A respond decision alone stays unresolved; a reply whose outcome was lost when the
// link dropped stays "not sure" after the link and heartbeats return; a real outcome settles both.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { loadCatalog } from '../../dsh-events/src/catalog.mjs'
import { EventWindow } from '../../dsh-events/src/window.mjs'
import { MindStream } from '../src/mind.mjs'
import { captionOf, moments, nowLine } from '../src/moments.mjs'

const catalog = loadCatalog()
const live = { stream: { state: 'live', label: 'Mind stream: live (pushed)' }, agent: { state: 'live', label: '' } }

function rig() {
  let clock = 1_000_000
  let seq = 0
  const window = new EventWindow(catalog, { limit: 2000 })
  const mind = new MindStream({ limit: 500, now: () => clock })
  window.onChange((t, p) => {
    if (t === 'added') mind.agentd(p, { via: 'rth-events observer feed' })
    if (t === 'updated') mind.agentdPatch(p)
  })
  const push = (kind, f = {}) => {
    const e = { v: 2, id: `g:${++seq}`, producer: 'g', seq, t_ns: seq * 1e6, source: 'guest', kind, ...f }
    window.push(e)
    return e
  }
  const view = (opts = { live: true }) => {
    const snap = mind.snapshot()
    const ms = moments(snap.rows, Object.fromEntries(snap.episodes.map((e) => [e.id, e])), opts)
    return { ms, head: nowLine(ms, live, clock) }
  }
  return { mind, push, view, tick: (ms) => { clock += ms }, now: () => clock }
}

test('a respond decision alone is not "Quiet"; its reply settles it', () => {
  const r = rig()
  r.mind.setLink('attached', 'observer')
  const d = r.push('decision', { decision_id: 'd1', candidates: ['respond', 'wait'], selected: 'respond', policy: 'rule:always-respond@v0', trigger_event: 'provider:mic-1' })
  let v = r.view()
  assert.notEqual(v.head.state, 'idle', 'nothing has answered the decision yet')
  assert.equal(v.head.text, 'Chose to reply, no reply yet')
  r.tick(120_000)
  v = r.view()
  assert.notEqual(v.head.text, 'Quiet', 'time alone does not settle it')
  assert.equal(v.head.state, 'unknown')
  r.push('response_started', { cause: d.id, response_id: 'r1', request_id: 'req1' })
  r.push('agent_delta', { cause: d.id, response_id: 'r1', text: 'Oi!' })
  r.push('agent_done', { cause: d.id, response_id: 'r1', status: 'completed' })
  v = r.view()
  assert.equal(v.head.text, 'Quiet', 'a real outcome settles it')
  assert.ok(!v.ms.some((m) => m.key.startsWith('d:') && m.decided === 'respond'), 'once the reply exists the respond decision is not a separate line')
})

test('a reply open when the link drops stays "not sure" after the link and heartbeats return, until its outcome arrives', () => {
  const r = rig()
  r.mind.setLink('attached', 'observer')
  const d = r.push('decision', { decision_id: 'd1', candidates: ['respond', 'wait'], selected: 'respond', policy: 'p', trigger_event: 't' })
  r.push('response_started', { cause: d.id, response_id: 'r', request_id: 'req' })
  let v = r.view()
  assert.equal(v.head.text, 'Replying…')
  r.mind.setLink('waiting', 1000)
  r.tick(1000)
  r.mind.setLink('attached', 'observer')
  r.push('heartbeat', { uptime_ms: 5000, attachments: 2 })
  v = r.view()
  const reply = v.ms.find((m) => m.type === 'reply')
  assert.equal(reply.status, 'unknown', 'MindStream marked it unknown when the link dropped')
  assert.equal(reply.open, false, 'not animated as replying')
  assert.equal(reply.uncertain, true)
  assert.match(captionOf(reply, r.now()), /not sure how it ended/)
  assert.notEqual(v.head.text, 'Quiet', 'a fresh link does not resolve it')
  assert.equal(v.head.state, 'unknown')
  assert.match(v.head.text, /^Not sure how the last reply ended/)
  assert.ok(v.ms.some((m) => /Lost the connection to the agent/.test(m.text ?? '')), 'the drop is a visible line')
  r.tick(600_000)
  assert.notEqual(r.view().head.text, 'Quiet', 'nor does time')
  // The outcome arrives (a later terminal for the same reply): it resolves.
  r.push('agent_done', { cause: d.id, response_id: 'r', status: 'completed' })
  v = r.view()
  const settled = v.ms.find((m) => m.type === 'reply')
  assert.equal(settled.uncertain, false, `episode ${settled.status}`)
  assert.equal(v.head.text, 'Quiet')
})

test('one episode holding two replies (a shared decision id): one ended, one did not -> still not sure', () => {
  const r = rig()
  r.mind.setLink('attached', 'observer')
  const d = r.push('decision', { decision_id: 'shared', candidates: ['respond', 'wait'], selected: 'respond', policy: 'p', trigger_event: 't' })
  r.push('response_started', { cause: d.id, response_id: 'r7', request_id: null })
  r.push('agent_delta', { cause: d.id, response_id: 'r7', text: 'Deixa eu pensar' })
  const d2 = r.push('decision', { decision_id: 'shared', candidates: ['respond', 'wait'], selected: 'respond', policy: 'p', trigger_event: 't2' })
  r.push('response_started', { cause: d2.id, response_id: 'r8', request_id: null })
  r.push('agent_done', { cause: d2.id, response_id: 'r8', status: 'completed' })
  r.tick(120_000)
  const v = r.view()
  assert.notEqual(v.head.text, 'Quiet', 'r7 never ended')
  assert.equal(v.head.state, 'unknown')
  r.push('agent_done', { cause: d.id, response_id: 'r7', status: 'completed' })
  assert.equal(r.view().head.text, 'Quiet', 'both ended: settled')
})

test('replies settle by their own ids; an end with no observed start settles nothing else', () => {
  const r = rig()
  r.mind.setLink('attached', 'observer')
  const g1 = r.push('decision', { decision_id: 'dd', candidates: ['respond', 'wait'], selected: 'respond', policy: 'p', trigger_event: 't' })
  r.push('response_started', { cause: g1.id, response_id: 'a', request_id: 'req' })
  r.push('response_started', { cause: g1.id, response_id: 'b', request_id: 'req' })
  r.push('agent_done', { cause: g1.id, response_id: 'b', status: 'completed' })
  r.push('agent_done', { cause: g1.id, response_id: 'c', status: 'completed' }) // c's start was never observed
  r.tick(120_000)
  let v = r.view()
  const reply = v.ms.find((m) => m.type === 'reply')
  assert.equal(reply.uncertain, true, 'a never ended')
  assert.notEqual(v.head.text, 'Quiet')
  assert.equal(v.head.state, 'unknown')
  r.push('agent_done', { cause: g1.id, response_id: 'a', status: 'completed' })
  v = r.view()
  assert.equal(v.ms.find((m) => m.type === 'reply').uncertain, false)
  assert.equal(v.head.text, 'Quiet', 'every started reply has its own end: settled')
})
