// A small stand-in for the DSH host (one root agent whose turns run the plugin's agent/pre-step,
// a Session that records appends, the Session API's prompt), shared by the apply() tests.
import assert from 'node:assert/strict'
import * as llm from '@deepseek-ai/dsh-llm'
import { within } from './wait.js'

/** A stand-in DSH: one live unfold-root agent; queued messages run one turn each, in order. */
export function fakeDsh({ live = true, sessionApi = true, stallPrompt = false, root = 'unfold-root', logger = { info: () => {}, warn: () => {} } } = {}) {
  const handlers = {}
  const events = []
  const prompts = []
  let turn = 0
  let running = Promise.resolve()
  const session = {
    id: root,
    header: {},
    append(type, data) {
      const ev = { type, data, seq: events.length }
      events.push(ev)
      return ev
    },
    snapshotEvents: () => events,
  }
  // As DSH's AgentLoop (dsh-agent-loop 0.2.1-rc.3): a wake while idle starts the driver; a
  // turn claims EVERY next-step message plus the FIRST next-turn one; because our pre-step
  // rejects, `turn()` returns false and the driver goes idle (`while (await this.turn())`),
  // even with work pending. A wake while running is dropped, so that work stays parked.
  let busy = false
  const nextTurn = []
  const nextStep = []
  const agent = {
    id: root,
    session,
    get status() {
      return busy ? 'running' : 'idle'
    },
    inbox: {
      nextTurn,
      nextStep,
      get hasPending() {
        return nextTurn.length > 0 || nextStep.length > 0
      },
      remove(id) {
        for (const list of [nextStep, nextTurn]) {
          const i = list.findIndex((m) => m.id === id)
          if (i >= 0) {
            list.splice(i, 1)
            return true
          }
        }
        return false
      },
    },
    whenIdle: () => running,
    send(message, target, wakeup) {
      ;(target === 'next-step' ? nextStep : nextTurn).push(message)
      if (!wakeup || busy) return
      busy = true
      const claimed = [...nextStep.splice(0), ...nextTurn.splice(0, 1)]
      running = (async () => {
        try {
          const [pre] = handlers['agent/pre-step']
          const decision = await pre({ agent, messages: claimed, turn: ++turn, signal: new AbortController().signal }, () => ({ kind: 'enter' }))
          assert.deepEqual(decision, { kind: 'reject' }, 'every unfold-root turn is rejected: no model step')
        } finally {
          busy = false
        }
      })()
    },
    followup(message) {
      agent.send(message, 'next-turn', true)
    },
    steer(message) {
      agent.send(message, 'next-step', true)
    },
  }
  const hasRequest = (id) => [...nextTurn, ...nextStep].some((m) => m.source.rpcId === id) || events.some((e) => e.type === 'user/message' && e.data.source.rpcId === id)
  const sessionController = {
    async prompt(req) {
      prompts.push(req.requestId)
      if (stallPrompt) return new Promise(() => {})
      if (hasRequest(req.requestId)) return { accepted: true } // DSH's own durable dedupe
      agent.followup(llm.createUserMessage({ content: req.content, source: { kind: 'user', rpcId: req.requestId } }))
      return { accepted: true }
    },
  }
  const disposers = []
  const provided = {}
  const routes = []
  const sctx = {
    effect: (fn) => { const d = fn(); if (d) disposers.push(d) },
    get: () => ({ stat: async () => true }),
    connection: { fetch: { register: (r) => { routes.push(r); return () => {} } } },
    ...(sessionApi ? { sessionController } : {}),
    workspaceRegistry: {},
  }
  const flushes = []
  const ctx = {
    effect: (fn) => { const d = fn(); if (d) disposers.push(d) },
    on: (name, fn) => { (handlers[name] ??= []).push(fn) },
    inject: (deps, cb) => {
      if (!deps.includes('sessionController') || sessionApi) cb(sctx)
    },
    provide: (name, v) => { provided[name] = v },
    get: () => undefined,
    logger,
    events: { dispatch: () => [] },
    serial: async () => {},
    agents: { get: (id) => (live && id === root ? agent : undefined) },
    // Each flush records how many user bubbles were in the Session then.
    sessions: { flush: async () => { flushes.push(events.filter((e) => e.type === 'user/message').length) } },
  }
  return {
    flushes,
    ctx,
    provided,
    routes,
    agent,
    events,
    prompts,
    handlers,
    idle: async (ms = 10_000) => { for (let i = 0; i < 40; i++) { await new Promise((r) => setTimeout(r, 5)); await within(running, ms, 'a fake DSH turn') } },
    type: (text, mode = 'queue') => {
      const m = llm.createUserMessage({ content: [{ type: 'text', text }], source: { kind: 'user', rpcId: `browser-${text}` } })
      if (mode === 'steer') agent.steer(m)
      else agent.followup(m)
      return m
    },
    bubbles: () => events.filter((e) => e.type === 'user/message' || e.type === 'assistant/message').map((e) => ({ role: e.type.split('/')[0], text: e.data.message ? e.data.message.content.map((c) => c.text).join('') : e.data.content.map((c) => c.text).join(''), rpcId: e.data.source?.rpcId, model: e.data.message?.source?.model })),
    dispose: () => disposers.forEach((d) => typeof d === 'function' && d()),
  }
}
