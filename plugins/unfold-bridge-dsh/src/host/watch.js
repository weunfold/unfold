// The work view: what the DSH this plugin runs in is doing, for Unfold. It follows DSH's own
// host events (session events, agent status, created agents, request errors), keeps one row per
// Session, the approvals still pending in DSH, the drafts Unfold registered for the person to
// tap, and Unfold's own view for the DSH client. Subscriptions decide what Unfold sees: an
// event outside them is never written to Unfold's stream, and an unsubscribed Session's turn
// text never leaves this file. The plugin answers no approval: an approval opens on DSH's
// `approval/asked` and clears on `approval/decided`, and the person answers it on DSH's card.
// Nothing here sends a prompt; `takeDraft` hands the route that sends exactly the text agentd
// registered, once.

import { createHash, timingSafeEqual } from 'node:crypto'

export const KINDS = ['turn_done', 'error', 'approval_waiting', 'status', 'created']
const TEXT_MAX = 500
const DRAFT_MAX = 16 * 1024
const ID = /^[A-Za-z0-9_-]{1,128}$/
const CONTROL_MAX = 64
// Characters that reorder or hide text: bidi embeddings, overrides, isolates and marks (Unicode's
// Bidi_Control set), and the tag block. A draft holding one would show the person something other than what Send delivers.
const HIDING = /[\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]|[\u{E0000}-\u{E007F}]/u

export class Refusal extends Error {
  constructor(status, code, message = code) {
    super(message)
    Object.assign(this, { status, code })
  }
}

const refuse = (status, code, message) => { throw new Refusal(status, code, message) }
const digest = text => createHash('sha256').update(text).digest()
const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value)
const cut = text => (text.length > TEXT_MAX ? `${text.slice(0, TEXT_MAX - 1)}…` : text)
const exactKeys = (body, keys) => isObject(body) && Object.keys(body).length === keys.length && keys.every(key => Object.hasOwn(body, key))
const strings = list => list === undefined || (Array.isArray(list) && list.every(item => typeof item === 'string' && item !== ''))

// A subscription list as the wire carries it; an empty list is valid and matches nothing. Who
// added one (`added_by`) and when (`at`) ride along for the DSH client's badges.
export function parseSubscriptions(list) {
  if (!Array.isArray(list)) refuse(400, 'bad_subscription', 'subscriptions must be an array')
  return list.map(sub => {
    if (!isObject(sub) || typeof sub.id !== 'string' || sub.id === '' || !isObject(sub.match)) refuse(400, 'bad_subscription', 'a subscription needs an id and a match')
    const { session_ids, names, workspaces } = sub.match
    if (![session_ids, names, workspaces].every(strings)) refuse(400, 'bad_subscription', 'match lists hold non-empty strings')
    if (![session_ids, names, workspaces].some(list => list?.length > 0)) refuse(400, 'bad_subscription', 'a subscription matches at least one session, name or workspace')
    if (!Array.isArray(sub.kinds) || sub.kinds.length === 0 || !sub.kinds.every(kind => KINDS.includes(kind))) refuse(400, 'bad_subscription', `kinds are some of ${KINDS.join(', ')}`)
    const out = { id: sub.id, match: { session_ids: session_ids ?? [], names: names ?? [], workspaces: workspaces ?? [] }, kinds: [...sub.kinds] }
    if (sub.added_by === 'person' || sub.added_by === 'unfold') out.added_by = sub.added_by
    if (Number.isFinite(sub.at)) out.at = sub.at
    return out
  })
}

const matchesRow = (sub, row) =>
  sub.match.session_ids.includes(row.session_id) || sub.match.names.includes(row.name) || sub.match.workspaces.includes(row.workspace)
// A Session any subscription watches, for any kind (its turn text may then be read).
export const watched = (subs, row) => subs.some(sub => matchesRow(sub, row))
// The kinds whose events about this Session pass.
export const passes = (subs, row, kind) => subs.some(sub => sub.kinds.includes(kind) && matchesRow(sub, row))

