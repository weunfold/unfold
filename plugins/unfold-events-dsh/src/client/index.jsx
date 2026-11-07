// @automagik/unfold-events-dsh, browser half: the "Unfold events" right-sidebar tab inside DSH's own web client.
// The two-stage path from upstream ui-sidebar-files, unmodified: the type into
// ctx.sidebarRightTabs, the body and chip title into the keyed `sidebar.right.pane.tab` and
// `sidebar.right.pane.tab.title` seats under the type's id. Extension priority, its own kind
// and id, no resource patterns: it claims no address, so it overrides no Bot or DSH tab.
//
// Data arrives by push only: one EventSource on the host's /api/unfold-events/stream per mounted
// body, a snapshot then increments. Closing the tab closes that stream; the host keeps
// ingesting. Rows are drawn by their plain `name` from the catalog; the UI names no kinds.
import { Fragment, useEffect, useId, useReducer, useRef, useState } from 'react'
import { BASE_CSS, Dot, Fold, clock, clockS, guarded, useNow, useStyle } from './ui.jsx'
import { CP_PREVIEW, createVoiceController, grainBudget, lifeFor, presenceFacts, presenceWords, readDelay, sceneFor, shouldRead, voiceKey } from '../avatar.js'
import { dustRenderer } from './dust-renderer.js'
import { MAX_SLOTS, WORDS, langOf, nextQueueChange, slotLine, stateOf, stepQueue } from '../queue.js'
// Defense in depth on the surface: every summary is masked with Mind's reviewed redactText before
// it is drawn (the server window redacts reply text, not every kind's summary; reported separately).
import { redactText } from './redact.js'

export const UNFOLD_EVENTS_ID = '@automagik/unfold-events-dsh'
export const UNFOLD_EVENTS_KIND = 'unfold.events'
// The presence: a second tab of this plugin, fed by its own pushed stream. It is
// Unfold's avatar: the cosmic-dust creature in a capsule on a phone-like frame;
// a click wakes or sleeps voice (the host's mic and speaker) through unfold-session's call route.
export const UNFOLD_PRESENCE_ID = '@automagik/unfold-events-dsh/presence'
export const UNFOLD_PRESENCE_KIND = 'unfold.presence'
let STREAM = 'api/unfold-events/stream'
let PRESENCE_STREAM = 'api/unfold-presence/stream'
let SESSION_ROUTE = 'api/unfold-session'
const SHOWN = 200
// The look is swappable behind this one factory (client/presence-renderer.js interface).
const createRenderer = dustRenderer

export const inject = ['slots', 'sidebarRightTabs', 'sidebarRight']

export async function apply(ctx) {
  const response = await fetch('api/unfold-events/config', { cache: 'no-store' })
  if (!response.ok) throw new Error(`Unfold configuration ${response.status}`)
  const config = await response.json()
  STREAM = `${config.route.replace(/^\//, '').replace(/\/$/, '')}/stream`
  PRESENCE_STREAM = `${config.presenceRoute.replace(/^\//, '').replace(/\/$/, '')}/stream`
  // Events can be installed without the session adapter. Its absence leaves voice unavailable.
  try {
    const sessionResponse = await fetch('api/unfold-session/config', { cache: 'no-store' })
    if (sessionResponse.ok) {
      const session = await sessionResponse.json()
      SESSION_ROUTE = session.route.replace(/^\//, '').replace(/\/$/, '')
    }
  } catch {}

  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: UNFOLD_EVENTS_ID,
    kind: UNFOLD_EVENTS_KIND,
    priority: 'extension',
    title: () => 'Events',
    guide: [{
      id: 'unfold-events',
      order: 90,
      title: () => 'Unfold events',
      description: () => 'Every event from the agent, as it arrives',
    }],
  }), 'unfold-events: tab type')
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: UNFOLD_PRESENCE_ID,
    kind: UNFOLD_PRESENCE_KIND,
    priority: 'extension',
    title: () => 'Unfold',
    guide: [{
      id: 'unfold-presence',
      order: 91,
      title: () => 'Unfold',
      description: () => 'Unfold itself: click to wake or sleep voice',
    }],
  }), 'unfold-presence: tab type')
  // On unload, close our open tabs in every Session: DSH's layout would otherwise keep each
  // one's chip (its captured title) with no body behind it.
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== UNFOLD_EVENTS_KIND && tab.kind !== UNFOLD_PRESENCE_KIND) continue
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId)
      } catch {} // the Session or tab already went away
    }
  }, 'unfold-events: close open tabs on unload')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab', key: UNFOLD_EVENTS_ID },
    guarded('unfold-events', EventsBody),
  )), 'unfold-events: tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab.title', key: UNFOLD_EVENTS_ID },
    EventsTitle,
  )), 'unfold-events: tab title')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab', key: UNFOLD_PRESENCE_ID },
    guarded('unfold-presence', PresenceBody),
  )), 'unfold-presence: tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab.title', key: UNFOLD_PRESENCE_ID },
    PresenceTitle,
  )), 'unfold-presence: tab title')
}

function EventsTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-unfold="events-title">{tab.title}</span>
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
    case 'unpublished':
      return 'agentd: not published now (Unfold does not hold the agent now); waiting'
    case 'unconfigured':
    case 'refused':
      return `agentd: ${link.detail}`
    default:
      return `agentd: ${link.state}`
  }
}

const EVENTS_CSS = `
.unfold-ev{display:flex;flex-direction:column;height:100%;overflow:hidden}
.unfold-ev-head{display:flex;align-items:center;gap:9px;padding:14px 16px 12px;border-bottom:1px solid var(--unfold-line)}
.unfold-ev-head b{font-size:14px;font-weight:600}
.unfold-ev-head[data-state=down] b{color:var(--unfold-muted);font-weight:500}
.unfold-ev-meta{margin-left:auto;font-size:11.5px;color:var(--unfold-faint)}
.unfold-ev-note{padding:8px 16px;font-size:11.5px;color:var(--unfold-warn);border-bottom:1px solid var(--unfold-line)}
.unfold-ev-list{flex:1 1 auto;overflow:auto;list-style:none;margin:0;padding:6px 8px 16px}
.unfold-ev-row{all:unset;box-sizing:border-box;display:grid;grid-template-columns:10px 1fr auto;gap:2px 10px;align-items:baseline;width:100%;padding:6px 8px;border-radius:8px;cursor:pointer}
.unfold-ev-row:hover{background:var(--unfold-hover)}
.unfold-ev-row:focus-visible{outline:2px solid var(--unfold-accent)}
.unfold-ev-row .unfold-dot{width:6px;height:6px;align-self:center}
.unfold-ev-name{font-size:12.5px;font-weight:550;min-width:0}
.unfold-ev-time{font-size:11px;color:var(--unfold-faint);font-variant-numeric:tabular-nums}
.unfold-ev-sum{grid-column:2/4;font-size:12px;color:var(--unfold-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.unfold-ev-row[aria-expanded=true] .unfold-ev-sum{white-space:normal;overflow-wrap:anywhere}
.unfold-ev-more{grid-column:2/4;margin-top:4px}
.unfold-ev-row[data-known=false] .unfold-ev-name{color:var(--unfold-warn)}
.unfold-ev-foot{padding:0 14px;border-top:1px solid var(--unfold-line)}
.unfold-ev-foot>.unfold-fold{border-top:0}
.unfold-ev-fresh{animation:unfold-in .5s var(--unfold-ease)}
@keyframes unfold-in{from{background:color-mix(in oklab,var(--unfold-accent) 12%,transparent)}to{background:transparent}}
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
  useStyle('unfold-ui-base', BASE_CSS)
  useStyle('unfold-ui-events', EVENTS_CSS)
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
    <div className="unfold-ui unfold-ev" data-unfold="events">
      <header className="unfold-ev-head" data-state={state} data-unfold="events-link" data-attaches={attaches}>
        <Dot tone={state === 'live' ? 'ok' : 'neutral'} />
        <b data-state={link?.state ?? 'none'}>{words}</b>
        <span className="unfold-ev-meta unfold-num" data-unfold="events-count">{rows.length} events</span>
      </header>
      {attaches > 1 ? <div className="unfold-ev-note">Reconnected. Events from while it was down are not shown here.</div> : null}
      <ol className="unfold-ev-list" data-unfold="events-rows">
        {shown.map((row) => {
          const [phase, tone] = PHASE[row.phase] ?? ['Other', 'neutral']
          const at = seen.current.get(row.key)
          const isOpen = open === row.key
          return (
            <li key={row.key}>
              <button type="button" className={`unfold-ev-row${at && Date.now() - at < 1500 ? ' unfold-ev-fresh' : ''}`} aria-expanded={isOpen} onClick={() => setOpen(isOpen ? null : row.key)} data-unfold="event-row" data-key={row.key} data-name={row.name} data-count={row.count} data-known={String(row.known)}>
                <Dot tone={tone} />
                <span className="unfold-ev-name">{row.known ? plainName(row.name) : 'Unrecognised event'}{row.count > 1 ? <span className="unfold-faint"> ×{row.count}</span> : null}</span>
                <span className="unfold-ev-time">{at ? clockS(at) : ''}</span>
                {row.summary ? <span className="unfold-ev-sum">{redactText(row.summary)}</span> : null}
                {isOpen ? (
                  <span className="unfold-ev-more unfold-mono unfold-faint">
                    {phase} · {row.wire}{row.known ? '' : ' (not in the catalog)'} · {row.key}{row.cause ? ` · caused by ${row.cause}` : ''}{at ? ` · seen ${clockS(at)}` : ' · seen before this tab opened'}
                  </span>
                ) : null}
              </button>
            </li>
          )
        })}
        {!shown.length ? <li className="unfold-faint" style={{ padding: 16, textAlign: 'center' }}>No events yet.</li> : null}
      </ol>
      <footer className="unfold-ev-foot">
        <Fold id="events:about" summary="About this list">
          <div className="unfold-faint" style={{ display: 'flex', flexDirection: 'column', gap: 6, fontSize: 12 }}>
            <div>Everything the agent sends to Unfold's read-only observer, newest first, named from the event catalog. Mind tells the same story in plain sentences.</div>
            {source && !source.host_frames ? <div>This is the live feed only: what the host sends is not shown, and nothing is replayed, so some causes stay unknown.</div> : null}
            <div className="unfold-mono" data-unfold="events-stream" data-state={stream}>{linkText(link, attaches)} · tab stream {stream}</div>
            {history.length ? (
              <ul data-unfold="events-link-history" className="unfold-mono" style={{ margin: 0, paddingLeft: 16 }}>
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
  return <span data-unfold="presence-title">{tab.title}</span>
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
 * The voice half: one controller per mounted body (bounded, ordered reads of unfold-session's
 * `api/unfold-session/call`, the path the composer button used). Reads only: mounting, re-rendering,
 * a hidden page coming back and presence events never send a request; a click or the mute glyph
 * does. Re-renders only when what the tab shows changes (a 500 ms clock expires leases and
 * pending clicks with no new answer; reads are fast while something settles, slow otherwise, and
 * paused while the page is hidden).
 */
function useVoice() {
  const [, force] = useReducer((n) => n + 1, 0)
  const ctl = useRef(null)
  const make = () => createVoiceController({ fetch: (...a) => fetch(...a), onChange: force, callUrl: `${SESSION_ROUTE}/call`, muteUrl: `${SESSION_ROUTE}/mute` })
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

const DEBUG = (globalThis.__unfoldAvatar ??= { live: 0, created: 0, frames: 0, rafs: 0, timeouts: 0, observers: 0 })
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
.unfold-pres{display:flex;flex-direction:column;align-items:center;gap:14px;padding:14px 16px 22px}
/* No device frame: the creature floats in a luminous capsule in the tab itself.
   The capsule is dark glass in both themes (the dust needs the dark); the words follow the theme. */
.unfold-stage{--glow:.12;--rim:98,230,242;position:relative;isolation:isolate;width:100%;max-width:300px;display:flex;flex-direction:column;align-items:stretch}
.unfold-stage[data-voice=active]{--glow:.34}
.unfold-stage[data-voice=starting]{--glow:.24}
.unfold-stage[data-voice=ending]{--glow:.16}
.unfold-stage[data-voice=unknown],.unfold-stage[data-presence=unknown]{--glow:.05;--rim:150,156,170}
.unfold-bar{display:flex;align-items:center;justify-content:space-between;height:28px;padding:0 2px}
.unfold-name{display:flex;align-items:center;gap:7px;font-size:12px;font-weight:600;letter-spacing:.04em;color:var(--unfold-muted)}
.unfold-mic{display:inline-flex;align-items:center;gap:5px;height:26px;min-width:26px;padding:0 7px;border-radius:13px;border:0;background:transparent;color:var(--unfold-muted);cursor:pointer;font-size:11px;font-weight:600;transition:background .2s,color .2s}
.unfold-mic:hover{background:var(--unfold-hover);color:var(--unfold-fg)}
.unfold-mic[data-state=muted],.unfold-mic[data-state=unmuting]{color:var(--unfold-warn);background:color-mix(in oklab,var(--unfold-warn) 13%,transparent)}
.unfold-mic[data-state=unknown]{color:var(--unfold-faint)}
.unfold-mic[aria-disabled=true]{cursor:default}
.unfold-mic[aria-disabled=true]:hover{background:transparent}
.unfold-tube{position:relative;display:block;width:76%;margin:10px auto 0;aspect-ratio:3/4.7;padding:0;border:0;border-radius:50%/19%;cursor:pointer;background:#04040a;
 box-shadow:inset 0 0 0 1px rgba(var(--rim),.26),inset 0 0 34px rgba(var(--rim),calc(var(--glow)*.8)),0 0 38px rgba(var(--rim),calc(var(--glow)*.55));transition:box-shadow .8s var(--unfold-ease),transform .25s var(--unfold-ease)}
.unfold-tube:hover{transform:translateY(-1px)}
.unfold-tube:active{transform:scale(.985)}
.unfold-tube[aria-disabled=true]{cursor:default;transform:none}
.unfold-tube::after{content:"";position:absolute;inset:0;border-radius:inherit;pointer-events:none;opacity:0;box-shadow:0 0 54px rgba(var(--rim),.30);transition:opacity .8s}
.unfold-stage[data-alive=true] .unfold-tube::after{animation:unfold-breathe 5s ease-in-out infinite}
@keyframes unfold-breathe{0%,100%{opacity:.15}50%{opacity:.6}}
.unfold-tube canvas{position:absolute;inset:5% 6%;width:88%;height:90%;border-radius:50%/18%;display:block}
.unfold-pres .unfold-pres-cap{position:absolute;left:22%;right:22%;height:8px;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(.22 + var(--glow))),rgba(var(--rim),0));transition:opacity .8s}
.unfold-pres .unfold-pres-cap.top{top:.8%}.unfold-pres .unfold-pres-cap.bot{bottom:.8%}
.unfold-sheen{position:absolute;inset:0;border-radius:inherit;pointer-events:none;background:linear-gradient(90deg,rgba(var(--rim),.10) 0%,rgba(255,255,255,0) 18%,rgba(255,255,255,.045) 30%,rgba(255,255,255,0) 42%,rgba(255,255,255,0) 78%,rgba(var(--rim),.08) 100%)}
.unfold-plinth{width:50%;height:10px;margin:7px auto 0;border-radius:0 0 50% 50%/0 0 100% 100%;background:linear-gradient(#1b1c27,#0b0b11);box-shadow:inset 0 0 0 1px rgba(255,255,255,.05),0 8px 18px rgba(var(--rim),calc(var(--glow)*.25))}
.unfold-tube{outline-offset:4px}
.unfold-stage::before{content:"";position:absolute;left:8%;right:8%;top:6%;height:72%;border-radius:50%;pointer-events:none;background:radial-gradient(50% 50% at 50% 50%,rgba(var(--rim),calc(var(--glow)*.35)),rgba(var(--rim),0) 70%);filter:blur(18px);z-index:-1;transition:opacity .8s}
.unfold-say{min-height:76px;margin-top:10px;padding:0 6px;text-align:center}
.unfold-word{display:block;margin:10px auto 0;width:max-content;padding:2px 9px;border-radius:9px;font-size:10.5px;font-weight:650;letter-spacing:.09em;text-transform:uppercase;color:var(--unfold-muted);background:var(--unfold-soft);border:1px solid var(--unfold-line)}
.unfold-word[data-state=replying],.unfold-word[data-state=working],.unfold-word[data-state=speaking]{color:var(--unfold-accent);border-color:color-mix(in oklab,var(--unfold-accent) 35%,transparent)}
.unfold-word[data-state=unknown]{border-style:dashed}
/* The production queue: one square slot per thing running, docked to the capsule's right. */
.unfold-queue{position:absolute;right:-2px;top:52px;display:flex;flex-direction:column;gap:6px;z-index:2}
.unfold-slot{position:relative;width:30px;height:36px;padding:0;border-radius:8px;border:1px solid rgba(98,230,242,.35);background:#06070d;color:#bfeff5;cursor:pointer;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;box-shadow:0 0 10px rgba(98,230,242,.12);transition:opacity .9s var(--unfold-ease),transform .9s var(--unfold-ease),border-color .3s}
.unfold-slot svg{width:14px;height:14px}
.unfold-slot-t{font-size:8.5px;font-variant-numeric:tabular-nums;line-height:1;color:#8fb8c2}
.unfold-slot-bar{position:absolute;left:3px;right:3px;bottom:2px;height:2px;border-radius:1px;overflow:hidden;background:rgba(98,230,242,.12)}
.unfold-slot-bar i{position:absolute;top:0;bottom:0;width:40%;left:0;background:linear-gradient(90deg,transparent,rgba(98,230,242,.9),transparent);animation:unfold-shimmer 1.6s linear infinite}
@keyframes unfold-shimmer{from{transform:translateX(-100%)}to{transform:translateX(250%)}}
.unfold-slot[data-phase=ended]{opacity:0;transform:scale(.85)}
.unfold-slot[data-phase=ended] .unfold-slot-bar i{animation:none}
.unfold-slot[data-phase=lost],.unfold-slot[data-unsure=true]{border-style:dashed;border-color:rgba(255,197,110,.55);color:#ffc56e}
.unfold-slot[data-phase=lost] .unfold-slot-bar,.unfold-slot[data-unsure=true] .unfold-slot-bar i{display:none}
.unfold-slot-more{width:30px;text-align:center;font-size:10px;font-weight:650;color:var(--unfold-muted)}
.unfold-slot-pop{position:absolute;right:38px;top:0;width:max-content;max-width:190px;padding:6px 9px;border-radius:8px;background:light-dark(#fff,#0f2129);color:var(--unfold-fg);border:1px solid var(--unfold-line);font-size:11.5px;line-height:1.35;box-shadow:0 8px 22px -10px rgba(0,0,0,.45);text-align:left;cursor:default}
.unfold-slot-pop button{all:unset;cursor:pointer;color:var(--unfold-bad);font-size:11px;margin-top:3px;display:inline-block}
.unfold-slot-pop button:focus-visible{outline:2px solid var(--unfold-accent)}
.unfold-head{font-size:17px;font-weight:620;letter-spacing:-.005em;color:var(--unfold-fg);transition:color .3s}
.unfold-stage[data-voice=unknown] .unfold-head{color:var(--unfold-muted)}
.unfold-line{margin-top:4px;font-size:12.5px;color:var(--unfold-muted);min-height:19px;transition:opacity .3s}
.unfold-line[data-tone=accent]{color:var(--unfold-accent)}
.unfold-line[data-tone=warn]{color:var(--unfold-warn)}
.unfold-line[data-tone=bad]{color:var(--unfold-bad)}
.unfold-hint{margin-top:8px;font-size:11.5px;color:var(--unfold-faint);opacity:0;transition:opacity .3s}
.unfold-stage[data-voice=off] .unfold-hint,.unfold-stage[data-voice=error] .unfold-hint,.unfold-stage:hover .unfold-hint,.unfold-stage:focus-within .unfold-hint{opacity:1}
.unfold-pres-more{width:100%;max-width:300px}
.unfold-pres-more .unfold-fold{border-top:0}
.unfold-pres-more .unfold-fold .unfold-fold{border-top:1px solid var(--unfold-line);margin-top:10px}
.unfold-tech{display:flex;flex-direction:column;gap:6px}
.unfold-tech ul{margin:0;padding-left:16px}
`

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
    <div className="unfold-queue" data-unfold="queue" role="list" aria-label="Running now">
      {shown.map((s) => {
        const line = slotLine(s, now)
        const age = Math.max(0, Math.round((now - s.since) / 1000))
        return (
          <div key={s.id} role="listitem" style={{ position: 'relative' }}>
            <button type="button" className="unfold-slot" data-unfold="queue-slot" data-kind={s.kind} data-phase={s.phase} data-unsure={String(Boolean(s.unsure))} aria-label={line} aria-expanded={open === s.id} onClick={() => setOpen(open === s.id ? null : s.id)}>
              <SlotIcon kind={s.kind} phase={s.phase} />
              <span className="unfold-slot-t">{s.phase === 'lost' ? '' : age < 60 ? `${age}s` : `${Math.round(age / 60)}m`}</span>
              <span className="unfold-slot-bar" aria-hidden="true"><i /></span>
            </button>
            {open === s.id ? (
              <div className="unfold-slot-pop" data-unfold="queue-pop">
                <div>{line}</div>
              </div>
            ) : null}
          </div>
        )
      })}
      {slots.length > MAX_SLOTS ? <div className="unfold-slot-more" data-unfold="queue-more">+{slots.length - MAX_SLOTS}</div> : null}
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
    <div className="unfold-mono" data-unfold="avatar-live">
      page {typeof document === 'undefined' ? '?' : document.visibilityState} · voice receipt read {age} · renderer {st ? `${st.renderer}, ${st.frames} frames, ${st.running ? `running at ${st.fps} fps` : 'paused'}` : 'none'}
    </div>
  )
}

