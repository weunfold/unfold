// Aggregates for the pane header, as a DSH session-projection unit. The unit is defined here
// but nothing drives it yet: no session adapter writes RTH envelopes into a DSH Session, so
// it only folds once an adapter appends { type: 'rth/event', data: envelope } events.
// The registry calls schema.parse on persisted state and wire views, so both are Zod
// (the contract dsh-session-projection 0.2.0-rc.2 is built on); plugin Config stays Schemastery.
import { z } from 'zod'
import { audioOf } from './window.mjs'

export const PROJECTION_KEY = 'rth-events'
export const RTH_EVENT_TYPE = 'rth/event'
export const STATE_VERSION = 2
export const OPEN_MAX = 64 // replies still open; older ones are counted, not kept
export const UNKNOWN_MAX = 32 // distinct unknown kinds named; the rest count as one bucket
const UNKNOWN_OVERFLOW = '(other unknown kinds)'

const count = z.number().int().nonnegative()
const counts = z.record(z.string(), count)

export const StateSchema = z.object({
  asOf: z.string().nullable(),
  events: count,
  phases: counts,
  names: counts,
  open: z.record(z.string(), z.string().nullable()),
  openEvicted: count,
  finished: counts,
  lastDecision: z.object({
    decision_id: z.string().nullable(),
    selected: z.string().nullable(),
    policy: z.string().nullable(),
    latency_ns: z.number().nullable(),
  }).nullable(),
  audio: z.object({ chunks: count, ms: z.number().nonnegative() }),
  unknown: z.array(z.string()).max(UNKNOWN_MAX),
  saveFailures: count,
})

export const ViewSchema = StateSchema

export function initialState() {
  return {
    asOf: null,
    events: 0,
    phases: {},
    names: {},
    open: {},
    openEvicted: 0,
    finished: { completed: 0, stopped: 0, failed: 0 },
    lastDecision: null,
    audio: { chunks: 0, ms: 0 },
    unknown: [],
    saveFailures: 0,
  }
}

/** One envelope folded into the aggregates. Pure: `state` is never mutated. */
export function fold(state, env, catalog) {
  const entry = catalog.entry(env.kind)
  if (entry?.ephemeral) return state
  const d = catalog.describe(env)
  const next = { ...state, asOf: typeof d.id === 'string' ? d.id : null, events: state.events + 1, phases: bump(state.phases, d.phase) }
  let name = d.name
  if (!d.known && !state.unknown.includes(d.wire)) {
    if (state.unknown.length < UNKNOWN_MAX) next.unknown = [...state.unknown, d.wire]
    else name = UNKNOWN_OVERFLOW
  }
  next.names = bump(state.names, name)
  const f = d.fields
  const str = (x) => (typeof x === 'string' ? x : null)
  switch (d.name) {
    case 'message_started': {
      if (!str(f.response_id)) break
      const open = { ...state.open, [f.response_id]: str(f.request_id) }
      const keys = Object.keys(open)
      for (const k of keys.slice(0, Math.max(0, keys.length - OPEN_MAX))) delete open[k]
      next.openEvicted = state.openEvicted + Math.max(0, keys.length - OPEN_MAX)
      next.open = open
      break
    }
    case 'message_finished': {
      const { [f.response_id]: _, ...rest } = state.open
      next.open = rest
      const result = str(entry.values?.status?.[f.status] ?? f.status) ?? 'unknown'
      next.finished = { ...state.finished, [result]: (state.finished[result] ?? 0) + 1 }
      break
    }
    case 'decision_made':
      next.lastDecision = { decision_id: str(f.decision_id), selected: str(f.selected), policy: str(f.policy), latency_ns: Number.isFinite(f.latency_ns) ? f.latency_ns : null }
      break
    case 'message_audio_added':
      next.audio = { chunks: state.audio.chunks + 1, ms: state.audio.ms + audioOf(f).ms }
      break
    case 'history_save_failed':
      next.saveFailures = state.saveFailures + 1
      break
  }
  return next
}

/** The unit for ctx.sessionProjections.register (dsh-session-projection 0.2.0-rc.2). */
export function projectionDefinition({ catalog }) {
  return {
    key: PROJECTION_KEY,
    stateSchema: StateSchema,
    stateVersion: STATE_VERSION,
    init: () => initialState(),
    apply: (state, event) => (event?.type === RTH_EVENT_TYPE && event.data ? fold(state, event.data, catalog) : state),
    wire: { viewSchema: ViewSchema, view: (state) => state },
  }
}

const bump = (counts, key) => ({ ...counts, [key]: (counts[key] ?? 0) + 1 })
