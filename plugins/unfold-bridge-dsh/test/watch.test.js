import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Refusal, createWatch, parseSubscriptions } from '../src/host/watch.js'
import { ALLOWED, REFUSED } from './hidden-vectors.js'

// Two Sessions: X (watched) and Y (not), each with its own title and workspace.
const X = { id: 'sess-x' }
const Y = { id: 'sess-y' }
const subX = [{ id: 's1', match: { session_ids: ['sess-x'] }, kinds: ['turn_done', 'approval_waiting', 'status', 'error', 'created'] }]
const TOKEN = 'test-token-' + '0'.repeat(10)

function setup() {
  let n = 0
  const watch = createWatch({
    names: { title: (s, id) => ({ 'sess-x': 'Builder', 'sess-y': 'Other' })[s?.id ?? id], workspace: id => (id === 'sess-x' ? 'Site' : 'Elsewhere') },
    now: () => 1000,
    newId: () => `conn-${++n}`,
  })
  const events = []
  const controls = []
  watch.on('event', e => events.push(e))
  watch.on('control', c => controls.push(c))
  return { watch, events, controls }
}

const turn = (watch, session, text) => {
  watch.sessionEvent(session, { type: 'assistant/message', data: { message: { role: 'assistant', content: [{ type: 'text', text }, { type: 'tool-call', id: 'c', name: 'bash', arguments: '{"command":"cat secrets"}' }] } } })
  watch.sessionEvent(session, { type: 'tool/result', data: { message: { role: 'tool', content: [{ type: 'text', text: 'TOOL OUTPUT' }] } } })
  watch.sessionEvent(session, { type: 'turn/end', data: { turn: 1, reason: { kind: 'completed' } } })
}

const refusal = (fn, status, code) => assert.throws(fn, error => error instanceof Refusal && error.status === status && error.code === code)

test('the filter drops events and turn text of a Session no subscription matches', () => {
  const { watch, events } = setup()
  watch.connect(subX)
  turn(watch, X, 'X finished the page')
  turn(watch, Y, 'Y private work')
  assert.equal(events.length, 1)
  assert.deepEqual(events[0], { kind: 'turn_done', session_id: 'sess-x', agent_name: 'Builder', workspace: 'Site', summary: 'X finished the page', at: 1000 })
  const { agents } = watch.agents(subX)
  assert.equal(agents.find(a => a.session_id === 'sess-x').last_summary, 'X finished the page')
  const y = agents.find(a => a.session_id === 'sess-y')
  assert.equal(y.name, 'Other')
  assert.equal('last_summary' in y, false)
  assert.equal(JSON.stringify(watch.agents(subX)).includes('Y private work'), false)
  assert.equal(JSON.stringify(events).includes('TOOL OUTPUT'), false, 'no tool output')
  assert.equal(JSON.stringify(events).includes('cat secrets'), false, 'no tool arguments')
})

test('an empty match is refused; an empty subscription list is valid and matches nothing', () => {
  refusal(() => parseSubscriptions([{ id: 'a', match: {}, kinds: ['turn_done'] }]), 400, 'bad_subscription')
  refusal(() => parseSubscriptions([{ id: 'a', match: { names: [] }, kinds: ['turn_done'] }]), 400, 'bad_subscription')
  refusal(() => parseSubscriptions([{ id: 'a', match: { names: ['x'] }, kinds: ['everything'] }]), 400, 'bad_subscription')
  refusal(() => parseSubscriptions({}), 400, 'bad_subscription')
  const { watch, events } = setup()
  watch.connect([])
  turn(watch, X, 'X finished')
  assert.deepEqual(events, [])
  assert.equal('last_summary' in watch.agents([]).agents[0], false)
})

test('names and workspaces match exactly, and the lists combine by OR', () => {
  const { watch, events } = setup()
  watch.connect([{ id: 's', match: { names: ['Other'], workspaces: ['Site'] }, kinds: ['turn_done'] }])
  turn(watch, X, 'by workspace')
  turn(watch, Y, 'by name')
  watch.sessionEvent({ id: 'sess-z' }, { type: 'turn/end', data: { reason: { kind: 'completed' } } })
  assert.deepEqual(events.map(e => e.session_id), ['sess-x', 'sess-y'])
})

