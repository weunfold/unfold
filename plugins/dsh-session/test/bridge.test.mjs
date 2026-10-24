// The session adapter's bridge against a scripted host owner and a hand-fed observer: lost
// acks, recorded results after gaps, stop deadlines. No DSH, no network.
import test from 'node:test'
import assert from 'node:assert/strict'
import { RthBridge } from '../src/bridge.mjs'
import { finalText } from '../index.mjs'

/** A host owner whose answers are scripted per call; records every call. */
function fakeHost(script) {
  const calls = []
  return {
    calls,
    async submit(command_id, text) {
      calls.push({ op: 'submit', command_id, text })
      const next = script.shift() ?? { status: 200, body: { command: { command_id, status: 'dispatched', generation: 1 } } }
      return typeof next === 'function' ? next(command_id) : next
    },
    async cancel() {
      calls.push({ op: 'cancel' })
      return { status: 200, body: { ok: true } }
    },
  }
}

const admitted = (status = 'admitted', extra = {}) => ({ status: 200, body: { command: { command_id: 'c1', status, generation: 1, ...extra }, duplicate: status !== 'admitted' } })
const fast = { stopMs: 200, reaskMs: [20, 40, 80], reconcileAttempts: 5 }

function bridge(script, timing = fast) {
  const host = fakeHost(script)
  const b = new RthBridge({ host, observer: { start() {}, stop() {} }, timing })
  b.onLink('attached')
  return { b, host }
}

async function collect(gen) {
  const evs = []
  for await (const ev of gen) evs.push(ev)
  return evs
}

const later = (ms, fn) => setTimeout(fn, ms)

test('one admission, live deltas, completed: the bubble is the whole live reply', async () => {
  const { b, host } = bridge([admitted()])
  const run = collect(b.run('c1', 'hi'))
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
  assert.equal(host.calls.filter((c) => c.op === 'submit').length, 1)
})

test('a lost ack is retried with the same id and yields one admitted result', async () => {
  const host = fakeHost([admitted(), admitted('dispatched', { response_id: 'r1' })])
  const b = new RthBridge({ host, observer: { start() {}, stop() {} }, faults: { dropAcks: 1 }, timing: fast })
  b.onLink('attached')
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'ok' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const evs = await run
  assert.deepEqual(host.calls.filter((c) => c.op === 'submit').map((c) => c.command_id), ['c1', 'c1'])
  assert.equal(evs.filter((e) => e.type === 'admitted').length, 1)
  assert.equal(b.stats.lost_acks_injected, 1)
  assert.equal(evs.at(-1).status, 'completed')
})

test('abort, then a late delta and no terminal: the stop deadline still ends the turn, unconfirmed', async () => {
  const { b, host } = bridge([admitted('dispatched', { response_id: 'r1' })])
  const ac = new AbortController()
  const t0 = Date.now()
  const run = collect(b.run('c1', 'count slowly', ac.signal))
  later(10, () => ac.abort())
  later(30, () => b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'late' }))
  const evs = await run
  const final = evs.at(-1)
  assert.equal(final.status, 'stop_unconfirmed', 'never a fabricated "cancelled"')
  assert.match(final.detail, /backend outcome is unknown/)
  assert.ok(Date.now() - t0 < 200 + 10 + 150, `ended at ${Date.now() - t0} ms`)
  assert.equal(host.calls.filter((c) => c.op === 'cancel').length, 1)
  assert.match(finalText(final), /stopped locally; the backend outcome is unknown/)
})

test('a backend-confirmed cancel is "cancelled"', async () => {
  const { b } = bridge([admitted('dispatched', { response_id: 'r1' })])
  const ac = new AbortController()
  const run = collect(b.run('c1', 'x', ac.signal))
  later(10, () => ac.abort())
  later(20, () => b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'cancelled' }))
  assert.equal((await run).at(-1).status, 'cancelled')
})

