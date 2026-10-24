// @rth/dsh-events, browser half: the "RTH events" right-sidebar tab inside DSH's own web client.
// The two-stage path from upstream ui-sidebar-files, unmodified: the type into
// ctx.sidebarRightTabs, the body and chip title into the keyed `sidebar.right.pane.tab` and
// `sidebar.right.pane.tab.title` seats under the type's id. Extension priority, its own kind
// and id, no resource patterns: it claims no address, so it overrides no Bot or DSH tab.
//
// Data arrives by push only: one EventSource on the host's /api/rth-events/stream per mounted
// body, a snapshot then increments. Closing the tab closes that stream; the host keeps
// ingesting. Rows are drawn by their plain `name` from the catalog; the UI names no kinds.
import { Fragment, useEffect, useId, useReducer, useRef, useState } from 'react'
import { BASE_CSS, Dot, Fold, clock, clockS, guarded, useNow, useStyle } from '../../rth-ui/ui.jsx'
import { CP_PREVIEW, createVoiceController, grainBudget, lifeFor, presenceFacts, presenceWords, readDelay, sceneFor, shouldRead, voiceKey } from '../src/avatar.mjs'
import { dustRenderer } from './dust-renderer.js'
import { MAX_SLOTS, WORDS, langOf, nextQueueChange, slotLine, stateOf, stepQueue } from '../src/queue.mjs'
// Defense in depth on the surface: every summary is masked with Mind's redactText before it is
// drawn (the server window redacts reply text, not every kind's summary).
import { redactText } from '../../dsh-mind/src/redact.mjs'

export const RTH_EVENTS_ID = '@rth/dsh-events'
export const RTH_EVENTS_KIND = 'rth.events'
// The presence: a second tab of this plugin, fed by its own pushed stream. It is RTH's avatar:
// the cosmic-dust creature in a capsule on a phone-like frame; a tap wakes or sleeps voice
// (rth-host's mic and speaker) through rth-session's call route.
export const RTH_PRESENCE_ID = '@rth/dsh-events/presence'
export const RTH_PRESENCE_KIND = 'rth.presence'
const STREAM = 'api/rth-events/stream'
const PRESENCE_STREAM = 'api/rth-presence/stream'
const SHOWN = 200
// The look is swappable behind this one factory (client/presence-renderer.js interface).
const createRenderer = dustRenderer

export const inject = ['slots', 'sidebarRightTabs', 'sidebarRight']

export function apply(ctx) {
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: RTH_EVENTS_ID,
    kind: RTH_EVENTS_KIND,
    priority: 'extension',
    title: () => 'Events',
    guide: [{
      id: 'rth-events',
      order: 90,
      title: () => 'Unfold events',
      description: () => 'Every event from the agent, as it arrives',
    }],
  }), 'rth-events: tab type')
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: RTH_PRESENCE_ID,
    kind: RTH_PRESENCE_KIND,
    priority: 'extension',
    title: () => 'Unfold',
    guide: [{
      id: 'rth-presence',
      order: 91,
      title: () => 'Unfold',
      description: () => 'Unfold itself: tap to wake or sleep voice',
    }],
  }), 'rth-presence: tab type')
  // On unload, close our open tabs in every Session: DSH's layout would otherwise keep each
  // one's chip (its captured title) with no body behind it.
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== RTH_EVENTS_KIND && tab.kind !== RTH_PRESENCE_KIND) continue
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId)
      } catch {} // the Session or tab already went away
    }
  }, 'rth-events: close open tabs on unload')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab', key: RTH_EVENTS_ID },
    guarded('rth-events', EventsBody),
  )), 'rth-events: tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab.title', key: RTH_EVENTS_ID },
    EventsTitle,
  )), 'rth-events: tab title')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab', key: RTH_PRESENCE_ID },
    guarded('rth-presence', PresenceBody),
  )), 'rth-presence: tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab.title', key: RTH_PRESENCE_ID },
    PresenceTitle,
  )), 'rth-presence: tab title')
}

function EventsTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-rth="events-title">{tab.title}</span>
}

const initial = { rows: [], link: null, source: null, stream: 'opening', attaches: 0, history: [], status: null, rev: 0, dropped: 0 }

function reduce(state, { event, data }) {
  switch (event) {
    case 'open':
      return { ...state, stream: 'open' }
    case 'error':
      return { ...state, stream: 'reconnecting' }
    case 'snapshot': {
      const attached = data.link?.state === 'attached'
      return { ...state, rows: data.rows, link: data.link, source: data.source, status: data.status, rev: data.rev, dropped: data.dropped, attaches: Math.max(state.attaches, attached ? 1 : 0) }
    }
    case 'added':
      return { ...state, rows: [...state.rows, data] }
    case 'updated':
      return { ...state, rows: state.rows.map((r) => (r.key === data.key ? { ...r, ...data } : r)) }
    case 'evicted':
      return { ...state, rows: state.rows.filter((r) => r.key !== data.key), dropped: state.dropped + 1 }
    case 'status':
      return { ...state, status: data }
    case 'link': {
      const attaches = data.state === 'attached' ? state.attaches + 1 : state.attaches
      const history = [...state.history, { ...data, at: new Date().toLocaleTimeString() }].slice(-6)
      return { ...state, link: data, attaches, history }
    }
    default:
      return state
  }
}

