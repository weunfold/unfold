// The session adapter's bridge against a scripted control endpoint and a hand-fed observer:
// one submit per composer message, never resent; gaps and stop deadlines. No DSH, no network.
import test from 'node:test'
import assert from 'node:assert/strict'
import { SessionBridge } from '../src/bridge.js'
import { within } from './wait.js'
import { finalText } from '../index.js'

/** A control endpoint whose answers are scripted per call; records every call. */
function fakeHost(script) {
  const calls = []
  return {
    calls,
    async submit(session, text) {
      calls.push({ op: 'submit', session, text })
      const next = script.shift() ?? accepted()
      return typeof next === 'function' ? next() : next
    },
  }
}

const accepted = (turn_id = 'c1') => ({ status: 200, body: { accepted: true, turn_id } })
const fast = { stopMs: 200, gapWaitMs: 60 }

function bridge(script, timing = fast) {
  const host = fakeHost(script)
  const b = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing })
  b.link.session_id = 's1'
  b.onLink('attached')
  return { b, host }
}

function collect(gen, ms = 10_000) {
  return within((async () => {
    const evs = []
    for await (const ev of gen) evs.push(ev)
    return evs
  })(), ms, 'a bridge run')
}

const later = (ms, fn) => setTimeout(fn, ms)
const submits = (host) => host.calls.filter((c) => c.op === 'submit')