test('a reattach mid-reply loses middle chunks; the bubble is the full RECORDED result', async () => {
  const { b, host } = bridge([
    admitted('dispatched', { response_id: 'r1' }),
    // the reattach re-ask: still running
    admitted('dispatched', { response_id: 'r1' }),
    // reconciliation after the live terminal: the record has the whole reply
    admitted('completed', { response_id: 'r1', result_text: 'Hello big world', text_complete: true, source: 'command_log' }),
  ])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'Hello ' })
    b.onLink('waiting')
    b.onLink('attached') // "big " was emitted while we were away
  })
  later(60, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'world' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'Hello big world', 'not the truncated live text')
  assert.ok(host.calls.length >= 3)
})

test('the response completes before the observer attaches: the record supplies it', async () => {
  const host = fakeHost([admitted(), admitted('completed', { response_id: 'r1', result_text: 'done while unseen', text_complete: true, source: 'command_log' })])
  const b = new RthBridge({ host, observer: { start() {}, stop() {} }, timing: fast })
  const run = collect(b.run('c1', 'hi')) // submitted while the observer is not attached
  later(20, () => b.onLink('attached'))
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'done while unseen')
  assert.equal(b.stats.gaps, 1)
})

test('reconciliation temporarily unavailable (503): retried with backoff, then the recorded result', async () => {
  const unavailable = { status: 503, body: { error: 'agent link is down', retry: true } }
  const { b } = bridge([admitted('dispatched', { response_id: 'r1' }), unavailable, unavailable, admitted('completed', { response_id: 'r1', result_text: 'full reply', text_complete: true, source: 'command_log' })])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.onLink('waiting')
    b.onLink('attached')
  })
  later(8, () => b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' }))
  const final = (await run).at(-1)
  assert.equal(final.text, 'full reply')
  assert.ok(b.stats.reconciled >= 1)
})

test('reconciliation never available: the bubble says it may be incomplete, never plain completed', async () => {
  const unavailable = { status: 503, body: { error: 'down', retry: true } }
  const { b } = bridge([admitted('dispatched', { response_id: 'r1' }), ...Array(10).fill(unavailable)])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'part' })
    b.onLink('waiting')
    b.onLink('attached')
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.match(finalText(final), /possibly incomplete/)
})

test('a retry after an agent restart reports the recorded interruption honestly', async () => {
  const { b } = bridge([admitted('interrupted', { response_id: 'r1', detail: 'agent restarted after dispatch, before a result was recorded: the effect may have started (unknown); not re-run' })])
  const final = (await collect(b.run('c1', 'hi'))).at(-1)
  assert.equal(final.status, 'interrupted')
  assert.match(finalText(final), /\[interrupted: agent restarted after dispatch/)
})

test('a refusal is shown as a refusal', async () => {
  const { b } = bridge([{ status: 409, body: { rejected: 'command c1: stale generation' } }])
  const final = (await collect(b.run('c1', 'hi'))).at(-1)
  assert.equal(final.status, 'rejected')
  assert.match(finalText(final), /Unfold refused this command: command c1: stale generation/)
})

// ── the one stop deadline holds while the bridge waits on the owner ──

const stalled = (ms, answer) => () => new Promise((resolve) => setTimeout(() => resolve(answer), ms))
const quick = { stopMs: 40, reaskMs: [10, 10, 10], reconcileAttempts: 8 }

async function abortedWithin(script, setup, bound = 100) {
  const { b, host } = bridge(script, quick)
  const ac = new AbortController()
  const run = collect(b.run('c1', 'hi', ac.signal))
  await setup(b, ac)
  const t0 = Date.now()
  ac.abort()
  const evs = await run
  return { final: evs.at(-1), ms: Date.now() - t0, host, evs }
}

test('abort during an in-flight admission: ends by the deadline, no second command, not "cancelled"', async () => {
  const { final, ms, host } = await abortedWithin([stalled(500, admitted())], () => new Promise((r) => setTimeout(r, 5)))
  assert.equal(final.status, 'stop_unconfirmed')
  assert.ok(ms < 100, `took ${ms} ms`)
  assert.equal(host.calls.filter((c) => c.op === 'submit').length, 1, 'no new command was created')
})

test('abort during a stalled re-ask after a gap: ends by the deadline', async () => {
  const { final, ms } = await abortedWithin([admitted('dispatched', { response_id: 'r1' }), stalled(500, admitted('dispatched', { response_id: 'r1' }))], async (b) => {
    await new Promise((r) => setTimeout(r, 5))
    b.onLink('waiting')
    b.onLink('attached')
    await new Promise((r) => setTimeout(r, 20)) // the re-ask is now in flight
  })
  assert.equal(final.status, 'stop_unconfirmed')
  assert.ok(ms < 100, `took ${ms} ms`)
})

test('abort during a stalled terminal reconciliation: ends by the deadline', async () => {
  const { final, ms } = await abortedWithin([admitted('dispatched', { response_id: 'r1' }), stalled(500, admitted('completed', { response_id: 'r1', result_text: 'x' }))], async (b) => {
    await new Promise((r) => setTimeout(r, 5))
    b.onLink('waiting')
    b.onLink('attached')
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
    await new Promise((r) => setTimeout(r, 5)) // the reconciliation request is in flight
  })
  assert.equal(final.status, 'stop_unconfirmed')
  assert.ok(ms < 100, `took ${ms} ms`)
})

test('unload during an in-flight admission ends the turn at once, as unknown', async () => {
  const { b } = bridge([stalled(500, admitted())], quick)
  const run = collect(b.run('c1', 'hi'))
  await new Promise((r) => setTimeout(r, 5))
  const t0 = Date.now()
  b.stop()
  const final = (await run).at(-1)
  assert.equal(final.status, 'unknown')
  assert.ok(Date.now() - t0 < 50)
})

test('a gapped completed record without its text stays incomplete (defensive: rth-agentd always records the text)', async () => {
  const { b } = bridge([admitted('dispatched', { response_id: 'r1' }), admitted('completed', { response_id: 'r1' })])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'part' })
    b.onLink('waiting')
    b.onLink('attached')
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.match(finalText(final), /possibly incomplete/)
})