/** One stream per mounted body: opened on mount, closed on unmount (tab close, plugin unload). */
function useEventStream() {
  const [state, dispatch] = useReducer(reduce, initial)
  useEffect(() => {
    const source = new EventSource(new URL(STREAM, document.baseURI))
    const on = (event) => (e) => dispatch({ event, data: e.data === undefined ? null : JSON.parse(e.data) })
    const names = ['snapshot', 'added', 'updated', 'evicted', 'status', 'link']
    const handlers = names.map((name) => [name, on(name)])
    for (const [name, fn] of handlers) source.addEventListener(name, fn)
    source.onopen = () => dispatch({ event: 'open' })
    source.onerror = () => dispatch({ event: 'error' })
    return () => source.close()
  }, [])
  return state
}

function linkText(link, attaches) {
  if (!link) return 'agentd: not linked yet'
  switch (link.state) {
    case 'attached':
      return attaches > 1 ? `agentd: reconnected (attach ${attaches}) as a read-only observer` : 'agentd: attached as a read-only observer'
    case 'waiting':
      return `agentd: link lost, reconnecting in ${link.detail} ms`
    case 'connecting':
      return 'agentd: connecting'
    case 'error':
      return `agentd: link error (${link.detail})`
    default:
      return `agentd: ${link.state}`
  }
}

const EVENTS_CSS = `
.rth-ev{display:flex;flex-direction:column;height:100%;overflow:hidden}
.rth-ev-head{display:flex;align-items:center;gap:9px;padding:14px 16px 12px;border-bottom:1px solid var(--rth-line)}
.rth-ev-head b{font-size:14px;font-weight:600}
.rth-ev-head[data-state=down] b{color:var(--rth-muted);font-weight:500}
.rth-ev-meta{margin-left:auto;font-size:11.5px;color:var(--rth-faint)}
.rth-ev-note{padding:8px 16px;font-size:11.5px;color:var(--rth-warn);border-bottom:1px solid var(--rth-line)}
.rth-ev-list{flex:1 1 auto;overflow:auto;list-style:none;margin:0;padding:6px 8px 16px}
.rth-ev-row{all:unset;box-sizing:border-box;display:grid;grid-template-columns:10px 1fr auto;gap:2px 10px;align-items:baseline;width:100%;padding:6px 8px;border-radius:8px;cursor:pointer}
.rth-ev-row:hover{background:var(--rth-hover)}
.rth-ev-row:focus-visible{outline:2px solid var(--rth-accent)}
.rth-ev-row .rth-dot{width:6px;height:6px;align-self:center}
.rth-ev-name{font-size:12.5px;font-weight:550;min-width:0}
.rth-ev-time{font-size:11px;color:var(--rth-faint);font-variant-numeric:tabular-nums}
.rth-ev-sum{grid-column:2/4;font-size:12px;color:var(--rth-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.rth-ev-row[aria-expanded=true] .rth-ev-sum{white-space:normal;overflow-wrap:anywhere}
.rth-ev-more{grid-column:2/4;margin-top:4px}
.rth-ev-row[data-known=false] .rth-ev-name{color:var(--rth-warn)}
.rth-ev-foot{padding:0 14px;border-top:1px solid var(--rth-line)}
.rth-ev-foot>.rth-fold{border-top:0}
.rth-ev-fresh{animation:rth-in .5s var(--rth-ease)}
@keyframes rth-in{from{background:color-mix(in oklab,var(--rth-accent) 12%,transparent)}to{background:transparent}}
`

const PHASE = { connection: ['Connection', 'neutral'], agent: ['Agent', 'neutral'], input: ['Input', 'accent'], decision: ['Decision', 'accent'], action: ['Action', 'ok'], result: ['Result', 'ok'] }
const plainName = (n) => (n ? `${n[0].toUpperCase()}${n.slice(1).replace(/_/g, ' ')}` : 'Unknown event')

function linkWords(link, stream) {
  if (stream !== 'open') return ['Reconnecting…', 'down']
  if (!link) return ['Connecting…', 'down']
  switch (link.state) {
    case 'attached': return ['Connected', 'live']
    case 'waiting': return ['Lost the agent, trying again', 'down']
    case 'connecting': return ['Connecting…', 'down']
    case 'error': return ["Can't reach the agent", 'down']
    default: return [plainName(link.state), 'down']
  }
}

