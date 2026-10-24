// Delegate tasks through the Bot channel: polling, retired producer lifetimes, and the
// read-only fence at mutation ingress.
import test from 'node:test'
import assert from 'node:assert/strict'
import { LEASE_MS, Projection } from '../src/rth-tasks.js'
import { install } from '../src/host/channel.js'

const row = (task_id, rev, lifetime, execution = 'running', publication = 'none') => ({ task_id, rev, lifetime, execution, publication, updated_at: Date.now(), title: 'Report' })

test('the current producer is set by its snapshot boundary; a retired lifetime never revives', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'old', tasks: [row('a', 1, 'old')] })
  p.applySnapshot({ lifetime: 'new', tasks: [row('b', 1, 'new')] })
  assert.equal(p.applySnapshot({ lifetime: 'old', tasks: [row('a', 2, 'old')] }), false, 'a retired lifetime cannot become current again')
  const v = Object.fromEntries(p.view().map(r => [r.task_id, r]))
  assert.equal(v.a.card, 'unknown', 'a newer record from a retired producer does not revive it')
  assert.equal(v.b.card, 'working', 'the current producer stays current')
})

test('rows carry the lease the browser enforces without another message', () => {
  const p = new Projection()
  p.applySnapshot({ lifetime: 'L', tasks: [row('a', 1, 'L')] })
  const [r] = p.view()
  assert.ok(Number.isFinite(r.lease_remaining_ms) && r.lease_remaining_ms <= LEASE_MS && r.obs === 'L:1')
})

function rig(records, sends) {
  const state = { revision: 1, mainBotIds: [], bots: {}, rooms: {}, agents: {}, tasks: {}, exchangeTurns: {}, questions: {}, prefs: {} }
  let route
  const noop = () => {}
  const empty = () => ({})
  const ctx = {
    get: n => (n === 'rthDelegation' ? { snapshot: () => ({ lifetime: 'L', tasks: records() }) } : undefined),
    effect: (f, name) => (name === 'ds-bot: api route' ? f() : undefined),
    connection: { fetch: { register: r => ((route = r), noop) } },
    sessionController: { prompt: async x => sends.push(x) },
  }
  const rt = { ctx, state, home: '/nonexistent', config: {}, refresh: noop, readOnly: () => undefined,
    extended: { endpointOf: noop, view: empty, live: empty }, bootstrap: noop, catalog: noop, loadSecrets: noop,
    cachedCatalog: () => [], cachedDefault: noop, secretsView: () => [], capabilityLibraryView: () => [],
    activityView: empty, workView: empty, roomWorkView: empty, approvalsView: () => [], memoryNews: empty,
    memoryReviews: empty, connectorsView: () => [], capabilityStatusesView: empty, noteFailure: x => { throw Error(JSON.stringify(x)) },
    load: noop, pastOwners: new Map(), botOf: noop, ensureCapabilities: noop }
  install(rt)
  return async (endpoint, payload) => (await route.fetch(new Request('http://bot.invalid/api/bot', { method: 'POST', body: JSON.stringify({ endpoint, payload }) }))).json()
}

test('an unchanged poll still carries the current Tasks projection', async () => {
  let recs = [row('a', 1, 'L')]
  const request = rig(() => recs, [])
  const initial = await request('state', {})
  assert.equal(initial.value.rthTasks[0].card, 'working')
  recs = [{ ...row('a', 2, 'L', 'settled_exited', 'delivered'), report_text: 'the report', delivered_at: Date.now() }]
  const incremental = await request('state', { since: 1, models: initial.value.modelsKey })
  assert.equal(incremental.value.unchanged, true)
  assert.equal(incremental.value.rthTasks?.[0]?.card, 'done', 'Tasks update without a Bot revision')
})

test('a projected id is refused at mutation ingress before any DSH effect', async () => {
  const sends = []
  const request = rig(() => [row('a', 1, 'L')], sends)
  for (const [endpoint, payload] of [
    ['send', { sessionId: 'rth-task:a', text: 'must not dispatch' }],
    ['agent-session-archive', { agentId: 'rth-task:a', sessionId: 'rth-task:a' }],
    ['agent-session-rename', { agentId: 'x', sessionId: 'rth-task:a', title: 'x' }],
    ['agent-session-adopt', { agentId: 'x', sessionId: 'rth-task:a' }],
  ]) {
    const answer = await request(endpoint, payload)
    assert.equal(answer.ok, false, `${endpoint} refused`)
  }
  assert.equal(sends.length, 0, 'nothing reached the session controller')
})
