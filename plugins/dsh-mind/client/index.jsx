// @rth/dsh-mind, browser half: the "Mind" right-sidebar tab inside DSH's web client, by the same
// two-stage path as @rth/dsh-events (a type in ctx.sidebarRightTabs, body and title in the keyed
// `sidebar.right.pane.tab` seats). Extension priority, its own kind and id, no resource patterns.
//
// The surface is a few plain moments (src/moments.mjs): heard on the right, replies on the left,
// small things as quiet lines; a moment opens to its steps, a step to its audit record.
//
// Data arrives by push: one EventSource on /api/rth-mind/stream per mounted body (a snapshot,
// then increments). The panel expires its own claims (src/display.mjs): a stream silent past
// three beats, or lost, is not live, and an agent heartbeat past its lease is stale, with no new
// frame needed to say so. The only write is a suggestion, which goes to a local append-only log.
// There is no forget, clear, reset or delete control anywhere.
import { Fragment, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { BASE_CSS, Dot, Fold, ago, guarded, useNow, useRemembered, useStyle } from '../../rth-ui/ui.jsx'
import { auditLoader, freshness } from '../src/display.mjs'
import { REPLY_FRESH_MS, captionOf, gapsIn, moments, nowLine } from '../src/moments.mjs'
import { fmtSec, modelFor, replyCaption, replyMetrics, sessionSummary, usageReports } from '../src/metrics.mjs'

export const RTH_MIND_ID = '@rth/dsh-mind'
export const RTH_MIND_KIND = 'rth.mind'
const BASE = 'api/rth-mind'
const SHOWN = 300

const CATS = {
  perceive: { icon: '👂', label: 'perceive' },
  decide: { icon: '⚖️', label: 'decide' },
  act: { icon: '⚙️', label: 'act' },
  speak: { icon: '💬', label: 'speak' },
  remember: { icon: '🗄️', label: 'remember' },
  system: { icon: '🔧', label: 'system' },
}

export const inject = ['slots', 'sidebarRightTabs', 'sidebarRight']

export function apply(ctx) {
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: RTH_MIND_ID,
    kind: RTH_MIND_KIND,
    priority: 'extension',
    title: () => 'Mind',
    guide: [{ id: 'rth-mind', order: 92, title: () => 'Mind', description: () => 'What Unfold hears, says and does, in plain words' }],
  }), 'rth-mind: tab type')
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== RTH_MIND_KIND) continue
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId)
      } catch {}
    }
  }, 'rth-mind: close open tabs on unload')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register({ name: 'sidebar.right.pane.tab', key: RTH_MIND_ID }, guarded('rth-mind', MindBody))), 'rth-mind: tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register({ name: 'sidebar.right.pane.tab.title', key: RTH_MIND_ID }, MindTitle)), 'rth-mind: tab title')
}

function MindTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-rth="mind-title">{tab.title}</span>
}

// ---------------------------------------------------------------- stream state

const initial = { rows: [], episodes: {}, link: null, sources: null, heartbeatMs: 5000, offsetMs: 0, lastStreamAt: null, stream: 'opening', reconnects: 0, marks: [], suggestions: [], dropped: 0 }

function reduce(s, { event, data, at }) {
  switch (event) {
    case 'open':
      return { ...s, stream: 'open', lastStreamAt: at }
    case 'lost': {
      const mark = { id: `local-${at}`, lane: 'mind', source: 'UI', kind: 'panel_stream_lost', category: 'system', status: 'gap', observed_at: at + s.offsetMs, sentence: `This panel's stream ${data}. It reconnects and reloads the server's snapshot; rows the server evicted meanwhile are not shown.`, clock: { basis: 'browser clock' }, local: true }
      return s.stream === 'open' ? { ...s, stream: 'lost', marks: [...s.marks, mark].slice(-20) } : { ...s, stream: 'lost' }
    }
    case 'snapshot':
      return {
        ...s,
        rows: data.rows,
        episodes: Object.fromEntries(data.episodes.map((e) => [e.id, e])),
        link: data.link,
        sources: data.sources,
        heartbeatMs: data.heartbeat_ms ?? s.heartbeatMs,
        offsetMs: data.at - at,
        lastStreamAt: at,
        stream: 'open',
        reconnects: s.lastStreamAt == null ? s.reconnects : s.reconnects + 1,
        suggestionKeys: data.suggestions?.keys ?? [],
        dropped: data.dropped,
      }
    case 'row':
      return { ...s, rows: [...s.rows, data], lastStreamAt: at }
    case 'row-updated':
      return { ...s, rows: s.rows.map((r) => (r.id === data.id ? data : r)), lastStreamAt: at }
    case 'evicted':
      return { ...s, rows: s.rows.filter((r) => r.id !== data.id), dropped: s.dropped + 1, lastStreamAt: at }
    case 'episode':
      return { ...s, episodes: { ...s.episodes, [data.id]: data }, lastStreamAt: at }
    case 'episode-removed': {
      // Every row of it was evicted on the server: the card goes too.
      const { [data.id]: _, ...rest } = s.episodes
      return { ...s, episodes: rest, lastStreamAt: at }
    }
    case 'merged': {
      const { [data.from]: _, ...rest } = s.episodes
      return { ...s, episodes: rest, rows: s.rows.map((r) => (r.episode === data.from ? { ...r, episode: data.into } : r)), lastStreamAt: at }
    }
    case 'link':
      return { ...s, link: data, lastStreamAt: at }
    case 'heartbeat':
      return { ...s, link: s.link ? { ...s.link, heartbeat: { ...s.link.heartbeat, at: data.at, stale: false } } : s.link, lastStreamAt: at }
    case 'heartbeat-mind':
      return { ...s, link: data.link ?? s.link, offsetMs: data.at - at, lastStreamAt: at }
    default:
      return s
  }
}