function EventsBody() {
  useStyle('rth-ui-base', BASE_CSS)
  useStyle('rth-ui-events', EVENTS_CSS)
  const { rows, link, source, stream, attaches, history } = useEventStream()
  const [open, setOpen] = useState(null)
  // When this tab first saw each row (a local clock; the feed has no wall time on rows).
  const seen = useRef(new Map())
  const first = useRef(true)
  for (const r of rows) if (!seen.current.has(r.key)) seen.current.set(r.key, first.current ? null : Date.now())
  if (rows.length) first.current = false
  const shown = rows.slice(-SHOWN).reverse()
  const [words, state] = linkWords(link, stream)
  return (
    <div className="rth-ui rth-ev" data-rth="events">
      <header className="rth-ev-head" data-state={state} data-rth="events-link" data-attaches={attaches}>
        <Dot tone={state === 'live' ? 'ok' : 'neutral'} />
        <b data-state={link?.state ?? 'none'}>{words}</b>
        <span className="rth-ev-meta rth-num" data-rth="events-count">{rows.length} events</span>
      </header>
      {attaches > 1 ? <div className="rth-ev-note">Reconnected. Events from while it was down are not shown here.</div> : null}
      <ol className="rth-ev-list" data-rth="events-rows">
        {shown.map((row) => {
          const [phase, tone] = PHASE[row.phase] ?? ['Other', 'neutral']
          const at = seen.current.get(row.key)
          const isOpen = open === row.key
          return (
            <li key={row.key}>
              <button type="button" className={`rth-ev-row${at && Date.now() - at < 1500 ? ' rth-ev-fresh' : ''}`} aria-expanded={isOpen} onClick={() => setOpen(isOpen ? null : row.key)} data-rth="event-row" data-key={row.key} data-name={row.name} data-count={row.count} data-known={String(row.known)}>
                <Dot tone={tone} />
                <span className="rth-ev-name">{row.known ? plainName(row.name) : 'Unrecognised event'}{row.count > 1 ? <span className="rth-faint"> ×{row.count}</span> : null}</span>
                <span className="rth-ev-time">{at ? clockS(at) : ''}</span>
                {row.summary ? <span className="rth-ev-sum">{redactText(row.summary)}</span> : null}
                {isOpen ? (
                  <span className="rth-ev-more rth-mono rth-faint">
                    {phase} · {row.wire}{row.known ? '' : ' (not in the catalog)'} · {row.key}{row.cause ? ` · caused by ${row.cause}` : ''}{at ? ` · seen ${clockS(at)}` : ' · seen before this tab opened'}
                  </span>
                ) : null}
              </button>
            </li>
          )
        })}
        {!shown.length ? <li className="rth-faint" style={{ padding: 16, textAlign: 'center' }}>No events yet.</li> : null}
      </ol>
      <footer className="rth-ev-foot">
        <Fold id="events:about" summary="About this list">
          <div className="rth-faint" style={{ display: 'flex', flexDirection: 'column', gap: 6, fontSize: 12 }}>
            <div>Everything the agent sends to Unfold's read-only observer, newest first, named from the event catalog. Mind tells the same story in plain sentences.</div>
            {source && !source.host_frames ? <div>This is the live feed only: what the host sends is not shown, and nothing is replayed, so some causes stay unknown.</div> : null}
            <div className="rth-mono" data-rth="events-stream" data-state={stream}>{linkText(link, attaches)} · tab stream {stream}</div>
            {history.length ? (
              <ul data-rth="events-link-history" className="rth-mono" style={{ margin: 0, paddingLeft: 16 }}>
                {history.map((h, i) => <li key={i}>{h.at} {h.state}{h.detail == null ? '' : ` (${h.detail})`}</li>)}
              </ul>
            ) : null}
          </div>
        </Fold>
      </footer>
    </div>
  )
}

function PresenceTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-rth="presence-title">{tab.title}</span>
}

// The server heartbeats every 10 s; 25 s of silence is a stale stream.
const PRESENCE_LEASE_MS = 25_000

/**
 * One presence stream per mounted body. A lost stream, or one silent past its lease (a frozen
 * server keeps the socket open), is a stale source: `unknown` at once, then a fresh stream
 * with backoff.
 */
function usePresence() {
  const [state, setState] = useState({ view: null, stream: 'opening' })
  useEffect(() => {
    let source = null
    let lease = null
    let retry = null
    let backoff = 1000
    const stale = () => {
      source?.close()
      setState((s) => ({ ...s, stream: 'stale' }))
      retry = setTimeout(open, backoff)
      backoff = Math.min(backoff * 2, 8000)
    }
    const alive = () => {
      clearTimeout(lease)
      lease = setTimeout(stale, PRESENCE_LEASE_MS)
    }
    function open() {
      source = new EventSource(new URL(PRESENCE_STREAM, document.baseURI))
      alive()
      source.addEventListener('presence', (e) => {
        alive()
        backoff = 1000
        setState({ view: JSON.parse(e.data), stream: 'open' })
      })
      source.addEventListener('heartbeat', alive)
      source.onerror = () => setState((s) => ({ ...s, stream: 'reconnecting' }))
    }
    open()
    return () => {
      clearTimeout(lease)
      clearTimeout(retry)
      source?.close()
    }
  }, [])
  return state
}

const STALE = { pose: 'unknown', label: 'Unknown', because: 'the presence stream is not open', layers: null, faults: [] }

function useReducedMotion() {
  const query = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null
  const [reduced, setReduced] = useState(Boolean(query?.matches))
  useEffect(() => {
    if (!query) return
    const on = (e) => setReduced(e.matches)
    query.addEventListener('change', on)
    return () => query.removeEventListener('change', on)
  }, [])
  return reduced
}

/**
 * The voice half: one controller per mounted body (bounded, ordered reads of rth-session's
 * `api/rth-session/call`). Reads only: mounting, re-rendering,
 * a hidden page coming back and presence events never send a request; a tap or the mute glyph
 * does. Re-renders only when what the tab shows changes (a 500 ms clock expires leases and
 * pending taps with no new answer; reads are fast while something settles, slow otherwise, and
 * paused while the page is hidden).
 */
function useVoice() {
  const [, force] = useReducer((n) => n + 1, 0)
  const ctl = useRef(null)
  const make = () => createVoiceController({ fetch: (...a) => fetch(...a), onChange: force })
  if (!ctl.current) ctl.current = make()
  useEffect(() => {
    if (ctl.current.disposed) ctl.current = make()
    const c = ctl.current
    let last = -Infinity
    let key = voiceKey(c.view())
    const due = () => {
      if (!shouldRead({ hidden: document.visibilityState === 'hidden', sinceLast: performance.now() - last, delay: readDelay(c.view()) })) return
      last = performance.now()
      void c.load()
    }
    const tick = setInterval(() => {
      const k = voiceKey(c.view())
      if (k !== key) {
        key = k
        force()
      }
      due()
    }, 500)
    const onVisible = () => {
      if (document.visibilityState !== 'hidden') {
        last = -Infinity
        due()
      }
    }
    document.addEventListener('visibilitychange', onVisible)
    DEBUG.listeners += 1
    due()
    return () => {
      clearInterval(tick)
      document.removeEventListener('visibilitychange', onVisible)
      DEBUG.listeners -= 1
      c.dispose()
    }
  }, [])
  return ctl.current
}