// ── on the typed path: a recorded result_text replaces the known text only when whole ──

test('a gapped typed reply whose record has a suffix (no text_complete) keeps the known text, marked incomplete', async () => {
  const { b } = bridge([admitted('dispatched', { response_id: 'r1' }), admitted('completed', { response_id: 'r1', result_text: 'world', text_complete: false, source: 'observed', observed: 'suffix' })])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'Hello ' })
    b.onLink('waiting')
    b.onLink('attached')
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
  assert.equal(final.text, 'Hello ', 'never replaced by the suffix')
  assert.match(finalText(final), /possibly incomplete: recorded as completed, but without a complete text \(observed suffix\)/)
})

test('a gapped typed reply whose record lacks text_complete (older owner) counts as incomplete', async () => {
  const { b } = bridge([admitted(), admitted('completed', { response_id: 'r1', result_text: 'maybe whole' })])
  const run = collect(b.run('c1', 'hi')) // submitted before attach: a gap
  b.link.state = 'waiting'
  later(20, () => b.onLink('attached'))
  const final = (await run).at(-1)
  assert.equal(final.status, 'incomplete')
})

test('without a gap, a completed record lacking text_complete waits for the live end and keeps the live text', async () => {
  const { b } = bridge([admitted('completed', { response_id: 'r1', result_text: 'tail' })])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'whole live text' })
    b.ingest({ kind: 'agent_done', response_id: 'r1', status: 'completed' })
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'whole live text')
})

test('a typed reply across a reconnect stays incomplete until a command_log result arrives, then takes it', async () => {
  const suffix = admitted('completed', { response_id: 'r1', result_text: 'world', text_complete: false, source: 'observed', observed: 'suffix' })
  const { b, host } = bridge([
    admitted('dispatched', { response_id: 'r1' }),
    suffix, // the reattach re-ask: ended, text not whole
    suffix, // reconciliation, first ask: still only the observation
    admitted('completed', { response_id: 'r1', result_text: 'Hello big world', text_complete: true, source: 'command_log', observed: 'complete' }),
  ])
  const run = collect(b.run('c1', 'hi'))
  later(5, () => {
    b.ingest({ kind: 'agent_delta', response_id: 'r1', text: 'Hello ' })
    b.onLink('waiting')
    b.onLink('attached') // "big world" and the end happened while away
  })
  const final = (await run).at(-1)
  assert.equal(final.status, 'completed')
  assert.equal(final.text, 'Hello big world')
  assert.deepEqual([...new Set(host.calls.map((c) => c.command_id))], ['c1'], 'one command id throughout')
})
