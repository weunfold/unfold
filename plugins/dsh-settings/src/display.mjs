// What the panel draws, shared by the browser half (client/index.jsx bundles it) and the
// tests. The panel holds a view, and TIME and the stream's health decide what it may still
// claim. The server's state is only the starting point:
//
// - Host values (`basis: 'host'`) expire at their own `expires_at`, the host read plus its
//   lease. A heartbeat never renews them; only a new view with a newer host read does.
// - Event-derived values (`basis: 'events' | 'bot'`) are current only while the server keeps
//   confirming them: the last view or heartbeat plus `stream_lease_ms`. They stop at once when
//   the stream reports an error, and they stay stopped until a new view arrives.
// - A failed manual refresh stops every value until the next view.
// - Working-tree constants (`basis: 'source'`) are not live claims and do not expire.
//
// Ordering: views from GET and SSE share one identity, the serving
// instance plus a content revision. A view older than the newest one held (an older serving, a
// lower revision, or the same revision at an earlier time) is refused, so a late Refresh never
// undoes a newer view. A heartbeat confirms only the view whose serving and revision it names;
// a newer revision means this view is outdated, so it is not confirmed and `needsSnapshot` asks
// for a new one.
//
// Times are compared on the server's clock. The client-minus-server offset is the smallest
// (clientNow - at) seen: network delay can only make that difference larger, so a delayed view
// keeps its real age instead of getting a fresh lease on arrival.
// Stale and unknown are both drawn "unknown": nothing past its evidence is shown as current.

/** A leaf's value as drawn, from its effective state. */
export function shown(leaf, state = leaf.state) {
  if (state === 'stale') return 'unknown (stale)'
  if (state === 'unknown') return 'unknown'
  const v = leaf.value
  if (v === null || v === undefined) return leaf.reported_null ? 'none reported' : 'unknown'
  return typeof v === 'string' ? v : JSON.stringify(v)
}

export class PanelModel {
  constructor() {
    this.view = null
    this.offset = null // client clock minus server clock: the smallest difference seen
    this.needsSnapshot = false // a heartbeat named a newer revision than this view
    this.confirmed = null // server time the event-derived values were last confirmed
    this.stream = 'opening'
    this.lostAt = null // server time the stream was lost, while it stays lost
    this.refreshError = null
  }

  /** Is `view` older than the view held? (Older serving, lower revision, or the same revision earlier.) */
  superseded(view) {
    const cur = this.view
    if (!cur) return false
    if ((view.serving_started_at ?? 0) !== (cur.serving_started_at ?? 0)) return (view.serving_started_at ?? 0) < (cur.serving_started_at ?? 0)
    if (view.serving !== cur.serving) return false
    if ((view.rev ?? 0) !== (cur.rev ?? 0)) return (view.rev ?? 0) < (cur.rev ?? 0)
    return view.at < cur.at
  }

  observe(serverAt, clientNow) {
    const d = clientNow - serverAt
    this.offset = this.offset === null ? d : Math.min(this.offset, d)
  }

  /** A full view, from the stream or from a successful Refresh. Returns false when refused as superseded. */
  setView(view, clientNow) {
    if (this.superseded(view)) return false
    this.observe(view.at, clientNow)
    this.view = view
    this.confirmed = view.at
    if (this.lostAt !== null && view.at > this.lostAt) this.lostAt = null
    // The outstanding target belongs to its serving instance: a view from a
    // newer serving retires it, as does reaching the wanted revision on the same serving.
    if (this.needsSnapshot && (view.serving !== this.wanted?.serving || (view.rev ?? 0) >= this.wanted.rev)) {
      this.needsSnapshot = false
      this.wanted = null
    }
    this.refreshError = null
    return true
  }

  /** Confirms the held view only when the beat names its serving and revision. */
  heartbeat(beat, clientNow) {
    if (!this.view) return false
    if (beat.serving !== this.view.serving || beat.serving_started_at !== this.view.serving_started_at) return false
    this.observe(beat.at, clientNow)
    if (beat.rev !== this.view.rev) {
      if (beat.rev > this.view.rev) {
        this.needsSnapshot = true
        this.wanted = { serving: beat.serving, rev: beat.rev }
      }
      return false
    }
    if (this.lostAt !== null || beat.at < this.view.at) return false
    this.confirmed = Math.max(this.confirmed ?? beat.at, beat.at)
    return true
  }