/** One stream per mounted body, leased: silence past three beats closes it and opens a fresh one. */
function useMindStream() {
  const [state, dispatch] = useReducer(reduce, initial)
  const beatMs = useRef(5000)
  beatMs.current = state.heartbeatMs
  useEffect(() => {
    let source = null
    let lease = null
    let retry = null
    let backoff = 1000
    let closed = false
    const alive = () => {
      clearTimeout(lease)
      lease = setTimeout(() => {
        dispatch({ event: 'lost', data: 'went silent past its lease', at: Date.now() })
        source?.close()
        retry = setTimeout(open, backoff)
        backoff = Math.min(backoff * 2, 8000)
      }, beatMs.current * 3 + 500)
    }
    const names = ['snapshot', 'row', 'row-updated', 'evicted', 'episode', 'episode-removed', 'merged', 'link', 'heartbeat', 'heartbeat-mind']
    function open() {
      if (closed) return
      source = new EventSource(new URL(`${BASE}/stream`, document.baseURI))
      for (const name of names) {
        source.addEventListener(name, (e) => {
          alive()
          if (name === 'snapshot') backoff = 1000
          dispatch({ event: name, data: JSON.parse(e.data), at: Date.now() })
        })
      }
      source.addEventListener('resnapshot', () => dispatch({ event: 'lost', data: 'was dropped by the server (slow reader)', at: Date.now() }))
      source.onopen = () => alive()
      source.onerror = () => dispatch({ event: 'lost', data: 'errored', at: Date.now() })
    }
    open()
    return () => {
      closed = true
      clearTimeout(lease)
      clearTimeout(retry)
      source?.close()
    }
  }, [])
  return state
}

/** Redraws when the freshness rule says the displayed state can next change. */
function useFreshness(state) {
  const [now, setNow] = useState(Date.now())
  const f = freshness({ stream: state.stream, lastStreamAt: state.lastStreamAt, heartbeatMs: state.heartbeatMs, offsetMs: state.offsetMs, link: state.link }, now)
  useEffect(() => {
    if (f.nextChange == null) return
    const t = setTimeout(() => setNow(Date.now()), Math.max(0, f.nextChange - Date.now()))
    return () => clearTimeout(t)
  }, [f.nextChange, state.lastStreamAt])
  useEffect(() => setNow(Date.now()), [state.lastStreamAt, state.stream])
  return f
}

// ---------------------------------------------------------------- view
// Styles for the technical layers (steps, audit, suggestion), on the shared tokens.
const muted = { color: 'var(--rth-muted)' }
const box = { border: '1px solid var(--rth-line)', borderRadius: 10, padding: '8px 10px', background: 'var(--rth-soft)' }
const chipBase = { fontSize: 10.5, borderRadius: 9, padding: '0 7px', whiteSpace: 'nowrap', lineHeight: '17px', color: 'var(--rth-muted)' }
const button = { font: 'inherit', fontSize: 12, padding: '3px 10px', borderRadius: 7, border: '1px solid var(--rth-line)', background: 'transparent', color: 'inherit', cursor: 'pointer' }
const pad2 = (n, w = 2) => String(n).padStart(w, '0')
const hms = (ms) => {
  if (!Number.isFinite(ms)) return 'unknown'
  const d = new Date(ms)
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}.${pad2(d.getMilliseconds(), 3)}`
}
const dur = (ms) => (ms == null ? null : ms < 1 ? `${ms.toFixed(2)} ms` : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`)
const STATUS_WORD = { observed: 'seen', pending: 'open', attempted: 'sent', completed: 'done', stopped: 'stopped', failed: 'failed', unknown: 'not sure', gap: 'gap' }

function Chip({ status, children }) {
  return <span data-rth="mind-status" data-status={status} className="rth-chip" style={chipBase}>{children ?? STATUS_WORD[status] ?? status}</span>
}

function producerTime(row) {
  const c = row.clock ?? {}
  if (Number.isFinite(c.wall_ms_estimate)) return { text: `≈${hms(c.wall_ms_estimate)} agent`, title: c.wall_basis }
  if (Number.isFinite(c.t_ms)) return { text: `host +${(c.t_ms / 1000).toFixed(3)} s`, title: c.basis }
  if (Number.isFinite(c.t_ns)) return { text: `agent +${(c.t_ns / 1e9).toFixed(3)} s`, title: `${c.basis}; ${c.wall_basis}` }
  return null
}