test('an approval opens on approval/asked and clears on approval/decided; the reason is cut to 500', () => {
  const { watch, events } = setup()
  watch.connect(subX)
  watch.sessionEvent(X, { type: 'approval/asked', data: { id: 'ap1', toolName: 'bash', callId: 'call-1', reason: 'r'.repeat(900) } })
  assert.equal(events[0].kind, 'approval_waiting')
  assert.equal(events[0].approval.call_id, 'call-1')
  assert.equal(events[0].approval.reason.length, 500)
  assert.deepEqual(watch.agents(subX).approvals.map(a => a.call_id), ['call-1'])
  assert.deepEqual(watch.agents([]).approvals, [], 'an unwatched set sees no approvals')
  watch.sessionEvent(X, { type: 'approval/decided', data: { id: 'ap1', outcome: 'allowed-once' } })
  assert.deepEqual(watch.agents(subX).approvals, [])
  assert.equal(events[1].status, 'approval_decided')
})

test('a session/title event names the Session', () => {
  const { watch } = setup()
  watch.sessionEvent({ id: 'sess-q' }, { type: 'session/title', data: { title: 'Renamed' } })
  assert.equal(watch.agents([]).agents.find(a => a.session_id === 'sess-q').name, 'Renamed')
})

test('a draft or view from an old connection or a lower generation is refused (409)', () => {
  const { watch } = setup()
  const first = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const second = watch.connect(subX)
  const draft = { conn_id: first.conn_id, generation: 1, effect_id: 'e1', token: TOKEN, session_id: 'sess-x', text: 'go' }
  refusal(() => watch.register(draft), 409, 'stale_generation')
  watch.register({ ...draft, conn_id: second.conn_id, generation: 5 })
  refusal(() => watch.register({ ...draft, conn_id: second.conn_id, generation: 4, effect_id: 'e2' }), 409, 'stale_generation')
  const view = { conn_id: second.conn_id, generation: 3, subscriptions: [], log_tail: [], advice: [] }
  refusal(() => watch.setView(view), 409, 'stale_generation')
  watch.setView({ ...view, generation: 6 })
  refusal(() => watch.setView({ ...view, conn_id: first.conn_id, generation: 9 }), 409, 'stale_generation')
})

test('re-registering an effect id, or a sent one, is refused; a new draft supersedes the old; labels are refused', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const base = { conn_id, generation: 1, token: TOKEN, session_id: 'sess-x' }
  const reg = watch.register({ ...base, effect_id: 'e1', text: 'first' })
  assert.deepEqual(reg, { effect_id: 'e1', session_id: 'sess-x', name: 'Builder', workspace: 'Site' })
  refusal(() => watch.register({ ...base, effect_id: 'e1', text: 'again' }), 409, 'already_registered')
  watch.register({ ...base, effect_id: 'e2', text: 'second' })
  assert.deepEqual(watch.snapshot().drafts.map(d => d.effect_id), ['e2'], 'e2 superseded e1')
  assert.deepEqual(watch.takeDraft('e2', TOKEN), { session_id: 'sess-x', text: 'second' })
  refusal(() => watch.register({ ...base, effect_id: 'e2', text: 'once more' }), 409, 'already_registered')
  refusal(() => watch.register({ ...base, effect_id: 'e3', text: 'x', name: 'Builder' }), 400, 'body_refused')
  refusal(() => watch.register({ ...base, effect_id: 'e4', text: 'x', workspace: 'Site' }), 400, 'body_refused')
  refusal(() => watch.register({ ...base, effect_id: 'e5', text: 'x', session_id: 'nobody' }), 404, 'not_found')
})

test('a wrong token takes nothing; the right one takes the draft once', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  watch.register({ conn_id, generation: 1, effect_id: 'e1', token: TOKEN, session_id: 'sess-x', text: 'go' })
  refusal(() => watch.takeDraft('e1', 'tok-wrong-not-a-secret'), 403, 'bad_token')
  assert.equal(watch.snapshot().drafts.length, 1)
  watch.takeDraft('e1', TOKEN)
  refusal(() => watch.takeDraft('e1', TOKEN), 404, 'not_found')
})