  streamOpen() {
    this.stream = 'open'
  }

  streamLost(clientNow) {
    this.stream = 'reconnecting'
    if (this.lostAt === null) this.lostAt = clientNow - (this.offset ?? 0)
  }

  refreshFailed(error) {
    this.refreshError = String(error)
  }

  /** The state a leaf may claim now, and why it changed, if it did. */
  effective(leaf, clientNow) {
    const base = leaf.state
    if (base !== 'live') return { state: base, why: null }
    if (this.refreshError) return { state: 'stale', why: `the last refresh failed (${this.refreshError}); nothing is confirmed until the next view` }
    const now = clientNow - (this.offset ?? 0)
    if (leaf.basis === 'host') {
      if (leaf.expires_at == null || now > leaf.expires_at) return { state: 'stale', why: 'the host read is older than its lease (expired in this panel)' }
      return { state: 'live', why: null }
    }
    if (leaf.basis === 'events' || leaf.basis === 'bot') {
      if (this.lostAt !== null) return { state: 'stale', why: 'the settings stream was lost: agent facts are not being confirmed' }
      if (this.needsSnapshot) return { state: 'stale', why: 'the server has newer settings than this view: waiting for the new view' }
      const lease = this.view?.stream_lease_ms ?? 0
      if (this.confirmed === null || now > this.confirmed + lease) {
        return { state: 'stale', why: `no word from the settings stream for over ${Math.round(lease / 1000)} s: agent facts are not being confirmed` }
      }
    }
    return { state: 'live', why: null }
  }

  /**
   * Client time of the next state change with no input, or null: when to draw again. Only
   * expiries still ahead count; one already past changes nothing more (no redraw loop).
   */
  nextChange(clientNow) {
    if (!this.view || this.refreshError) return null
    let at = Infinity
    const ahead = (t) => {
      if (t > clientNow) at = Math.min(at, t)
    }
    const consider = (leaf) => {
      if (leaf.state !== 'live') return
      if (leaf.basis === 'host' && leaf.expires_at != null) ahead(leaf.expires_at + 1 + (this.offset ?? 0))
      if ((leaf.basis === 'events' || leaf.basis === 'bot') && this.lostAt === null && this.confirmed !== null) {
        ahead(this.confirmed + (this.view.stream_lease_ms ?? 0) + 1 + (this.offset ?? 0))
      }
    }
    for (const s of this.view.sections) s.leaves.forEach(consider)
    Object.values(this.view.revisions).forEach(consider)
    return at === Infinity ? null : at
  }

  /** Everything the panel draws, with effective states applied. */
  render(clientNow) {
    if (!this.view) return null
    const draw = (leaf) => {
      const e = this.effective(leaf, clientNow)
      return { leaf, state: e.state, text: shown(leaf, e.state), note: e.why ?? leaf.note ?? null }
    }
    return {
      stream: this.stream,
      refreshError: this.refreshError,
      revisions: Object.entries(this.view.revisions).map(([key, r]) => ({ key, ...draw(r) })),
      sections: this.view.sections.map((s) => ({ id: s.id, title: s.title, leaves: s.leaves.map(draw) })),
      sources: this.view.sources,
      edits: this.view.edits,
      gaps: this.view.gaps,
    }
  }
}

/** A rendered panel as the plain lines it shows, in order. */
export function lines(drawn) {
  const out = []
  for (const r of drawn.revisions) out.push(`revision ${r.key}: ${r.text} [${r.state}]`)
  for (const s of drawn.sections) {
    out.push(`# ${s.title}`)
    for (const l of s.leaves) out.push(`${l.leaf.label}: ${l.text} [${l.state}] (${l.leaf.source})${l.note ? ` ${l.note}` : ''}`)
  }
  for (const g of drawn.gaps) out.push(`gap ${g.what}: ${g.detail}`)
  return out
}

/** The server's view drawn as-is, at its own time (for tests and logs). */
export function viewLines(view) {
  const m = new PanelModel()
  m.setView(view, view.at)
  m.streamOpen()
  return lines(m.render(view.at))
}