const DEBUG = (globalThis.__rthAvatar ??= { live: 0, created: 0, frames: 0, rafs: 0, timeouts: 0, observers: 0 })
DEBUG.listeners ??= 0

/** Visible = the page is shown and the avatar is on screen (a hidden tab or window draws nothing). */
function useVisible(ref) {
  const [visible, setVisible] = useState(true)
  useEffect(() => {
    let page = document.visibilityState !== 'hidden'
    let onScreen = true
    const apply = () => setVisible(page && onScreen)
    const onPage = () => {
      page = document.visibilityState !== 'hidden'
      apply()
    }
    document.addEventListener('visibilitychange', onPage)
    DEBUG.listeners += 1
    const io = typeof IntersectionObserver === 'function' && ref.current
      ? new IntersectionObserver((entries) => {
        onScreen = entries.some((e) => e.isIntersecting)
        apply()
      })
      : null
    if (io) {
      io.observe(ref.current)
      DEBUG.observers += 1
    }
    return () => {
      document.removeEventListener('visibilitychange', onPage)
      DEBUG.listeners -= 1
      if (io) {
        io.disconnect()
        DEBUG.observers -= 1
      }
    }
  }, [])
  return visible
}

const MicGlyph = ({ state }) => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <rect x="9" y="2.5" width="6" height="11.5" rx="3" />
    <path d="M18.5 10.5v.5a6.5 6.5 0 0 1-13 0v-.5M12 17.5V21" />
    {state === 'muted' || state === 'unmuting' ? <path d="M3.5 3.5l17 17" /> : null}
  </svg>
)

const MIC_WORD = { muted: 'Muted', muting: 'Muting…', unmuting: 'Unmuting…' }