// The text of an assistant message: its text parts only, never tool calls or results.
const textOf = message => (Array.isArray(message?.content) ? message.content.filter(part => part?.type === 'text' && typeof part.text === 'string').map(part => part.text).join('') : '')

// `names` resolves a Session's label: `title(session, sessionId)` (DSH's own title service) and
// `workspace(sessionId, cwd)`; `now` and `newId` are injectable for tests.
export function createWatch({ names = {}, now = Date.now, newId, controlMax = CONTROL_MAX } = {}) {
  const rows = new Map() // session_id -> row
  const titles = new Map() // session_id -> title from its own session/title events
  const turnText = new Map() // session_id -> the last assistant text of the running turn
  const approvals = new Map() // DSH approval id -> {call_id, session_id, tool, reason, at}
  const drafts = new Map() // effect_id -> {token, session_id, text, name, workspace, at}
  const draftOf = new Map() // session_id -> effect_id
  const spent = new Set() // effect ids sent or cancelled, never registered again
  const listeners = { event: new Set(), control: new Set(), change: new Set() }
  const controls = [] // control events waiting for the next stream
  let subs = []
  let conn // {conn_id, generation}
  let connected = false
  let view = null
  let viewGeneration = -1 // views rise strictly, so an older one in flight never wins

  const emit = (name, value) => { for (const fn of Array.from(listeners[name])) fn(value) }
  const changed = () => emit('change')

  const nameOf = (sessionId, session) =>
    titles.get(sessionId) ?? names.title?.(session, sessionId) ?? (rows.get(sessionId)?.name || `session ${sessionId.slice(0, 8)}`)
  const row = (sessionId, session, cwd) => {
    let r = rows.get(sessionId)
    if (!r) {
      r = { session_id: sessionId, name: '', workspace: '', status: 'idle' }
      rows.set(sessionId, r)
    }
    r.name = nameOf(sessionId, session)
    r.workspace = names.workspace?.(sessionId, cwd ?? r.cwd) ?? r.workspace
    if (cwd !== undefined) r.cwd = cwd
    return r
  }

  // One typed event; it reaches Unfold only when a subscription passes it.
  // `also` names a second kind whose subscribers get the event too (an approval's clear goes to
  // whoever saw it open).
  const publish = (r, kind, extra = {}, also) => {
    if (!connected || !(passes(subs, r, kind) || (also !== undefined && passes(subs, r, also)))) return
    emit('event', { kind, session_id: r.session_id, agent_name: r.name, workspace: r.workspace, ...extra, at: now() })
  }

  const watch = {
    on(name, fn) {
      listeners[name].add(fn)
      return () => listeners[name].delete(fn)
    },

    // ---- DSH host hooks
    sessionEvent(session, event) {
      const sessionId = session?.id
      if (typeof sessionId !== 'string' || !isObject(event)) return
      const data = event.data ?? {}
      if (event.type === 'session/title' && typeof data.title === 'string') titles.set(sessionId, data.title)
      const r = row(sessionId, session, typeof session.cwd === 'string' ? session.cwd : undefined)
      switch (event.type) {
        case 'assistant/message': {
          const text = textOf(data.message ?? data)
          if (text) turnText.set(sessionId, text)
          break
        }
        case 'turn/end': {
          const text = turnText.get(sessionId) ?? ''
          turnText.delete(sessionId)
          const failed = data.reason?.kind === 'error'
          r.status = 'idle'
          r.last_turn_at = now()
          if (text && watched(subs, r)) r.last_summary = cut(text)
          else delete r.last_summary
          publish(r, failed ? 'error' : 'turn_done', failed ? { summary: cut(String(data.reason?.error?.code ?? 'error')) } : text ? { summary: cut(text) } : {})
          changed()
          break
        }
        case 'approval/asked': {
          if (typeof data.id !== 'string' || typeof data.callId !== 'string') break
          const approval = { call_id: data.callId, session_id: sessionId, tool: String(data.toolName ?? ''), reason: cut(String(data.reason ?? '')), at: now() }
          approvals.set(data.id, approval)
          publish(r, 'approval_waiting', { approval: { call_id: approval.call_id, tool: approval.tool, reason: approval.reason } })
          changed()
          break
        }
        case 'approval/decided': {
          const approval = approvals.get(data.id)
          if (!approval) break
          approvals.delete(data.id)
          publish(r, 'status', { status: 'approval_decided', approval: { call_id: approval.call_id, tool: approval.tool, reason: approval.reason } }, 'approval_waiting')
          changed()
          break
        }
        default:
      }
    },
    agentStatus(sessionId, status, session) {
      if (typeof sessionId !== 'string') return
      const r = row(sessionId, session)
      const value = typeof status === 'string' ? status : typeof status?.kind === 'string' ? status.kind : undefined
      if (value === undefined || value === r.status) return
      r.status = value
      publish(r, 'status', { status: value })
      changed()
    },
    agentCreated(sessionId, session) {
      if (typeof sessionId !== 'string') return
      publish(row(sessionId, session), 'created')
      changed()
    },
    requestError(sessionId, code, session) {
      if (typeof sessionId !== 'string') return
      publish(row(sessionId, session), 'error', { summary: cut(String(code ?? 'error')) })
    },
    // Sessions DSH lists (sessionController.list items), so a quiet Session has a row too.
    listed(items) {
      for (const item of items ?? []) if (typeof item?.sessionId === 'string') {
        const r = row(item.sessionId, undefined, typeof item.cwd === 'string' ? item.cwd : undefined)
        if (item.running === true && r.status === 'idle') r.status = 'running'
        // A Session quiet since this plugin started still has a last turn, for the first-run suggestion.
        if (Number.isFinite(item.updatedAt) && !(r.last_turn_at >= item.updatedAt)) r.last_turn_at = item.updatedAt
      }
    },

    // ---- agentd
    // A new stream replaces the old connection: its view and drafts go, its control events stay.
    connect(list) {
      const parsed = parseSubscriptions(list)
      conn = { conn_id: newId(), generation: 0 }
      subs = parsed
      view = null
      viewGeneration = -1
      drafts.clear()
      draftOf.clear()
      connected = true
      const backlog = controls.splice(0)
      changed()
      return { conn_id: conn.conn_id, backlog }
    },
    disconnect(connId) {
      if (conn?.conn_id !== connId) return
      connected = false
    },
    // The ordering check every agentd write passes: the current connection, a rising generation.
    current(connId, generation) {
      if (!conn || connId !== conn.conn_id || !Number.isInteger(generation) || generation < conn.generation) refuse(409, 'stale_generation')
      conn.generation = generation
    },
    agents(list) {
      const set = parseSubscriptions(list)
      const agents = [...rows.values()].map(r => {
        const out = { session_id: r.session_id, name: r.name, workspace: r.workspace, status: r.status }
        if (watched(set, r)) {
          if (r.last_summary !== undefined) out.last_summary = r.last_summary
          if (r.last_turn_at !== undefined) out.last_turn_at = r.last_turn_at
        }
        return out
      })
      const pending = [...approvals.values()].filter(a => watched(set, rows.get(a.session_id))).map(a => ({ ...a }))
      return { agents, approvals: pending }
    },
    register(body) {
      if (!exactKeys(body, ['conn_id', 'generation', 'effect_id', 'token', 'session_id', 'text'])) refuse(400, 'body_refused', 'a draft is exactly {conn_id, generation, effect_id, token, session_id, text}')
      const { effect_id, token, session_id, text } = body
      if (typeof effect_id !== 'string' || !ID.test(effect_id)) refuse(400, 'body_refused', 'effect_id is 1-128 of A-Z a-z 0-9 _ -')
      if (typeof token !== 'string' || token.length < 16 || token.length > 256) refuse(400, 'body_refused', 'token is 16-256 characters')
      if (typeof text !== 'string' || text.trim() === '' || text.length > DRAFT_MAX) refuse(400, 'body_refused', `text is 1-${DRAFT_MAX} characters`)
      if (HIDING.test(text)) refuse(400, 'body_refused', 'text holds bidi controls or tag characters')
      watch.current(body.conn_id, body.generation)
      if (spent.has(effect_id) || drafts.has(effect_id)) refuse(409, 'already_registered')
      const r = typeof session_id === 'string' ? rows.get(session_id) : undefined
      if (!r) refuse(404, 'not_found', 'no such session')
      const old = draftOf.get(session_id)
      if (old !== undefined) {
        drafts.delete(old)
        spent.add(old) // a superseded id is never registered again
      }
      drafts.set(effect_id, { token, session_id, text, name: r.name, workspace: r.workspace, at: now() })
      draftOf.set(session_id, effect_id)
      changed()
      return { effect_id, session_id, name: r.name, workspace: r.workspace }
    },
    unregister(effectId) {
      const draft = drafts.get(effectId)
      if (!draft) refuse(404, 'not_found')
      drafts.delete(effectId)
      spent.add(effectId)
      if (draftOf.get(draft.session_id) === effectId) draftOf.delete(draft.session_id)
      changed()
    },
    setView(body) {
      if (!exactKeys(body, ['conn_id', 'generation', 'subscriptions', 'log_tail', 'advice'])) refuse(400, 'body_refused', 'a view is exactly {conn_id, generation, subscriptions, log_tail, advice}')
      if (!Array.isArray(body.log_tail) || !Array.isArray(body.advice)) refuse(400, 'body_refused', 'log_tail and advice are arrays')
      const subscriptions = parseSubscriptions(body.subscriptions)
      const advice = body.advice.map(a => {
        if (!isObject(a) || typeof a.call_id !== 'string' || typeof a.session_id !== 'string' || typeof a.note !== 'string' || typeof a.marked !== 'boolean') refuse(400, 'body_refused', 'advice is {call_id, session_id, note, marked, at}')
        return { call_id: a.call_id, session_id: a.session_id, note: cut(a.note), marked: a.marked, at: Number(a.at) || now() }
      })
      if (conn && body.conn_id === conn.conn_id && body.generation <= viewGeneration) refuse(409, 'stale_generation')
      watch.current(body.conn_id, body.generation)
      viewGeneration = body.generation
      view = { subscriptions, log_tail: body.log_tail.slice(-50), advice }
      changed()
    },

    // ---- the DSH client (admitted taps)
    // Control events for agentd: written now on a live stream, else queued for the next one.
    // `outcome`: the result of a draft already taken (sent, or a failed send). It is recorded past
    // the bound, since the tap was admitted before the send.
    control(event, { outcome = false } = {}) {
      if (connected) {
        emit('control', event)
        return
      }
      if (!outcome && controls.length >= controlMax) refuse(503, 'agentd_unavailable', 'Unfold is not connected')
      controls.push(event)
    },
    // Room for one more control event (a send checks before it sends).
    canControl: () => connected || controls.length < controlMax,
    // The draft a tap names, checked against its token, and taken: it can't be sent twice.
    takeDraft(effectId, token) {
      const draft = drafts.get(effectId)
      if (!draft) refuse(404, 'not_found')
      if (typeof token !== 'string' || !timingSafeEqual(digest(token), digest(draft.token))) refuse(403, 'bad_token')
      drafts.delete(effectId)
      if (draftOf.get(draft.session_id) === effectId) draftOf.delete(draft.session_id)
      spent.add(effectId)
      changed()
      return { session_id: draft.session_id, text: draft.text }
    },
    // What the DSH client shows: every Session's label and status, Unfold's view, the drafts.
    snapshot() {
      return {
        connected,
        agents: [...rows.values()].map(r => ({ session_id: r.session_id, name: r.name, workspace: r.workspace, status: r.status, ...(r.last_turn_at === undefined ? {} : { last_turn_at: r.last_turn_at }) })),
        approvals: [...approvals.values()].map(a => ({ call_id: a.call_id, session_id: a.session_id })),
        view,
        drafts: [...drafts.entries()].map(([effect_id, d]) => ({ effect_id, token: d.token, session_id: d.session_id, name: d.name, workspace: d.workspace, text: d.text, at: d.at })),
      }
    },
  }
  return watch
}