/** One readable line of the voice receipt: actual and desired, apart (the host's words). */
function receiptLine(voice) {
  if (voice.actual === 'unknown') return `the host receipt: none fresh${voice.stale ? ' (stale)' : ''}`
  return `the host receipt: actual ${voice.actual} · desired ${voice.desired ?? '—'}`
}

function PresenceBody() {
  useStyle('unfold-ui-base', BASE_CSS)
  useStyle('unfold-ui-presence', PRESENCE_CSS)
  const { view: live, stream } = usePresence()
  const view = stream === 'open' && live ? live : STALE
  const reduced = useReducedMotion()
  const voiceCtl = useVoice()
  const voice = { ...voiceCtl.view() }
  const scene = sceneFor(view, voice)
  const words = presenceWords(view, voice)
  const queue = useQueue(view)
  const cp = CP_PREVIEW.cp // the start (100): no source of earned points in this plugin
  const tierColour = 'cyan'
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
  // The app's quit: the native shell evaluates window.__unfoldGoodbye(ms) before it hides
  // the window; every mounted presence dissolves, and the count that started is returned.
  useEffect(() => {
    const byes = (globalThis.__unfoldGoodbyes ??= new Set())
    const bye = (ms) => renderer.current?.goodbye?.(ms) ?? false
    byes.add(bye)
    globalThis.__unfoldGoodbye ??= (ms) => [...globalThis.__unfoldGoodbyes].filter((f) => f(ms)).length
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
  // One quiet line: a pending click first (it is not the state), then a failed request, then the
  // reason voice didn't wake, then what Unfold is doing.
  const line = voice.settling ? { line: voice.settling, tone: 'accent' }
    : voice.note ? { line: voice.note, tone: 'bad' }
      : voice.reason ? { line: voice.reason, tone: 'warn' }
        // the state word and the queue already say a reply or a request is running
        : (state === 'replying' || state === 'working') && words?.tone === 'accent' ? null
          : words
  return (
    <div className="unfold-ui unfold-pres" data-unfold="presence" data-pose={view.pose} data-scene={scene.form} data-voice-actual={voice.actual} data-voice-desired={voice.desired ?? ''}>
      <div className="unfold-stage" data-unfold="avatar-frame" data-voice={voice.read ? voice.actual : 'unknown'} data-presence={view.pose === 'unknown' ? 'unknown' : 'known'} data-alive={String(lifeFor(view, voice))}>
        <div className="unfold-bar">
          <span className="unfold-name"><Dot tone={layers?.link?.state === 'live' ? 'ok' : 'neutral'} /> Unfold</span>
          {/* The mic: the APPLIED mute fence from the host's receipt, apart from the creature. */}
          <button
            type="button"
            className="unfold-mic"
            data-unfold="avatar-mute"
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
          className="unfold-tube"
          data-unfold="avatar"
          data-voice={voice.actual}
          aria-label={voice.label}
          aria-describedby={statusId}
          aria-disabled={!voice.action}
          onClick={tap}
        >
          <span className="unfold-pres-cap top" aria-hidden="true" />
          <canvas ref={canvas} data-unfold="presence-canvas" aria-hidden="true" />
          <span className="unfold-sheen" aria-hidden="true" />
          <span className="unfold-pres-cap bot" aria-hidden="true" />
        </button>
        <div className="unfold-plinth" aria-hidden="true" />
        <Queue queue={queue} />
        <span className="unfold-word" data-unfold="avatar-state" data-state={state}>{WORDS[lang][state]}</span>
        <div className="unfold-say" id={statusId}>
          <div className="unfold-head" data-unfold="avatar-voice">{voice.status}</div>
          <div className="unfold-line" data-unfold="avatar-line" data-tone={line?.tone ?? 'faint'} role={voice.settling || voice.note ? 'status' : undefined}>{line?.line ?? ''}</div>
          {voice.hint ? <div className="unfold-hint" data-unfold="avatar-hint">{voice.hint}</div> : null}
        </div>
      </div>
      {/* Faults are announced politely, once per new fault; speaking and idle are not. */}
      <div aria-live="polite" className="unfold-sr" data-unfold="presence-faults" style={{ position: 'absolute', width: 1, height: 1, overflow: 'hidden', clipPath: 'inset(50%)' }}>
        {fault ? `Problem at ${clock(fault.at)}: ${fault.summary}` : ''}
      </div>
      <div className="unfold-pres-more">
        <Fold id="presence:details" summary="Details" data-unfold="presence-layers">
          <dl className="unfold-facts">
            {presenceFacts(view, voice).map(([k, v]) => <Fragment key={k}><dt>{k}</dt><dd>{v}</dd></Fragment>)}
          </dl>
          <Fold id="presence:technical" summary="Technical details">
            <div className="unfold-tech unfold-muted">
              <div className="unfold-mono" data-unfold="avatar-receipt">{receiptLine(voice)}{voice.title ? ` · ${voice.title}` : ''}</div>
              <div className="unfold-mono" data-unfold="presence-because">creature: {view.label} ({view.pose}) because {view.because}; scene {scene.form}{scene.face ? `/${scene.face}` : ''}</div>
              {layers ? (
                <ul className="unfold-mono">
                  <li>{layers.link.label}</li>
                  <li>{layers.request.label}</li>
                  <li>{layers.audio.label}</li>
                  <li data-unfold="presence-mic" data-state={layers.mic?.state ?? 'unknown'}>{layers.mic?.label ?? 'mic: unknown'}</li>
                </ul>
              ) : <div className="unfold-mono">presence stream: {stream} (everything is unknown)</div>}
              <TechLive voiceCtl={voiceCtl} renderer={renderer} />
              <div>
                Voice asleep releases only the mic and speaker: Unfold, its link and the conversation stay on. &ldquo;Speaking&rdquo; is
                drawn only from audio the host reports handing to the device. A message waiting for a reply is not Unfold thinking.
                There is no listening face: nothing on this feed says someone is speaking to Unfold.
              </div>
            </div>
          </Fold>
        </Fold>
      </div>
    </div>
  )
}