test('one admission, live deltas, completed: the bubble is the whole live reply', async () => {
  const { b, host } = bridge([accepted()])
  const run = collect(b.run('k1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'hi ' })
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'there' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const evs = await run
  const final = evs.at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'hi there')
  assert.deepEqual(submits(host).map((c) => [c.session, c.text]), [['s1', 'hi']])
  assert.deepEqual(evs[0], { type: 'admitted', command: { command_id: 'c1', status: 'admitted' }, duplicate: false })
})

test("what agentd broadcasts before the endpoint's 200 is read is adopted, in order", async () => {
  const { b } = bridge([() => {
    b.ingest({ kind: 'command_admitted', command_id: 'c1', generation: 1, status: 'admitted' })
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'early ' })
    b.ingest({ kind: 'response_started', request_id: 'other', response_id: 'r2' })
    b.ingest({ kind: 'agent_delta', response_id: 'r2', text: 'not ours' })
    return accepted()
  }])
  const run = collect(b.run('k1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'and late' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.text, 'early and late')
  assert.equal(final.status, 'completed')
  assert.equal(b.stats.early_adopted, 3)
})

test('a composer message is submitted at most once: a key seen before is never sent again', async () => {
  const { b, host } = bridge([accepted()])
  const first = collect(b.run('k1', 'once'))
  later(5, () => {
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  await first
  const again = (await collect(b.run('k1', 'once'))).at(-1)
  assert.equal(again.status, 'unknown')
  assert.match(again.detail, /already handled \(sent as c1\); it is never sent again/)
  assert.equal(submits(host).length, 1)
})

test('a lost connection after the message was written is never resent: shown as unknown', async () => {
  const { b, host } = bridge([{ status: 0, body: { error: 'transport', sent: 'unknown' } }])
  const final = (await collect(b.run('k1', 'hi'))).at(-1)
  assert.equal(final.status, 'unknown')
  assert.match(finalText(final), /outcome unknown: .*may have been sent/)
  assert.equal(submits(host).length, 1)
  assert.equal((await collect(b.run('k1', 'hi'))).at(-1).status, 'unknown')
  assert.equal(submits(host).length, 1, 'not even when the same message comes again')
})

test('abort, then a late delta and no terminal: the stop deadline still ends the turn, unconfirmed', async () => {
  const { b, host } = bridge([accepted()])
  const ac = new AbortController()
  const t0 = Date.now()
  const run = collect(b.run('k1', 'count slowly', ac.signal))
  later(5, () => b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' }))
  later(10, () => ac.abort())
  later(30, () => b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'late' }))
  const evs = await run
  const final = evs.at(-1)
  assert.equal(final.status, 'stop_unconfirmed', 'never a fabricated "cancelled"')
  assert.match(final.detail, /outcome in Unfold is unknown/)
  assert.ok(Date.now() - t0 < 200 + 10 + 150, `ended at ${Date.now() - t0} ms`)
  assert.equal(submits(host).length, 1, 'a stop sends nothing: the endpoint has no cancel')
  assert.match(finalText(final), /stopped locally; the backend outcome is unknown/)
})

test('a backend-confirmed cancel is "cancelled"', async () => {
  const { b } = bridge([accepted()])
  const ac = new AbortController()
  const run = collect(b.run('k1', 'x', ac.signal))
  later(5, () => b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' }))
  later(10, () => ac.abort())
  later(20, () => b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'cancelled' }))
  assert.equal((await run).at(-1).status, 'cancelled')
})

test('a reattach mid-reply loses middle chunks: the bubble keeps the live text, marked possibly incomplete', async () => {
  const { b, host } = bridge([accepted()])
  const run = collect(b.run('k1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'Hello ' })
    b.onLink('waiting')
    b.onLink('attached') // "big " was emitted while we were away
  })
  later(20, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'world' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete', 'never plain completed after a gap')
  assert.equal(final.text, 'Hello world')
  assert.match(finalText(final), /possibly incomplete: the live stream had a gap; the reply ended \(completed\)/)
  assert.equal(submits(host).length, 1, 'nothing is asked again')
})

test('the response completes before the observer attaches: shown possibly incomplete after the bounded wait', async () => {
  const host = fakeHost([accepted()])
  const b = new SessionBridge({ host, observer: { start() {}, stop() {} }, timing: fast })
  b.link.session_id = 's1'
  const run = collect(b.run('k1', 'hi')) // submitted while the observer is not attached
  later(20, () => b.onLink('attached'))
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.match(final.detail, /no event of this reply arrived/)
  assert.equal(b.stats.gaps, 1)
})

test('after a 200, a reply whose broadcast never comes ends as not seen, bounded, never sent again', async () => {
  const { b, host } = bridge([accepted()], { stopMs: 200, gapWaitMs: 5000, typedIdleMs: 80 })
  const t0 = Date.now()
  const final = (await collect(b.run('k1', 'hi'), 2000)).at(-1)
  assert.equal(final.status, 'not_seen')
  assert.match(finalText(final), /^\[reply not seen: Unfold took this message \(c1\), but no event of its reply arrived/)
  assert.ok(Date.now() - t0 >= 75, `waited ${Date.now() - t0} ms`)
  assert.equal(submits(host).length, 1, 'nothing is submitted again')
})

test('a typed reply that goes quiet after some text ends possibly incomplete; each event renews the wait', async () => {
  const { b } = bridge([accepted()], { stopMs: 200, gapWaitMs: 5000, typedIdleMs: 60 })
  const t0 = Date.now()
  const run = collect(b.run('k1', 'hi'), 2000)
  later(5, () => b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' }))
  later(45, () => b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'part' }))
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'part')
  assert.ok(Date.now() - t0 >= 100, `the delta renewed the wait (${Date.now() - t0} ms)`)
})

test('a gap with no live end and no further event ends incomplete; each event renews the wait', async () => {
  const { b } = bridge([accepted()], { stopMs: 200, gapWaitMs: 50 })
  const t0 = Date.now()
  const run = collect(b.run('k1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.onLink('waiting')
    b.onLink('attached')
  })
  later(40, () => b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'part' }))
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'part')
  assert.ok(Date.now() - t0 >= 85, `the delta renewed the wait (${Date.now() - t0} ms)`)
})