const PRESENCE_CSS = `
.rth-pres{display:flex;flex-direction:column;align-items:center;gap:14px;padding:14px 16px 22px}
/* No device frame: the creature floats in a luminous capsule in the tab itself.
   The capsule is dark glass in both themes (the dust needs the dark); the words follow the theme. */
.rth-stage{--glow:.12;--rim:98,230,242;position:relative;isolation:isolate;width:100%;max-width:300px;display:flex;flex-direction:column;align-items:stretch}
.rth-stage[data-voice=active]{--glow:.34}
.rth-stage[data-voice=starting]{--glow:.24}
.rth-stage[data-voice=ending]{--glow:.16}
.rth-stage[data-voice=unknown],.rth-stage[data-presence=unknown]{--glow:.05;--rim:150,156,170}
.rth-bar{display:flex;align-items:center;justify-content:space-between;height:28px;padding:0 2px}
.rth-name{display:flex;align-items:center;gap:7px;font-size:12px;font-weight:600;letter-spacing:.04em;color:var(--rth-muted)}
.rth-mic{display:inline-flex;align-items:center;gap:5px;height:26px;min-width:26px;padding:0 7px;border-radius:13px;border:0;background:transparent;color:var(--rth-muted);cursor:pointer;font-size:11px;font-weight:600;transition:background .2s,color .2s}
.rth-mic:hover{background:var(--rth-hover);color:var(--rth-fg)}
.rth-mic[data-state=muted],.rth-mic[data-state=unmuting]{color:var(--rth-warn);background:color-mix(in oklab,var(--rth-warn) 13%,transparent)}
.rth-mic[data-state=unknown]{color:var(--rth-faint)}
.rth-mic[aria-disabled=true]{cursor:default}
.rth-mic[aria-disabled=true]:hover{background:transparent}
.rth-tube{position:relative;display:block;width:76%;margin:10px auto 0;aspect-ratio:3/4.7;padding:0;border:0;border-radius:50%/19%;cursor:pointer;background:#04040a;
 box-shadow:inset 0 0 0 1px rgba(var(--rim),.26),inset 0 0 34px rgba(var(--rim),calc(var(--glow)*.8)),0 0 38px rgba(var(--rim),calc(var(--glow)*.55));transition:box-shadow .8s var(--rth-ease),transform .25s var(--rth-ease)}
.rth-tube:hover{transform:translateY(-1px)}
.rth-tube:active{transform:scale(.985)}
.rth-tube[aria-disabled=true]{cursor:default;transform:none}
.rth-tube::after{content:"";position:absolute;inset:0;border-radius:inherit;pointer-events:none;opacity:0;box-shadow:0 0 54px rgba(var(--rim),.30);transition:opacity .8s}
.rth-stage[data-alive=true] .rth-tube::after{animation:rth-breathe 5s ease-in-out infinite}
@keyframes rth-breathe{0%,100%{opacity:.15}50%{opacity:.6}}
.rth-tube canvas{position:absolute;inset:5% 6%;width:88%;height:90%;border-radius:50%/18%;display:block}
.rth-pres .rth-pres-cap{position:absolute;left:22%;right:22%;height:8px;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(.22 + var(--glow))),rgba(var(--rim),0));transition:opacity .8s}
.rth-pres .rth-pres-cap.top{top:.8%}.rth-pres .rth-pres-cap.bot{bottom:.8%}
.rth-sheen{position:absolute;inset:0;border-radius:inherit;pointer-events:none;background:linear-gradient(90deg,rgba(var(--rim),.10) 0%,rgba(255,255,255,0) 18%,rgba(255,255,255,.045) 30%,rgba(255,255,255,0) 42%,rgba(255,255,255,0) 78%,rgba(var(--rim),.08) 100%)}
.rth-plinth{width:50%;height:10px;margin:7px auto 0;border-radius:0 0 50% 50%/0 0 100% 100%;background:linear-gradient(#1b1c27,#0b0b11);box-shadow:inset 0 0 0 1px rgba(255,255,255,.05),0 8px 18px rgba(var(--rim),calc(var(--glow)*.25))}
.rth-tube{outline-offset:4px}
.rth-stage::before{content:"";position:absolute;left:8%;right:8%;top:6%;height:72%;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(var(--glow)*.35)),rgba(var(--rim),0) 70%);filter:blur(18px);z-index:-1;transition:opacity .8s}
.rth-say{min-height:76px;margin-top:10px;padding:0 6px;text-align:center}
.rth-word{display:block;margin:10px auto 0;width:max-content;padding:2px 9px;border-radius:9px;font-size:10.5px;font-weight:650;letter-spacing:.09em;text-transform:uppercase;color:var(--rth-muted);background:var(--rth-soft);border:1px solid var(--rth-line)}
.rth-word[data-state=replying],.rth-word[data-state=working],.rth-word[data-state=speaking]{color:var(--rth-accent);border-color:color-mix(in oklab,var(--rth-accent) 35%,transparent)}
.rth-word[data-state=unknown]{border-style:dashed}
.rth-cp-real{display:inline-flex;align-items:center;gap:5px;border-style:solid}
.rth-cp-sw{width:7px;height:7px;border-radius:50%;flex:none}
.rth-cp-bar{position:relative;width:26px;height:3px;border-radius:2px;background:var(--rth-line);overflow:hidden}
.rth-cp-bar i{position:absolute;left:0;top:0;bottom:0;background:var(--rth-accent)}
.rth-tiers{display:inline-flex;gap:3px;vertical-align:middle}
.rth-tiers i{width:9px;height:9px;border-radius:50%}
.rth-history{list-style:none;margin:0 0 6px;padding:0;display:flex;flex-direction:column;gap:6px;font-size:12px}
.rth-cp{font-size:10px;font-weight:600;letter-spacing:.04em;color:var(--rth-faint);padding:1px 6px;border:1px dashed var(--rth-line);border-radius:7px;margin-left:6px}
/* The production queue: one square slot per thing running, docked to the capsule's right. */
.rth-queue{position:absolute;right:-2px;top:52px;display:flex;flex-direction:column;gap:6px;z-index:2}
.rth-slot{position:relative;width:30px;height:36px;padding:0;border-radius:8px;border:1px solid rgba(98,230,242,.35);background:#06070d;color:#bfeff5;cursor:pointer;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;box-shadow:0 0 10px rgba(98,230,242,.12);transition:opacity .9s var(--rth-ease),transform .9s var(--rth-ease),border-color .3s}
.rth-slot svg{width:14px;height:14px}
.rth-slot-t{font-size:8.5px;font-variant-numeric:tabular-nums;line-height:1;color:#8fb8c2}
.rth-slot-bar{position:absolute;left:3px;right:3px;bottom:2px;height:2px;border-radius:1px;overflow:hidden;background:rgba(98,230,242,.12)}
.rth-slot-bar i{position:absolute;top:0;bottom:0;width:40%;left:0;background:linear-gradient(90deg,transparent,rgba(98,230,242,.9),transparent);animation:rth-shimmer 1.6s linear infinite}
@keyframes rth-shimmer{from{transform:translateX(-100%)}to{transform:translateX(250%)}}
.rth-slot[data-phase=ended]{opacity:0;transform:scale(.85)}
.rth-slot[data-phase=ended] .rth-slot-bar i{animation:none}
.rth-slot[data-phase=lost],.rth-slot[data-unsure=true]{border-style:dashed;border-color:rgba(255,197,110,.55);color:#ffc56e}
.rth-slot[data-phase=lost] .rth-slot-bar,.rth-slot[data-unsure=true] .rth-slot-bar i{display:none}
.rth-slot-more{width:30px;text-align:center;font-size:10px;font-weight:650;color:var(--rth-muted)}
.rth-slot-pop{position:absolute;right:38px;top:0;width:max-content;max-width:190px;padding:6px 9px;border-radius:8px;background:light-dark(#fff,#0f2129);color:var(--rth-fg);border:1px solid var(--rth-line);font-size:11.5px;line-height:1.35;box-shadow:0 8px 22px -10px rgba(0,0,0,.45);text-align:left;cursor:default}
.rth-slot-pop button{all:unset;cursor:pointer;color:var(--rth-bad);font-size:11px;margin-top:3px;display:inline-block}
.rth-slot-pop button:focus-visible{outline:2px solid var(--rth-accent)}
.rth-head{font-size:17px;font-weight:620;letter-spacing:-.005em;color:var(--rth-fg);transition:color .3s}
.rth-stage[data-voice=unknown] .rth-head{color:var(--rth-muted)}
.rth-line{margin-top:4px;font-size:12.5px;color:var(--rth-muted);min-height:19px;transition:opacity .3s}
.rth-line[data-tone=accent]{color:var(--rth-accent)}
.rth-line[data-tone=warn]{color:var(--rth-warn)}
.rth-line[data-tone=bad]{color:var(--rth-bad)}
.rth-hint{margin-top:8px;font-size:11.5px;color:var(--rth-faint);opacity:0;transition:opacity .3s}
.rth-stage[data-voice=off] .rth-hint,.rth-stage[data-voice=error] .rth-hint,.rth-stage:hover .rth-hint,.rth-stage:focus-within .rth-hint{opacity:1}
.rth-pres-more{width:100%;max-width:300px}
.rth-pres-more .rth-fold{border-top:0}
.rth-pres-more .rth-fold .rth-fold{border-top:1px solid var(--rth-line);margin-top:10px}
.rth-tech{display:flex;flex-direction:column;gap:6px}
.rth-tech ul{margin:0;padding-left:16px}
`