const MIND_CSS = `
.rth-mind{display:flex;flex-direction:column;height:100%;overflow:hidden;position:relative}
.rth-now{display:flex;align-items:center;gap:9px;padding:14px 16px 12px;border-bottom:1px solid var(--rth-line)}
.rth-now-text{font-size:14px;font-weight:600;letter-spacing:-.005em}
.rth-now[data-state=unknown] .rth-now-text{color:var(--rth-muted);font-weight:500}
.rth-now-meta{margin-left:auto;font-size:11.5px;color:var(--rth-faint)}
/* Plain block flow (no flex column): nothing can be squeezed into its neighbour (WebKit can
   shrink flex items until the captions overlap). */
.rth-feed{flex:1 1 auto;overflow:auto;padding:14px 14px 18px;display:block;transition:opacity .4s}
.rth-feed[data-live=false]{opacity:.6}
.rth-sum-line{padding:6px 16px 7px;font-size:11.5px;color:var(--rth-faint);border-bottom:1px solid var(--rth-line)}
.rth-break{display:grid;grid-template-columns:auto 1fr;gap:3px 12px;margin:0 0 8px;font-size:11.5px}
.rth-break dt{color:var(--rth-faint)}
.rth-break dd{margin:0}
.rth-empty{margin:auto;max-width:240px;text-align:center;color:var(--rth-faint);font-size:12.5px}
.rth-m{box-sizing:border-box;display:block;cursor:pointer;border-radius:12px;max-width:100%;margin:6px 0;padding:0;background:none;border:0;color:inherit;font:inherit;text-align:start}
.rth-m:focus-visible{outline:2px solid var(--rth-accent);outline-offset:2px}
.rth-m-heard{display:flex;flex-direction:column;align-items:flex-end;width:fit-content;max-width:86%;margin:12px 0 6px auto}
.rth-m-heard .rth-bubble{padding:7px 12px;border-radius:16px 16px 5px 16px;background:color-mix(in oklab,var(--rth-accent) 13%,transparent);color:var(--rth-fg)}
.rth-m-heard[data-final=false] .rth-bubble{background:color-mix(in oklab,var(--rth-accent) 7%,transparent);color:var(--rth-muted)}
.rth-mind .rth-m-heard .rth-cap{text-align:right}
.rth-m-reply{width:fit-content;max-width:92%;margin:6px auto 10px 0;padding:2px}
.rth-m-reply .rth-said{font-size:13.5px;line-height:1.5}
.rth-m-reply[data-open=true] .rth-said{color:var(--rth-muted)}
.rth-mind .rth-cap{margin-top:3px;font-size:11px;color:var(--rth-faint)}
.rth-mind .rth-m:hover .rth-cap{color:var(--rth-muted)}
.rth-m-line{width:fit-content;max-width:94%;margin:6px auto;padding:1px 8px;text-align:center;font-size:11.5px;color:var(--rth-faint)}
.rth-m-line[data-tone=warn]{color:var(--rth-warn)}
.rth-m-line[data-tone=gap]{color:color-mix(in oklab,var(--rth-warn) 55%,var(--rth-faint))}
.rth-m-line[data-tone=bad]{color:var(--rth-bad)}
.rth-working{display:inline-flex;gap:3px;margin-left:2px;vertical-align:middle}
.rth-working i{width:4px;height:4px;border-radius:50%;background:currentColor;opacity:.35;animation:rth-blink 1.2s infinite}
.rth-working i:nth-child(2){animation-delay:.2s}.rth-working i:nth-child(3){animation-delay:.4s}
@keyframes rth-blink{0%,100%{opacity:.25}40%{opacity:.9}}
.rth-steps{margin:6px 0 4px;padding:8px 10px;border-radius:10px;border:1px solid var(--rth-line);background:var(--rth-soft);font-size:12px;cursor:default;text-align:left;align-self:stretch}
.rth-steps ol{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:2px}
.rth-step{all:unset;box-sizing:border-box;display:flex;gap:8px;width:100%;padding:3px 4px;border-radius:6px;cursor:pointer;align-items:baseline}
.rth-step:hover{background:var(--rth-hover)}
.rth-step:focus-visible{outline:2px solid var(--rth-accent)}
.rth-step-t{flex:none;font-variant-numeric:tabular-nums;color:var(--rth-faint);font-size:11px;min-width:58px}
.rth-step-s{flex:1;min-width:0;overflow-wrap:anywhere}
.rth-steps-foot{display:flex;gap:10px;margin-top:6px;flex-wrap:wrap}
.rth-link{all:unset;cursor:pointer;color:var(--rth-accent);font-size:11.5px}
.rth-link:focus-visible{outline:2px solid var(--rth-accent)}
.rth-jump{position:absolute;left:50%;bottom:58px;transform:translateX(-50%);border:1px solid var(--rth-line);border-radius:14px;padding:4px 12px;font-size:12px;background:light-dark(#fff,#0f2129);color:var(--rth-fg);cursor:pointer;box-shadow:0 6px 18px -8px rgba(0,0,0,.4)}
.rth-mind-foot{padding:0 14px;border-top:1px solid var(--rth-line);max-height:55%;overflow:auto}
.rth-mind-foot>.rth-fold{border-top:0}
.rth-src{list-style:none;margin:0 0 8px;padding:0;display:flex;flex-direction:column;gap:3px}
.rth-src li{display:flex;gap:8px;align-items:center}
.rth-input{font:inherit;font-size:12px;padding:5px 9px;border-radius:7px;border:1px solid var(--rth-line);background:transparent;color:inherit;width:100%}
.rth-all{list-style:none;margin:8px 0 0;padding:0;display:flex;flex-direction:column;gap:3px}
.rth-chip{border:1px solid var(--rth-line)}
.rth-chip[data-status=failed]{color:var(--rth-bad);border-color:color-mix(in oklab,var(--rth-bad) 40%,transparent)}
.rth-chip[data-status=pending],.rth-chip[data-status=attempted]{color:var(--rth-accent)}
.rth-chip[data-status=unknown],.rth-chip[data-status=gap]{color:var(--rth-warn);border-style:dashed}
`