test('an interruption agentd broadcasts for the command is reported honestly', async () => {
  const { b } = bridge([accepted()])
  const run = collect(b.run('k1', 'hi'))
  later(5, () => b.ingest({ kind: 'command_admitted', command_id: 'c1', status: 'interrupted', generation: 1, detail: 'agent restarted after dispatch, before a result was recorded: not re-run' }))
  const final = (await run).at(-1)
  assert.equal(final.status, 'interrupted')
  assert.match(finalText(final), /\[interrupted: agent restarted after dispatch/)
})

test('an owner command refused by the endpoint is shown as a refusal', async () => {
  const { b } = bridge([{ status: 400, body: { refused: 'command', sent: false } }])
  const final = (await collect(b.run('k1', '/grant'))).at(-1)
  assert.equal(final.status, 'rejected')
  assert.match(finalText(final), /Unfold refused this message: a message that starts with "\/" is an owner command/)
})

// ── the one stop deadline holds while the bridge waits on the endpoint ──

const stalled = (ms, answer) => () => new Promise((resolve) => setTimeout(() => resolve(answer), ms))
const quick = { stopMs: 40, gapWaitMs: 5000 }

async function abortedWithin(script, setup) {
  const { b, host } = bridge(script, quick)
  const ac = new AbortController()
  const run = collect(b.run('k1', 'hi', ac.signal))
  await setup(b, ac)
  const t0 = Date.now()
  ac.abort()
  const evs = await run
  return { final: evs.at(-1), ms: Date.now() - t0, host, evs }
}

test('abort during an in-flight submit: ends by the deadline, no second submit, not "cancelled"', async () => {
  const { final, ms, host } = await abortedWithin([stalled(500, accepted())], () => new Promise((r) => setTimeout(r, 5)))
  assert.equal(final.status, 'stop_unconfirmed')
  assert.ok(ms < 100, `took ${ms} ms`)
  assert.equal(submits(host).length, 1, 'no new command was created')
})

test('abort during the wait after a gap: ends by the deadline', async () => {
  const { final, ms } = await abortedWithin([accepted()], async (b) => {
    await new Promise((r) => setTimeout(r, 5))
    b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
    b.onLink('waiting')
    b.onLink('attached')
    await new Promise((r) => setTimeout(r, 10))
  })
  assert.equal(final.status, 'stop_unconfirmed')
  assert.ok(ms < 100, `took ${ms} ms`)
})

test('a stop before the message was sent sends nothing', async () => {
  const { b, host } = bridge([])
  const ac = new AbortController()
  ac.abort()
  const final = (await collect(b.run('k1', 'hi', ac.signal))).at(-1)
  assert.equal(final.status, 'not_sent')
  assert.equal(submits(host).length, 0)
})

test('unload during an in-flight submit ends the turn at once, as unknown', async () => {
  const { b } = bridge([stalled(500, accepted())], quick)
  const run = collect(b.run('k1', 'hi'))
  await new Promise((r) => setTimeout(r, 5))
  const t0 = Date.now()
  b.stop()
  const final = (await run).at(-1)
  assert.equal(final.status, 'unknown')
  assert.ok(Date.now() - t0 < 50)
})

for (const partial of [false, true]) test(`accepted typed turn settles after observer disconnect (partial=${partial})`, async () => {
  const { b, host } = bridge([accepted()], { ...fast, typedIdleMs: 40 })
  const run = collect(b.run('disconnect', 'hello'), 300)
  later(5, () => {
    if (partial) {
      b.ingest({ kind: 'response_started', request_id: 'c1', response_id: 'r1' })
      b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'partial' })
    }
    b.onLink('disconnected')
  })
  const final = (await run).at(-1)
  assert.equal(final.status, partial ? 'incomplete' : 'not_seen')
  assert.equal(submits(host).length, 1)
  assert.equal(b.tracks.size, 0)
})

for (const id of ['', ' \t\n ']) test(`malformed accepted turn id ${JSON.stringify(id)} settles unknown without retry`, async () => {
  const { b, host } = bridge([accepted(id)], { ...fast, typedIdleMs: 40 })
  const events = await collect(b.run('invalid-id', 'hello'), 300)
  assert.equal(events.some((e) => e.type === 'admitted'), false)
  assert.equal(events.at(-1).status, 'unknown')
  assert.match(events.at(-1).detail, /form this plugin cannot read/)
  assert.equal((await collect(b.run('invalid-id', 'hello'), 300)).at(-1).status, 'unknown')
  assert.equal(submits(host).length, 1)
  assert.equal(b.tracks.size, 0)
})