/** The cosmic points from the server's read-only CP ledger route; null until read. */
function useGrowth() {
  const [g, setG] = useState(null)
  useEffect(() => {
    let alive = true
    const read = async () => {
      try {
        const r = await fetch('api/rth-presence/growth', { cache: 'no-store', signal: AbortSignal.timeout(4000) })
        const body = r.ok ? await r.json() : { unavailable: `route ${r.status}` }
        if (alive) setG(body)
      } catch (e) {
        if (alive) setG({ unavailable: String(e?.message ?? e) })
      }
    }
    void read()
    const t = setInterval(read, 60_000) // the ledger changes only when a build is accepted
    return () => { alive = false; clearInterval(t) }
  }, [])
  return g
}

const SWATCH = { cyan: '#5fe3ef', magenta: '#ff5fd2', violet: '#a98bff', amber: '#ffc56e', rose: '#ff8fa8', mint: '#7ff0c0', 'ice-blue': '#b8e6ff', coral: '#ff8f6a', lime: '#c8f06e', 'white-gold': '#ffe9b8' }

/** The growth chip: stage, level, CP and the progress to the next level. */
function GrowthChip({ g }) {
  if (!g) return null
  if (g.unavailable || g.state === 'unavailable') return <span className="rth-cp" data-rth="avatar-cp" data-state="unavailable" title={`growth not available: ${g.unavailable ?? g.why}`}>growth unavailable</span>
  if (g.state !== 'approved') return <span className="rth-cp" data-rth="avatar-cp" data-state="unapproved" title="Cosmic points start counting once the owner approves the policy">growth: awaiting approval</span>
  const pct = ((10 - g.nextLevelIn) / 10) * 100
  return (
    <span className="rth-cp rth-cp-real" data-rth="avatar-cp" title={`${g.cp} cosmic points from ${g.entries.length} accepted build(s); next level in ${g.nextLevelIn} CP`}>
      <span className="rth-cp-sw" style={{ background: SWATCH[g.palette.at(-1)] }} aria-hidden="true" />
      {g.stage}{g.availableStage ? ` (${g.availableStage} available)` : ''} · L{g.level} · {g.cp} CP
      <span className="rth-cp-bar" aria-hidden="true"><i style={{ width: `${pct}%` }} /></span>
    </span>
  )
}

/** The growth detail: what earned the points (the ledger's evidence), tiers, what is next. */
function Growth({ g }) {
  if (!g) return <div className="rth-faint">Reading…</div>
  if (g.unavailable || g.state === 'unavailable') return <div className="rth-faint" data-rth="avatar-growth" data-state="unavailable">Growth can't be read right now ({g.unavailable ?? g.why}). Nothing is assumed.</div>
  if (g.state !== 'approved') return <div className="rth-faint" data-rth="avatar-growth" data-state="unapproved">Cosmic points aren't counting yet: the policy and its first entry wait for the owner's approval ({g.source}). Until then RTH shows no level or points.</div>
  return (
    <div className="rth-growth" data-rth="avatar-growth">
      <dl className="rth-facts">
        <dt>Stage</dt><dd>{g.stage}{g.availableStage ? ` · ${g.availableStage} is available (evolving is an explicit choice)` : g.nextStage ? ` (next: ${g.nextStage.name} at ${g.nextStage.cp} CP)` : ''}</dd>
        <dt>Level</dt><dd>{g.level} · next in {g.nextLevelIn} CP (one accepted build)</dd>
        <dt>Cosmic points</dt><dd>{g.cp} CP = {g.grains.toLocaleString('en-US')} grains of dust</dd>
        <dt>Colours</dt><dd><span className="rth-tiers">{g.palette.map((c) => <i key={c} title={c} style={{ background: SWATCH[c] }} />)}</span> {g.colours} of 10{g.nextColourAtLevel ? ` · next at level ${g.nextColourAtLevel}` : ''}</dd>
      </dl>
      <div className="rth-faint" style={{ margin: '8px 0 4px' }}>What earned them (+10 per activated build; it starts at 100)</div>
      {g.entries.length ? (
        <ul className="rth-history">
          {g.entries.map((e, i) => <li key={i}><span className="rth-num">{e.at.slice(0, 10)}</span> · +{e.cp} · {e.build}<div className="rth-mono rth-faint">{e.evidence.join(' · ')}</div></li>)}
        </ul>
      ) : <div className="rth-faint">No accepted build yet.</div>}
      <div className="rth-mono rth-faint">source: {g.source} · policy {g.policy?.id} v{g.policy?.version}{g.notes?.length ? ` · ${g.notes.length} ledger note(s)` : ''}{g.incompleteTail ? ' · an incomplete last line was ignored' : ''}</div>
    </div>
  )
}

const SlotIcon = ({ kind, phase }) => phase === 'lost'
  ? <svg viewBox="0 0 16 16" aria-hidden="true"><text x="8" y="12" textAnchor="middle" fontSize="12" fontWeight="700" fill="currentColor">?</text></svg>
  : kind === 'reply'
    ? <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true"><path d="M3 4.5h10M3 8h7M3 11.5h4.5" /></svg>
    : <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M8 2.5v7M5 6.5l3 3 3-3M3 12.5h10" /></svg>