const Working = () => <span className="rth-working" aria-hidden="true"><i /><i /><i /></span>

function MindBody() {
  useStyle('rth-ui-base', BASE_CSS)
  useStyle('rth-ui-mind', MIND_CSS)
  const state = useMindStream()
  const fresh = useFreshness(state)
  const now = useNow(5000)
  const live = fresh.stream.state === 'live' && fresh.agent.state === 'live'
  const [open, setOpen] = useState(() => new Set())
  const [audit, setAudit] = useState(null)
  const list = useRef(null)
  const stick = useRef(true)
  const [unseen, setUnseen] = useState(0)

  const all = useMemo(() => [...state.rows, ...state.marks].sort((a, b) => a.observed_at - b.observed_at), [state.rows, state.marks])
  const byIdAll = useMemo(() => new Map(all.map((r) => [r.id, r])), [all])
  const items = useMemo(() => moments(all, state.episodes, { live }).map((m) => (m.type === 'reply' && !m.open ? { ...m, metrics: replyMetrics(m, byIdAll), model: modelFor(m, all, byIdAll) } : m)), [all, state.episodes, live, byIdAll])
  const summary = useMemo(() => sessionSummary(items, all), [items, all])
  const serverNow = Date.now() + state.offsetMs
  const head = nowLine(items, fresh, serverNow)
  const byId = useMemo(() => new Map(all.map((r) => [r.id, r])), [all])

  // Follow the newest moment like a chat, unless the reader has scrolled up to read.
  const count = items.length
  useEffect(() => {
    const el = list.current
    if (!el) return
    if (stick.current) {
      el.scrollTop = el.scrollHeight
      setUnseen(0)
    } else setUnseen((n) => n + 1)
  }, [count, items.at(-1)?.text])
  const onScroll = () => {
    const el = list.current
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40
    if (stick.current) setUnseen(0)
  }
  const jump = () => {
    stick.current = true
    list.current.scrollTop = list.current.scrollHeight
    setUnseen(0)
  }
  const toggle = (key) => setOpen((s) => {
    const n = new Set(s)
    n.has(key) ? n.delete(key) : n.add(key)
    return n
  })

  return (
    <div className="rth-ui rth-mind" data-rth="mind">
      <header className="rth-now" data-state={head.state} data-rth="mind-now">
        <Dot tone={head.state === 'unknown' ? 'neutral' : head.state === 'busy' ? 'accent' : 'ok'} pulse={Boolean(head.pulse)} />
        <span className="rth-now-text" data-rth="mind-agent" data-state={fresh.agent.state} role="status">{head.text}{head.pulse ? <Working /> : null}</span>
        <span className="rth-now-meta rth-num" data-rth="mind-stream" data-state={fresh.stream.state}>
          {head.stale && head.since ? `since ${ago(head.since, serverNow)}` : head.state === 'unknown' ? (state.lastStreamAt ? `last update ${ago(state.lastStreamAt, now)}` : '') : head.since ? `last reply ${ago(head.since, serverNow)}` : ''}
        </span>
      </header>
      {summary.replies ? (
        <div className="rth-sum-line rth-num" data-rth="mind-summary" title="In what this panel still holds (not a durable session total). Tokens and cost need per-reply usage from the agent.">
          {[`${summary.replies} repl${summary.replies > 1 ? 'ies' : 'y'}`, summary.avgDurationMs != null ? `avg reply ${fmtSec(summary.avgDurationMs)} (${summary.timed} of ${summary.completed} completed timed)` : null, 'tokens and cost: not linked yet'].filter(Boolean).join(' · ')}
        </div>
      ) : null}
      <div className="rth-feed" ref={list} onScroll={onScroll} data-live={live} data-rth="mind-rows">
        {items.length ? items.map((m) => (
          <Moment key={m.key} m={m} open={open.has(m.key)} onToggle={() => toggle(m.key)} rows={m.rows.map((id) => byId.get(id)).filter(Boolean)} episode={state.episodes[m.episode]} audit={audit} onAudit={setAudit} live={live} now={serverNow} />
        )) : <div className="rth-empty">Nothing yet. When Unfold hears something, replies or notes something, it shows up here.</div>}
      </div>
      {unseen > 0 ? <button type="button" className="rth-jump" onClick={jump}>↓ {unseen} new</button> : null}
      <footer className="rth-mind-foot">
        <Fold id="mind:more" summary="More">
          <Sources sources={state.sources} fresh={fresh} reconnects={state.reconnects} gaps={gapsIn(all)} usage={usageReports(all).length} />
          <Fold id="mind:every-event" summary="Every event (technical)">
            <EveryEvent rows={all} episodes={state.episodes} dropped={state.dropped} audit={audit} onAudit={setAudit} />
          </Fold>
        </Fold>
      </footer>
    </div>
  )
}