test('control events with no stream connected arrive in order on the next connect; a full queue gives 503', () => {
  let n = 0
  const watch = createWatch({ newId: () => `c${++n}`, controlMax: 3 })
  const first = watch.connect([])
  watch.disconnect(first.conn_id)
  watch.control({ kind: 'sub_add', session_id: 'a' })
  watch.control({ kind: 'sub_remove', id: 'b' })
  watch.control({ kind: 'sub_undo', change_id: 'c' })
  assert.equal(watch.canControl(), false)
  refusal(() => watch.control({ kind: 'sub_add', session_id: 'd' }), 503, 'agentd_unavailable')
  const { backlog } = watch.connect([])
  assert.deepEqual(backlog.map(c => c.kind), ['sub_add', 'sub_remove', 'sub_undo'])
  assert.equal(watch.canControl(), true)
})

test('a new connection drops the old drafts and view but keeps spent ids', () => {
  const { watch } = setup()
  const a = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }, { sessionId: 'sess-y' }])
  watch.register({ conn_id: a.conn_id, generation: 1, effect_id: 'e1', token: TOKEN, session_id: 'sess-x', text: 'go' })
  watch.takeDraft('e1', TOKEN)
  watch.register({ conn_id: a.conn_id, generation: 1, effect_id: 'e2', token: TOKEN, session_id: 'sess-y', text: 'still live' })
  watch.setView({ conn_id: a.conn_id, generation: 1, subscriptions: [], log_tail: ['x'], advice: [] })
  assert.equal(watch.snapshot().drafts.length, 1, 'e2 is live before the new connection')
  const b = watch.connect(subX)
  assert.deepEqual(watch.snapshot().drafts, [], 'the live draft e2 is dropped')
  assert.equal(watch.snapshot().view, null)
  watch.register({ conn_id: b.conn_id, generation: 1, effect_id: 'e2', token: TOKEN, session_id: 'sess-y', text: 'registered again' })
  refusal(() => watch.register({ conn_id: b.conn_id, generation: 1, effect_id: 'e1', token: TOKEN, session_id: 'sess-x', text: 'go' }), 409, 'already_registered')
})

test('a summary recorded under a wider stream never reaches a narrower agents call', () => {
  const { watch } = setup()
  watch.connect(subX)
  turn(watch, X, 'X finished the page')
  const subY = [{ id: 'y', match: { session_ids: ['sess-y'] }, kinds: ['turn_done'] }]
  for (const set of [[], subY]) {
    const x = watch.agents(set).agents.find(a => a.session_id === 'sess-x')
    assert.equal('last_summary' in x, false)
    assert.equal('last_turn_at' in x, false)
  }
})

test('turn summaries are cut to 500 characters, in the event and in the agents list', () => {
  const { watch, events } = setup()
  watch.connect(subX)
  turn(watch, X, 'a'.repeat(1200))
  assert.equal(events[0].summary.length, 500)
  assert.equal(watch.agents(subX).agents.find(a => a.session_id === 'sess-x').last_summary.length, 500)
})

test('an approval clear reaches a subscription that has approval_waiting but not status', () => {
  const { watch, events } = setup()
  watch.connect([{ id: 'a', match: { session_ids: ['sess-x'] }, kinds: ['approval_waiting'] }])
  watch.sessionEvent(X, { type: 'approval/asked', data: { id: 'ap1', toolName: 'bash', callId: 'call-1', reason: 'r' } })
  watch.sessionEvent(X, { type: 'approval/decided', data: { id: 'ap1', outcome: 'rejected' } })
  assert.deepEqual(events.map(e => [e.kind, e.status]), [['approval_waiting', undefined], ['status', 'approval_decided']])
  watch.agentStatus('sess-x', 'running', X)
  assert.equal(events.length, 2, 'a plain status event still needs the status kind')
})

test('a superseded or withdrawn draft id is never registered again', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const base = { conn_id, generation: 1, token: TOKEN, session_id: 'sess-x' }
  watch.register({ ...base, effect_id: 'e1', text: 'one' })
  watch.register({ ...base, effect_id: 'e2', text: 'two' })
  refusal(() => watch.register({ ...base, effect_id: 'e1', text: 'one again' }), 409, 'already_registered')
  watch.unregister('e2')
  refusal(() => watch.register({ ...base, effect_id: 'e2', text: 'two again' }), 409, 'already_registered')
})