/** The queue's state, stepped on every presence view; redraws while something runs or settles. */
function useQueue(view) {
  const q = useRef({ slots: [] })
  const seen = useRef(null)
  if (seen.current !== view) {
    q.current = stepQueue(q.current, view, Date.now())
    seen.current = view
  }
  const [, tick] = useReducer((n) => n + 1, 0)
  const running = q.current.slots.some((x) => x.phase === 'running')
  const settle = nextQueueChange(q.current)
  useEffect(() => {
    const timers = []
    if (running) timers.push(setInterval(tick, 1000)) // elapsed time only; stops when nothing runs
    if (settle) timers.push(setTimeout(() => { q.current = stepQueue(q.current, seen.current, Date.now()); tick() }, Math.max(0, settle - Date.now()) + 20))
    return () => timers.forEach((t) => clearInterval(t))
  }, [running, settle])
  return q.current
}

/** The RTS-style production queue: only what is observed running; empty means nothing shows. */
function Queue({ queue }) {
  const [open, setOpen] = useState(null)
  const now = Date.now()
  const slots = queue.slots
  if (!slots.length) return null
  const shown = slots.slice(0, MAX_SLOTS)
  return (
    <div className="rth-queue" data-rth="queue" role="list" aria-label="Running now">
      {shown.map((s) => {
        const line = slotLine(s, now)
        const age = Math.max(0, Math.round((now - s.since) / 1000))
        return (
          <div key={s.id} role="listitem" style={{ position: 'relative' }}>
            <button type="button" className="rth-slot" data-rth="queue-slot" data-kind={s.kind} data-phase={s.phase} data-unsure={String(Boolean(s.unsure))} aria-label={line} aria-expanded={open === s.id} onClick={() => setOpen(open === s.id ? null : s.id)}>
              <SlotIcon kind={s.kind} phase={s.phase} />
              <span className="rth-slot-t">{s.phase === 'lost' ? '' : age < 60 ? `${age}s` : `${Math.round(age / 60)}m`}</span>
              <span className="rth-slot-bar" aria-hidden="true"><i /></span>
            </button>
            {open === s.id ? (
              <div className="rth-slot-pop" data-rth="queue-pop">
                <div>{line}</div>
              </div>
            ) : null}
          </div>
        )
      })}
      {slots.length > MAX_SLOTS ? <div className="rth-slot-more" data-rth="queue-more">+{slots.length - MAX_SLOTS}</div> : null}
    </div>
  )
}

/** Live plumbing for the technical fold: page visibility, the receipt's age, the renderer's frames. */
function TechLive({ voiceCtl, renderer }) {
  useNow(1000)
  const v = voiceCtl.view()
  const st = renderer.current?.stats?.() ?? null
  const age = v.readAt == null ? 'none yet' : `${((performance.now() - v.readAt) / 1000).toFixed(1)} s ago`
  return (
    <div className="rth-mono" data-rth="avatar-live">
      page {typeof document === 'undefined' ? '?' : document.visibilityState} · voice receipt read {age} · renderer {st ? `${st.renderer}, ${st.frames} frames, ${st.running ? `running at ${st.fps} fps` : 'paused'}` : 'none'}
    </div>
  )
}

/** One readable line of the voice receipt: actual and desired, apart (rth-host's words). */
function receiptLine(voice) {
  if (voice.actual === 'unknown') return `rth-host receipt: none fresh${voice.stale ? ' (stale)' : ''}`
  return `rth-host receipt: actual ${voice.actual} · desired ${voice.desired ?? '—'}`
}