/** One moment: a plain line; a click shows what happened (steps), a step shows its audit record. */
function Moment({ m, open, onToggle, rows, episode, audit, onAudit, live, now }) {
  const going = m.open && now - (m.last_at ?? m.at) < REPLY_FRESH_MS
  const cls = m.type === 'heard' ? 'rth-m rth-m-heard' : m.type === 'reply' ? 'rth-m rth-m-reply' : 'rth-m rth-m-line'
  const body = m.type === 'heard'
    ? <><div className="rth-bubble">{m.text}</div><div className="rth-cap">{captionOf(m, now)}</div></>
    : m.type === 'reply'
      ? <><div className="rth-said">{m.text ?? (m.open || m.uncertain ? 'A reply started' : 'A reply (no text seen)')}{going ? <Working /> : null}</div><div className="rth-cap" data-rth="mind-metrics">{replyCaption(m, now)}</div></>
      : <>{m.text}{m.count > 1 ? ` ×${m.count}` : ''}</>
  return (
    <>
      <div
        role="button"
        tabIndex={0}
        className={cls}
        aria-expanded={open}
        data-rth="mind-moment"
        data-type={m.type}
        data-final={m.final === undefined ? undefined : String(m.final)}
        data-open={String(Boolean(m.open))}
        data-stale={m.uncertain && !going ? 'true' : undefined}
        data-tone={m.tone}
        data-note={m.note ? 'true' : undefined}
        data-episode={m.episode ?? ''}
        onClick={onToggle}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle() } }}
      >
        {body}
      </div>
      {open ? <Steps m={m} rows={rows} episode={episode} audit={audit} onAudit={onAudit} live={live} /> : null}
    </>
  )
}

/** The measured breakdown of one reply: each span with its endpoints and clock, or why it is missing. */
function Breakdown({ met, model }) {
  const clockName = { host: 'rth-host clock', agent: 'agent clock' }
  return (
    <dl className="rth-break" data-rth="mind-breakdown">
      {met.why ? <><dt>Timing</dt><dd>not available: {met.why}</dd></> : null}
      {met.spans.map((sp) => (
        <Fragment key={sp.name}>
          <dt>{sp.name[0].toUpperCase() + sp.name.slice(1)}</dt>
          <dd>{sp.ms != null ? `${fmtSec(sp.ms)} · ${sp.start.kind} → ${sp.end.kind} (${clockName[sp.clock]})` : `not available: ${sp.why}`}</dd>
        </Fragment>
      ))}
      {!met.why ? <><dt>Playback</dt><dd>{met.playback === 'reported' ? 'reported by rth-host (handed to the device; not proof it was heard)' : 'not reported'}</dd></> : null}
      <dt>Status</dt><dd>{met.status ?? 'not reported'}</dd>
      <dt>Model configured before it</dt><dd>{model ? `${model} (observed context, by order; not a per-reply binding)` : 'not known'}</dd>
      <dt>Tokens</dt><dd>not available: {met.tokens.why}</dd>
      <dt>Cost</dt><dd>not available: {met.cost.why}</dd>
    </dl>
  )
}

/** Level two: what happened, one sentence per step; level three: a step's audit record. */
function Steps({ m, rows, episode, audit, onAudit, live }) {
  const [suggesting, setSuggesting] = useState(false)
  return (
    <div className="rth-steps" data-rth="mind-steps">
      {m.type === 'heard' && !m.typed ? <div className="rth-faint" style={{ marginBottom: 4 }}>What the provider transcribed. It does not say who spoke{m.final ? '' : ', and it may still change'}.</div> : null}
      {m.metrics ? <Breakdown met={m.metrics} model={m.model} /> : null}
      {episode ? <div className="rth-faint" style={{ marginBottom: 4 }}>{episode.title} · <Chip status={!live && ['pending', 'attempted'].includes(episode.status) ? 'unknown' : episode.status} /></div> : null}
      <ol>
        {rows.map((r) => (
          <li key={r.id}>
            <button type="button" className="rth-step" data-rth="mind-row" data-id={r.id} data-kind={r.kind} data-category={r.category} data-status={r.status} data-episode={r.episode ?? ''} data-lane={r.lane} aria-expanded={audit === r.id} onClick={() => !r.local && onAudit(audit === r.id ? null : r.id)}>
              <span className="rth-step-t">{hms(r.observed_at).slice(0, 8)}</span>
              <span className="rth-step-s" data-rth="mind-sentence">{r.sentence}</span>
              <Chip status={r.status} />
            </button>
            {audit === r.id ? <Audit id={r.id} onClose={() => onAudit(null)} onSelect={onAudit} /> : null}
          </li>
        ))}
      </ol>
      {episode ? (
        <div className="rth-steps-foot">
          <button type="button" className="rth-link" data-rth="mind-suggest" onClick={() => setSuggesting(!suggesting)}>{suggesting ? 'Close the suggestion' : 'Suggest a better way'}</button>
        </div>
      ) : null}
      {suggesting && episode ? <Suggest ep={episode} /> : null}
    </div>
  )
}

const SOURCE_NAMES = { agentd_feed: "The agent's live feed", host_journal: "The host's log", agentd_journal: "The agent's journal", brain: 'Brain (memory)', jev: 'Jev' }
const SOURCE_STATE = { bound: ['Connected', 'ok'], following: ['Following', 'ok'], off: ['Not set up', 'neutral'], 'no feed': ['No feed yet', 'neutral'] }