test('views rise strictly within a connection; drafts may share a generation', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const view = { conn_id, generation: 2, subscriptions: [], log_tail: [], advice: [] }
  watch.setView(view)
  refusal(() => watch.setView(view), 409, 'stale_generation')
  watch.register({ conn_id, generation: 2, effect_id: 'e1', token: TOKEN, session_id: 'sess-x', text: 'a' })
  watch.register({ conn_id, generation: 2, effect_id: 'e2', token: TOKEN, session_id: 'sess-x', text: 'b' })
  watch.setView({ ...view, generation: 3 })
})

test('a taken draft\'s outcome is recorded past a full control queue; a new tap is not', () => {
  let n = 0
  const watch = createWatch({ newId: () => `c${++n}`, controlMax: 1 })
  watch.disconnect(watch.connect([]).conn_id)
  watch.control({ kind: 'sub_add', session_id: 'a' })
  refusal(() => watch.control({ kind: 'sub_add', session_id: 'b' }), 503, 'agentd_unavailable')
  watch.control({ kind: 'sent', effect_id: 'e1' }, { outcome: true })
  assert.deepEqual(watch.connect([]).backlog.map(c => c.kind), ['sub_add', 'sent'])
})


// The bridge refuses only bidi controls and tags; the other Refused vectors are agentd's and the
// client's to catch.
const HOST_REFUSED = REFUSED.filter(text => /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]|[\u{E0000}-\u{E007F}]/u.test(text))

test("the wish's hidden-character vectors at the bridge", () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const base = { conn_id, generation: 1, token: TOKEN, session_id: 'sess-x' }
  assert.deepEqual(HOST_REFUSED, ['file\u202Etxt.exe', 'run\u{E0074}', 'a\u061Cb'])
  HOST_REFUSED.forEach((text, n) => refusal(() => watch.register({ ...base, effect_id: `r${n}`, text }), 400, 'body_refused'))
  ALLOWED.forEach((text, n) => watch.register({ ...base, effect_id: `a${n}`, text }))
  // One draft per Session: the last Allowed one is held, exactly as sent.
  assert.deepEqual(watch.snapshot().drafts.map(d => d.text), [ALLOWED.at(-1)])
})

test('a draft holding bidi controls or tag characters is refused', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x' }])
  const base = { conn_id, generation: 1, token: TOKEN, session_id: 'sess-x' }
  const hiding = ['\u061C', '\u202E', '\u202A', '\u2066', '\u2069', '\u200E', '\u200F', '\u{E0001}', '\u{E0074}', '\u{E007F}']
  hiding.forEach((char, n) => refusal(() => watch.register({ ...base, effect_id: `h${n}`, text: `run the tests${char}` }), 400, 'body_refused'))
  assert.deepEqual(watch.snapshot().drafts, [])
  // An emoji with its joiner and variation selector is ordinary text.
  watch.register({ ...base, effect_id: 'ok', text: 'done 👩\u200D💻 \u2764\uFE0F' })
  assert.deepEqual(watch.snapshot().drafts.map(d => d.effect_id), ['ok'])
})

// The DSH client's badges and its first-run suggestion read these from the snapshot.
test('the snapshot carries who added a subscription and each Session\'s last turn', () => {
  const { watch } = setup()
  const { conn_id } = watch.connect(subX)
  watch.listed([{ sessionId: 'sess-x', updatedAt: 1234 }, { sessionId: 'sess-y' }])
  watch.setView({ conn_id, generation: 1, subscriptions: [{ ...subX[0], added_by: 'unfold', at: 99 }, { id: 's2', match: { names: ['writer'] }, kinds: ['turn_done'], added_by: 'someone' }], log_tail: [], advice: [] })
  const snap = watch.snapshot()
  assert.deepEqual(snap.view.subscriptions.map(sub => [sub.id, sub.added_by, sub.at]), [['s1', 'unfold', 99], ['s2', undefined, undefined]])
  assert.deepEqual(snap.agents.map(agent => [agent.session_id, agent.last_turn_at]), [['sess-x', 1234], ['sess-y', undefined]])
  // An older list time never moves a newer turn back.
  watch.listed([{ sessionId: 'sess-x', updatedAt: 1000 }])
  assert.equal(watch.snapshot().agents[0].last_turn_at, 1234)
})