function PresenceBody() {
  useStyle('rth-ui-base', BASE_CSS)
  useStyle('rth-ui-presence', PRESENCE_CSS)
  const { view: live, stream } = usePresence()
  const view = stream === 'open' && live ? live : STALE
  const reduced = useReducedMotion()
  const voiceCtl = useVoice()
  const voice = { ...voiceCtl.view() }
  const scene = sceneFor(view, voice)
  const words = presenceWords(view, voice)
  const queue = useQueue(view)
  const growthView = useGrowth()
  const approved = growthView?.state === 'approved'
  const cp = approved ? growthView.cp : CP_PREVIEW.cp // the start (100) until approved CP exists
  const tierColour = approved ? growthView.palette.at(-1) : 'cyan'
  const state = stateOf(view, voice, queue)
  const lang = langOf(globalThis.document?.documentElement?.lang || globalThis.navigator?.language)
  const statusId = useId()
  const canvas = useRef(null)
  const tube = useRef(null)
  const renderer = useRef(null)
  const visible = useVisible(tube)
  const visibleRef = useRef(visible)
  visibleRef.current = visible
  const sceneRef = useRef(scene)
  sceneRef.current = scene
  // The dust's grain count is the creature's CP x 100, capped by the tab's size, and
  // its colour the newest colour tier the levels have unlocked.
  useEffect(() => {
    const r = canvas.current.getBoundingClientRect()
    const { grains } = grainBudget(r.width, r.height, cp)
    renderer.current = createRenderer(canvas.current, { reducedMotion: reduced, grains, colour: tierColour })
    renderer.current.setVisible?.(visibleRef.current)
    renderer.current.draw(sceneRef.current)
    return () => renderer.current?.dispose()
  }, [cp, tierColour])
  // The app's quit: the native shell evaluates window.__rthGoodbye(ms) before it hides
  // the window; every mounted presence dissolves, and the count that started is returned.
  useEffect(() => {
    const byes = (globalThis.__rthGoodbyes ??= new Set())
    const bye = (ms) => renderer.current?.goodbye?.(ms) ?? false
    byes.add(bye)
    globalThis.__rthGoodbye ??= (ms) => [...globalThis.__rthGoodbyes].filter((f) => f(ms)).length
    return () => byes.delete(bye)
  }, [])
  useEffect(() => renderer.current?.setReducedMotion(reduced), [reduced])
  useEffect(() => renderer.current?.setVisible?.(visible), [visible])
  // Drawn on a scene change only (the key names everything the dust shows).
  useEffect(() => renderer.current?.draw(scene), [scene.key])
  const layers = view.layers
  const fault = view.faults?.at(-1)
  const tap = () => {
    if (voice.action) void voiceCtl.toggle()
  }
  const mute = () => {
    if (voice.mute.action !== null) void voiceCtl.mute()
  }
  // One quiet line: a pending tap first (it is not the state), then a failed request, then the
  // reason voice didn't wake, then what RTH is doing.
  const line = voice.settling ? { line: voice.settling, tone: 'accent' }
    : voice.note ? { line: voice.note, tone: 'bad' }
      : voice.reason ? { line: voice.reason, tone: 'warn' }
        // the state word and the queue already say a reply or a request is running
        : (state === 'replying' || state === 'working') && words?.tone === 'accent' ? null
          : words
  return (
    <div className="rth-ui rth-pres" data-rth="presence" data-pose={view.pose} data-scene={scene.form} data-voice-actual={voice.actual} data-voice-desired={voice.desired ?? ''}>
      <div className="rth-stage" data-rth="avatar-frame" data-voice={voice.read ? voice.actual : 'unknown'} data-presence={view.pose === 'unknown' ? 'unknown' : 'known'} data-alive={String(lifeFor(view, voice))}>
        <div className="rth-bar">
          <span className="rth-name"><Dot tone={layers?.link?.state === 'live' ? 'ok' : 'neutral'} /> Unfold<GrowthChip g={growthView} /></span>
          {/* The mic: the APPLIED mute fence from rth-host's receipt, apart from the creature. */}
          <button
            type="button"
            className="rth-mic"
            data-rth="avatar-mute"
            data-state={voice.mute.state}
            aria-label={voice.mute.label}
            aria-pressed={voice.mute.muted === null ? undefined : voice.mute.muted}
            aria-disabled={voice.mute.action === null}
            title={voice.mute.label}
            onClick={mute}
          >
            <MicGlyph state={voice.mute.state} />
            {MIC_WORD[voice.mute.state] ?? null}
          </button>
        </div>
        <button
          type="button"
          ref={tube}
          className="rth-tube"
          data-rth="avatar"
          data-voice={voice.actual}
          aria-label={voice.label}
          aria-describedby={statusId}
          aria-disabled={!voice.action}
          onClick={tap}
        >
          <span className="rth-pres-cap top" aria-hidden="true" />
          <canvas ref={canvas} data-rth="presence-canvas" aria-hidden="true" />
          <span className="rth-sheen" aria-hidden="true" />
          <span className="rth-pres-cap bot" aria-hidden="true" />
        </button>
        <div className="rth-plinth" aria-hidden="true" />
        <Queue queue={queue} />
        <span className="rth-word" data-rth="avatar-state" data-state={state}>{WORDS[lang][state]}</span>
        <div className="rth-say" id={statusId}>
          <div className="rth-head" data-rth="avatar-voice">{voice.status}</div>
          <div className="rth-line" data-rth="avatar-line" data-tone={line?.tone ?? 'faint'} role={voice.settling || voice.note ? 'status' : undefined}>{line?.line ?? ''}</div>
          {voice.hint ? <div className="rth-hint" data-rth="avatar-hint">{voice.hint}</div> : null}
        </div>
      </div>
      {/* Faults are announced politely, once per new fault; speaking and idle are not. */}
      <div aria-live="polite" className="rth-sr" data-rth="presence-faults" style={{ position: 'absolute', width: 1, height: 1, overflow: 'hidden', clipPath: 'inset(50%)' }}>
        {fault ? `Problem at ${clock(fault.at)}: ${fault.summary}` : ''}
      </div>
      <div className="rth-pres-more">
        <Fold id="presence:details" summary="Details" data-rth="presence-layers">
          <dl className="rth-facts">
            {presenceFacts(view, voice).map(([k, v]) => <Fragment key={k}><dt>{k}</dt><dd>{v}</dd></Fragment>)}
          </dl>
          <Fold id="presence:growth" summary="Growth">
            <Growth g={growthView} />
          </Fold>
          <Fold id="presence:technical" summary="Technical details">
            <div className="rth-tech rth-muted">
              <div className="rth-mono" data-rth="avatar-receipt">{receiptLine(voice)}{voice.title ? ` · ${voice.title}` : ''}</div>
              <div className="rth-mono" data-rth="presence-because">creature: {view.label} ({view.pose}) because {view.because}; scene {scene.form}{scene.face ? `/${scene.face}` : ''}</div>
              {layers ? (
                <ul className="rth-mono">
                  <li>{layers.link.label}</li>
                  <li>{layers.request.label}</li>
                  <li>{layers.audio.label}</li>
                  <li data-rth="presence-mic" data-state={layers.mic?.state ?? 'unknown'}>{layers.mic?.label ?? 'mic: unknown'}</li>
                </ul>
              ) : <div className="rth-mono">presence stream: {stream} (everything is unknown)</div>}
              <TechLive voiceCtl={voiceCtl} renderer={renderer} />
              <div>
                Voice asleep releases only the mic and speaker: RTH, its link and the conversation stay on. &ldquo;Speaking&rdquo; is
                drawn only from audio rth-host reports handing to the device. A message waiting for a reply is not RTH thinking.
                There is no listening face: nothing on this feed says someone is speaking to RTH.
              </div>
            </div>
          </Fold>
        </Fold>
      </div>
    </div>
  )
}
