// What the slots show, derived from the bridge's snapshot: `{connected, agents, approvals,
// view, drafts}` (watch.snapshot() in src/host/watch.js). `view` is Unfold's own part
// (subscriptions, the change log tail, advice) and is null until Unfold first sends it.

export const subscriptionsOf = snap => snap?.view?.subscriptions ?? []

const matchOf = sub => sub?.match ?? {}
// A match list as given, or empty: a malformed entry in Unfold's view must not take a slot down.
const listIn = value => (Array.isArray(value) ? value.filter(item => typeof item === 'string') : [])
const has = (list, value) => listIn(list).includes(value)
export const matchesAgent = (sub, agent) => {
  const match = matchOf(sub)
  return has(match.session_ids, agent.session_id) || has(match.names, agent.name) || has(match.workspaces, agent.workspace)
}
export const watchedAgent = (snap, agent) => subscriptionsOf(snap).some(sub => matchesAgent(sub, agent))

// The row toggle's own subscription: one that names this Session by id, preferring the one the
// toggle itself made (that id alone). Subscriptions by name or workspace are not the row's.
export function rowSubscription(snap, sessionId) {
  const own = subscriptionsOf(snap).filter(sub => has(matchOf(sub).session_ids, sessionId))
  const alone = sub => listIn(matchOf(sub).session_ids).length === 1 && listIn(matchOf(sub).names).length === 0 && listIn(matchOf(sub).workspaces).length === 0
  return own.find(alone) ?? own[0] ?? null
}

export const agentOf = (snap, sessionId) => snap?.agents?.find(agent => agent.session_id === sessionId) ?? null

// The draft held for this Session. The bridge keeps one per Session (a new one replaces the
// old), so the newest wins should two ever arrive together.
export function draftFor(snap, sessionId) {
  const own = (snap?.drafts ?? []).filter(draft => draft.session_id === sessionId)
  return own.reduce((newest, draft) => (newest === null || draft.at >= newest.at ? draft : newest), null)
}

export const pendingFor = (snap, sessionId) => (snap?.approvals ?? []).filter(approval => approval.session_id === sessionId)

// Unfold's advice for one pending approval: only for that call, in that Session.
export function adviceFor(snap, approval) {
  const notes = (snap?.view?.advice ?? []).filter(advice => advice.call_id === approval.call_id && advice.session_id === approval.session_id)
  return notes.reduce((newest, advice) => (newest === null || advice.at >= newest.at ? advice : newest), null)
}

// The first-run suggestion: with nothing watched yet, the Session with the most recent turn.
export function suggestion(snap) {
  if (!snap || subscriptionsOf(snap).length > 0) return null
  return (snap.agents ?? []).reduce((best, agent) =>
    (Number.isFinite(agent.last_turn_at) && (best === null || agent.last_turn_at > best.last_turn_at) ? agent : best), null)
}

const listOf = value => (Array.isArray(value) ? value : value ? [value] : [])
export const changesOf = snap => listOf(snap?.view?.log_tail).filter(entry => entry && typeof entry.change_id === 'string').slice().reverse()
// Unfold's own changes, newest first: the board's notices.
export const noticesOf = snap => changesOf(snap).filter(entry => entry.by === 'unfold')

// A subscription in words: the Sessions it names by id (shown by their labels), the names and
// workspaces it matches, and the event kinds it carries.
export function describeSubscription(sub, snap, t) {
  const match = matchOf(sub)
  const label = id => {
    const agent = agentOf(snap, id)
    return agent ? `${agent.name} · ${agent.workspace}` : id
  }
  const parts = []
  const sessions = listIn(match.session_ids)
  const names = listIn(match.names)
  const workspaces = listIn(match.workspaces)
  if (sessions.length > 0) parts.push(t('match.sessions', { list: sessions.map(label).join(', ') }))
  if (names.length > 0) parts.push(t('match.names', { list: names.join(', ') }))
  if (workspaces.length > 0) parts.push(t('match.workspaces', { list: workspaces.join(', ') }))
  const kinds = listIn(sub?.kinds)
  const what = parts.join(', ')
  return kinds.length > 0 ? `${what} ${t('match.kinds', { list: kinds.map(kind => t(`kind.${kind}`)).join(', ') })}` : what
}

// Who made a change. agentd sends `cause` as "Person" when Unfold acted on the person's own
// turn and "Event" when an agent's event led to it.
export const changedBy = entry => (entry.by !== 'unfold' ? 'log.byYou' : entry.cause === 'Person' ? 'log.byUnfoldAsked' : 'log.byUnfold')

// One change log entry in words: who made it, and what it added and removed.
export function describeChange(entry, snap, t) {
  const what = list => listOf(list).map(sub => describeSubscription(sub, snap, t)).filter(Boolean).join(' | ')
  const changes = []
  const added = what(entry.added)
  const removed = what(entry.removed)
  if (added) changes.push(t('log.added', { what: added }))
  if (removed) changes.push(t('log.removed', { what: removed }))
  return t('log.line', { who: t(changedBy(entry)), change: changes.join('; ') })
}

export const statusText = (status, t) =>
  (['idle', 'running', 'error', 'approval_decided'].includes(status) ? t(`status.${status}`) : t('status.other', { status: String(status ?? '') }))