function Sources({ sources, fresh, reconnects, gaps = 0, usage = 0 }) {
  return (
    <div data-rth="mind-sources">
      <div className="rth-faint" style={{ marginBottom: 6 }}>What Mind can see</div>
      {sources ? (
        <ul className="rth-src">
          {Object.entries(sources).map(([k, v]) => {
            const [word, tone] = SOURCE_STATE[v.state] ?? [v.state, 'warn']
            return <li key={k} data-source={k} data-state={v.state} title={v.detail}><Dot tone={tone} /> <span>{SOURCE_NAMES[k] ?? k}</span> <span className="rth-faint">{word}</span></li>
          })}
        </ul>
      ) : <div className="rth-faint">Not known yet.</div>}
      {gaps ? <div className="rth-faint" style={{ marginBottom: 6 }} data-rth="mind-gaps">{gaps} gap{gaps > 1 ? 's' : ''} in this feed (sequence numbers not seen here: sent to another connection, or skipped while catching up; how many events is not known).</div> : null}
      {usage ? <div className="rth-faint" style={{ marginBottom: 6 }} data-rth="mind-usage">{usage} provider usage report{usage > 1 ? 's' : ''} seen, not linked to any reply (they name a connection, not a reply), so not counted per reply.</div> : null}
      <div className="rth-mono rth-faint" style={{ marginBottom: 8 }}>{fresh.stream.label}{reconnects ? ` · reloaded ${reconnects}×` : ''} · {fresh.agent.label}</div>
    </div>
  )
}

/** The full record: every observed row, newest first, searchable. Nothing here can be deleted. */
function EveryEvent({ rows, episodes, dropped, audit, onAudit }) {
  const [query, setQuery] = useRemembered('mind:query', '')
  const q = query.trim().toLowerCase()
  const shown = rows.filter((r) => !q || `${r.sentence} ${r.kind} ${r.event_id ?? ''} ${r.episode ?? ''}`.toLowerCase().includes(q)).slice(-SHOWN).reverse()
  return (
    <div data-rth="mind-every">
      <input className="rth-input" data-rth="mind-search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search sentences and ids" aria-label="Search every event" />
      <div className="rth-faint" style={{ margin: '6px 0 0', fontSize: 11.5 }} data-rth="mind-count">{shown.length} of {rows.length} rows{dropped ? `, ${dropped} evicted` : ''}</div>
      <ul className="rth-all">
        {shown.map((r) => {
          const pt = producerTime(r)
          return (
            <li key={r.id}>
              <button type="button" className="rth-step" data-rth="mind-row" data-id={r.id} data-kind={r.kind} data-category={r.category} data-status={r.status} data-episode={r.episode ?? ''} data-lane={r.lane} aria-expanded={audit === r.id} onClick={() => !r.local && onAudit(audit === r.id ? null : r.id)}>
                <span className="rth-step-t">{hms(r.observed_at).slice(0, 8)}</span>
                <span className="rth-step-s">
                  <span data-rth="mind-sentence">{r.sentence}</span>
                  <span className="rth-faint" style={{ display: 'block', fontSize: 10.5 }}>{CATS[r.category]?.label ?? r.category} · {r.source}·{r.lane}{pt ? ` · ${pt.text}` : ''}{r.duration_ms != null ? ` · ${dur(r.duration_ms)}` : ''}{r.count > 1 ? ` · ×${r.count}` : ''}{r.episode && episodes[r.episode] ? ` · ${r.episode}` : ''}</span>
                </span>
                <Chip status={r.status} />
              </button>
              {audit === r.id ? <Audit id={r.id} onClose={() => onAudit(null)} onSelect={onAudit} /> : null}
            </li>
          )
        })}
      </ul>
    </div>
  )
}

function newKey() {
  try {
    return `mind-${crypto.randomUUID()}`
  } catch {
    return `mind-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
  }
}

/** One form = one idempotency key: a retry after a failure resends the same key, so it can't duplicate. */
function Suggest({ ep }) {
  const [key, setKey] = useState(newKey)
  const [actor, setActor] = useState(() => {
    try {
      return localStorage.getItem('rth-mind-actor') ?? ''
    } catch {
      return ''
    }
  })
  const [text, setText] = useState('')
  const [result, setResult] = useState(null)
  const [history, setHistory] = useState(null)
  useEffect(() => {
    fetch(new URL(`${BASE}/suggestions?episode=${encodeURIComponent(ep.id)}`, document.baseURI)).then((r) => r.json()).then((b) => setHistory(b.suggestions)).catch(() => setHistory(null))
  }, [ep.id, result])
  const send = async () => {
    try {
      localStorage.setItem('rth-mind-actor', actor)
    } catch {}
    setResult({ state: 'sending' })
    try {
      const r = await fetch(new URL(`${BASE}/suggestions`, document.baseURI), { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ key, actor, scope: { episode: ep.id, title: ep.title }, text }) })
      const b = await r.json()
      if (!r.ok) return setResult({ state: 'failed', detail: b.error ?? `status ${r.status}` })
      setResult({ state: b.duplicate ? 'duplicate' : 'saved', record: b.record })
      setText('')
      setKey(newKey())
    } catch (e) {
      setResult({ state: 'failed', detail: `${e.message} (retrying sends the same key)` })
    }
  }
  return (
    <div data-rth="mind-suggest-form" style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 4 }}>
      <div style={{ fontSize: 12, ...muted }}>How would a mind handle this differently? A proposal for people to read: it is appended to this plugin's local log, sent nowhere, and changes nothing in Unfold.</div>
      <input data-rth="mind-suggest-actor" value={actor} onChange={(e) => setActor(e.target.value)} placeholder="your name (not verified)" maxLength={80} style={{ ...button, cursor: 'text' }} />
      <textarea data-rth="mind-suggest-text" value={text} onChange={(e) => setText(e.target.value)} rows={3} maxLength={4000} placeholder="e.g. a person would wait for the end of my sentence before answering" style={{ ...button, cursor: 'text', resize: 'vertical' }} />
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <button type="button" data-rth="mind-suggest-send" style={button} disabled={!text.trim() || result?.state === 'sending'} onClick={send}>Append suggestion</button>
        {result ? <span data-rth="mind-suggest-result" data-state={result.state} style={{ fontSize: 12 }}>{result.state === 'saved' ? `saved (${result.record.key})` : result.state === 'duplicate' ? 'already saved under this key; nothing added' : result.state === 'failed' ? `not saved: ${result.detail}` : 'sending…'}</span> : null}
      </div>
      {history?.length ? (
        <ul data-rth="mind-suggest-history" style={{ margin: 0, paddingLeft: 16, fontSize: 12, ...muted }}>
          {history.map((h) => <li key={h.key}>{h.at.slice(0, 19).replace('T', ' ')} {h.actor}: {h.text}</li>)}
        </ul>
      ) : null}
    </div>
  )
}

// ---------------------------------------------------------------- audit panel

function Audit({ id, onClose, onSelect }) {
  const [d, setD] = useState(null)
  const [error, setError] = useState(null)
  const [copied, setCopied] = useState(null)
  // One loader per panel: only the latest selection's answer, for exactly that id, is shown.
  const loader = useRef(null)
  if (!loader.current) {
    loader.current = auditLoader(async (rowId, signal) => {
      const r = await fetch(new URL(`${BASE}/event?id=${encodeURIComponent(rowId)}`, document.baseURI), { signal })
      const body = await r.json().catch(() => ({}))
      return r.ok ? { ok: true, body } : { ok: false, error: body.error ?? `status ${r.status}` }
    })
  }
  useEffect(() => {
    setD(null)
    setError(null)
    setCopied(null)
    loader.current.select(id, (s) => {
      if (s.state === 'ready') setD(s.data)
      else if (s.state === 'error') setError(s.error)
    })
  }, [id])
  useEffect(() => () => loader.current?.cancel(), [])
  const copy = async () => {
    if (d?.row?.id !== id) return // never copy evidence for a row other than the selected one
    const text = JSON.stringify({ evidence: d.evidence, evidence_sha256: d.evidence_sha256, evidence_sha256_basis: d.evidence_sha256_basis }, null, 2)
    try {
      await navigator.clipboard.writeText(text)
      setCopied({ state: 'copied' })
    } catch {
      setCopied({ state: 'manual', text })
    }
  }
  const kv = (o) => Object.entries(o ?? {}).filter(([, v]) => v !== null && v !== undefined && !(typeof v === 'object' && !Object.values(v).some((x) => x != null)))
  const section = { margin: '6px 0 2px', fontWeight: 600, fontSize: 12 }
  const pre = { margin: 0, padding: 6, fontSize: 11, maxHeight: 180, overflow: 'auto', background: 'rgba(127,127,127,0.1)', borderRadius: 4, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }
  return (
    <div data-rth="mind-audit" data-id={id} style={{ ...box, flex: '0 1 52%', overflow: 'auto', borderColor: 'rgba(59,130,246,0.7)' }}>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <b style={{ flex: 1 }}>Audit record</b>
        {d ? <button type="button" data-rth="mind-copy" data-state={copied?.state ?? 'idle'} style={button} onClick={copy}>{copied?.state === 'copied' ? 'Copied ✓' : 'Copy as evidence'}</button> : null}
        <button type="button" style={button} onClick={onClose} aria-label="close audit record">✕</button>
      </div>
      {error ? <div style={muted}>Not available: {error}</div> : null}
      {!d && !error ? <div style={muted}>loading…</div> : null}
      {copied?.state === 'manual' ? <textarea readOnly value={copied.text} rows={6} style={{ width: '100%', fontSize: 11 }} onFocus={(e) => e.target.select()} /> : null}
      {d ? (
        <div style={{ fontSize: 12 }}>
          <div data-rth="mind-audit-sentence" style={{ marginTop: 4 }}>{CATS[d.row.category]?.icon} {d.row.sentence} <Chip status={d.row.status} /></div>
          <div style={{ ...muted, fontSize: 11 }}>{d.evidence.row.sentence_basis}</div>

          <div style={section}>Ids</div>
          <table data-rth="mind-audit-ids" style={{ borderCollapse: 'collapse', fontSize: 11 }}><tbody>
            {[...kv({ event: d.ids.event, cause: d.ids.cause, producer: d.ids.producer, seq: d.ids.seq, session: d.ids.session_id, boot: d.ids.boot_id, epoch: d.ids.epoch, generation: d.ids.generation, attachment: d.ids.attachment_id }), ...kv(d.ids.correlation)].map(([k, v]) => <tr key={k}><td style={{ paddingRight: 8, ...muted }}>{k}</td><td style={{ overflowWrap: 'anywhere' }}><code>{String(v)}</code></td></tr>)}
            {!d.ids.event ? <tr><td style={{ paddingRight: 8, ...muted }}>event</td><td>unknown (this source carries no event id)</td></tr> : null}
          </tbody></table>

          <div style={section}>Cause chain (explicit cause ids only)</div>
          <div data-rth="mind-audit-chain">
            {d.chain.parents.length ? d.chain.parents.map((p, i) => <div key={p.id}>{'↑'.repeat(i + 1)} <a href="#" data-rth="mind-audit-parent" onClick={(e) => (e.preventDefault(), onSelect(p.id))}>{p.sentence}</a></div>) : <div style={muted}>{d.ids.cause ? '' : 'no cause id on this event'}</div>}
            {d.chain.unresolved ? <div style={muted}>↑ <code>{d.chain.unresolved.event_id}</code>: {d.chain.unresolved.reason}</div> : null}
            {d.chain.children.map((c) => <div key={c.id}>↓ <a href="#" data-rth="mind-audit-child" onClick={(e) => (e.preventDefault(), onSelect(c.id))}>{c.sentence}</a></div>)}
          </div>
          {d.episode ? (
            <>
              <div style={section}>Episode {d.episode.id}: {d.episode.title} <Chip status={d.episode.status} /></div>
              <div style={{ ...muted, fontSize: 11 }}>grouped through shared ids (response, command, decision, utterance) and explicit causes only · {d.episode.because}</div>
            </>
          ) : <div style={{ ...section, fontWeight: 400, ...muted }}>Not part of an episode (stream, connection or system row, or no shared id).</div>}

          <div style={section}>Source</div>
          <table style={{ borderCollapse: 'collapse', fontSize: 11 }}><tbody>
            <tr><td style={{ paddingRight: 8, ...muted }}>source</td><td>{d.row.source} · lane {d.row.lane} · training eligible: {d.row.training_eligible === null ? 'unknown' : String(d.row.training_eligible)}</td></tr>
            <tr><td style={{ paddingRight: 8, ...muted }}>observed</td><td>{d.evidence.observed_at} (DSH host clock)</td></tr>
            <tr><td style={{ paddingRight: 8, ...muted }}>clock</td><td>{d.clock.basis}{d.clock.t_ns != null ? `: t_ns ${d.clock.t_ns}` : ''}{d.clock.t_ms != null ? `: t_ms ${d.clock.t_ms}` : ''}. Wall time: {d.clock.wall_ms_estimate != null ? `${new Date(d.clock.wall_ms_estimate).toISOString()} (${d.clock.wall_basis})` : d.clock.wall_basis}</td></tr>
            {d.origins.map((o, i) => <tr key={i}><td style={{ paddingRight: 8, ...muted }}>origin {i + 1}</td><td style={{ overflowWrap: 'anywhere' }}>{o.via}; process {o.process ?? 'unknown'}; file {o.file ?? 'none'}{o.offset != null ? ` @ byte ${o.offset} (${o.length} bytes)` : '; offset unknown'}{o.agent_journal ? `; agent journal ${o.agent_journal}` : ''}{o.note ? `; ${o.note}` : ''}</td></tr>)}
          </tbody></table>

          <div style={section}>Integrity</div>
          <div data-rth="mind-audit-integrity" style={{ fontSize: 11 }}>
            <div>{d.integrity.journal_record_hash}</div>
            {d.integrity.observer_feed ? <div>{d.integrity.observer_feed}</div> : null}
            {d.integrity.file_lines.map((l, i) => <div key={i}>line sha256 <code>{l.line_sha256}</code> at byte {l.offset} of {l.file} ({l.basis})</div>)}
            {d.integrity.payload_hashes.map((h) => <div key={h.field}>{h.field} <code>{h.value}</code>: {h.means}</div>)}
            <div style={muted}>evidence sha256 <code>{d.evidence_sha256}</code> ({d.evidence_sha256_basis})</div>
          </div>

          <div style={section}>What this kind means</div>
          <Definition def={d.definition} />

          <div style={section}>Raw envelope (redacted)</div>
          <div style={{ ...muted, fontSize: 11 }}>{d.envelope_basis}</div>
          <pre data-rth="mind-audit-envelope" style={pre}>{d.envelope ? JSON.stringify(d.envelope, null, 2) : 'none'}</pre>
        </div>
      ) : null}
    </div>
  )
}

function Definition({ def }) {
  if (!def) return <div>unknown</div>
  if (def.glossary) return <div data-rth="mind-audit-definition" style={{ fontSize: 11 }}>{def.glossary}</div>
  const c = def.catalog
  return (
    <div data-rth="mind-audit-definition" style={{ fontSize: 11 }}>
      {c?.name ? (
        <div>
          <b>{c.name}</b> (wire <code>{def.kind}</code>, phase {c.phase}: {c.phase_means}; {c.file} v{c.version}).
          {c.note ? <> {c.note}.</> : null}
          {c.field_notes?.map((n, i) => <div key={i} style={{ marginTop: 2 }}>Fields {n.added.join(', ') || '(none added)'} ({n.source}): {n.note}</div>)}
        </div>
      ) : <div>catalog: {c?.missing ?? 'unknown'}</div>}
      {def.protocol?.variant ? <div style={{ marginTop: 2 }}>rth-protocol v2 <code>Event::{def.protocol.variant}</code> ({def.protocol.direction}): {def.protocol.comment}</div> : <div>protocol: {def.protocol?.missing ?? 'unknown'}</div>}
    </div>
  )
}
