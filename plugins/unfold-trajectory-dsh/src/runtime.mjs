// Browser runtime for the Unfold trajectory fork: the one shared Mind subscription, the taxonomy badges
// and icons, and the Unfold style sheet. Bundled by scripts/build-client.mjs into the forked client (region
// "unfold/runtime") with React and DSH primitives taken from the module loader.
import { jsx, jsxs } from 'react/jsx-runtime'
import { DEFAULT_GROUPS, buildUnfoldTrajectory, filterTurns, newSince } from './adapter.mjs'
import { STATUS_EN, STATUS_ZH, stateOf, timelineDetail } from './status.mjs'
import { GROUPS, KINDS, LANES } from './taxonomy.mjs'
import { ICONS } from './icons.generated.mjs'
import { layoutClockOverview } from './clock-layout.mjs'

export { DEFAULT_GROUPS, GROUPS, KINDS, LANES, buildUnfoldTrajectory, filterTurns, layoutClockOverview, newSince, stateOf, timelineDetail }
// Unfold's own Session in DSH, and the Mind stream route, relative to the DSH page (the plugin
// opens no other connection, and names no host or port).
export let UNFOLD_SESSION = 'unfold-root'
export let MIND_STREAM = 'api/unfold-mind/stream'

/** Resolve the configured peers before any view or Bot button registers. */
export async function discover() {
  const read = async (path) => {
    const response = await fetch(new URL(path, document.baseURI), { cache: 'no-store' })
    if (!response.ok) throw new Error(`Unfold configuration ${response.status}`)
    return response.json()
  }
  const [session, mind] = await Promise.all([read('api/unfold-session/config'), read('api/unfold-mind/config')])
  UNFOLD_SESSION = session.sessionId
  MIND_STREAM = `${mind.route.replace(/^\//, '').replace(/\/$/, '')}/stream`
}

// ---------------------------------------------------------------- the shared subscription
// One EventSource on /api/unfold-mind/stream for every Unfold trajectory view in this page (ref-counted),
// so several views never open several streams. The reducer is Mind's own (same events, same
// semantics): this view reads Mind's projection and adds nothing to it.
const initial = { rows: [], episodes: {}, link: null, sources: null, heartbeatMs: 5000, stream: 'opening', lastStreamAt: null }
function reduce(s, event, data, at) {
  switch (event) {
    case 'snapshot': return { ...s, rows: data.rows, episodes: Object.fromEntries(data.episodes.map((e) => [e.id, e])), link: data.link, sources: data.sources, heartbeatMs: data.heartbeat_ms ?? s.heartbeatMs, stream: 'open', lastStreamAt: at }
    case 'row': return { ...s, rows: [...s.rows, data], lastStreamAt: at }
    case 'row-updated': return { ...s, rows: s.rows.map((r) => (r.id === data.id ? data : r)), lastStreamAt: at }
    case 'evicted': return { ...s, rows: s.rows.filter((r) => r.id !== data.id), lastStreamAt: at }
    case 'episode': return { ...s, episodes: { ...s.episodes, [data.id]: data }, lastStreamAt: at }
    case 'episode-removed': { const { [data.id]: _, ...rest } = s.episodes; return { ...s, episodes: rest, lastStreamAt: at } }
    case 'merged': { const { [data.from]: _, ...rest } = s.episodes; return { ...s, episodes: rest, rows: s.rows.map((r) => (r.episode === data.from ? { ...r, episode: data.into } : r)), lastStreamAt: at } }
    case 'link': return { ...s, link: data, lastStreamAt: at }
    case 'heartbeat': case 'heartbeat-mind': return { ...s, lastStreamAt: at }
    default: return s
  }
}
const EVENTS = ['snapshot', 'row', 'row-updated', 'evicted', 'episode', 'episode-removed', 'merged', 'link', 'heartbeat', 'heartbeat-mind']

// Freshness lease, Mind's rule (unfold-mind-dsh's display.js freshness, STREAM_BEATS
// = 3; index.mjs: "readers treat 3 missed beats as a lost stream"): 'live' only while the last
// frame is within three heartbeats by the browser clock; 'stale' past it with no new input needed;
// 'lost' on a transport error; 'opening' until the first snapshot. Derived at read time; a timer
// only wakes subscribers at the boundary. A test checks it against display.js.
const STREAM_BEATS = 3
function streamFreshness(s, now) {
  if (s.stream !== 'open' || s.lastStreamAt == null) return { state: s.stream === 'opening' ? 'opening' : 'lost', nextChange: null }
  const lease = s.heartbeatMs * STREAM_BEATS
  return now - s.lastStreamAt > lease ? { state: 'stale', nextChange: null } : { state: 'live', nextChange: s.lastStreamAt + lease + 1 }
}

let shared = null
function sharedStream() {
  if (shared) return shared
  let state = initial
  // Arrival bookkeeping: a monotonic number per row id as it reaches this page; rows the
  // server evicts drop out, so it is bounded by the projection window. Display positions are
  // never used as identities.
  let arrivals = new Map()
  let latestArrival = 0
  let derived = null
  let derivedFrom = null
  let derivedFresh = null
  const listeners = new Set()
  let source = null
  let refs = 0
  let retry = null
  let backoff = 1000
  let lease = null
  let pending = false
  const emit = () => {
    // Coalesce bursts into one notification per animation frame.
    if (pending) return
    pending = true
    const run = () => { pending = false; for (const l of [...listeners]) l() }
    if (typeof requestAnimationFrame === 'function' && typeof document !== 'undefined' && document.visibilityState !== 'hidden') requestAnimationFrame(run)
    else setTimeout(run, 50)
  }
  const arm = () => {
    clearTimeout(lease)
    lease = null
    const f = streamFreshness(state, Date.now())
    if (refs > 0 && f.nextChange != null) lease = setTimeout(() => { lease = null; emit(); arm() }, Math.max(0, f.nextChange - Date.now()))
  }
  const track = () => {
    const live = new Set()
    for (const r of state.rows) {
      live.add(r.id)
      if (!arrivals.has(r.id)) arrivals.set(r.id, ++latestArrival)
    }
    for (const id of [...arrivals.keys()]) if (!live.has(id)) arrivals.delete(id)
  }
  const open = () => {
    const s = (source = new EventSource(new URL(MIND_STREAM, document.baseURI)))
    for (const name of EVENTS) {
      source.addEventListener(name, (e) => {
        if (name === 'snapshot') backoff = 1000
        state = reduce(state, name, JSON.parse(e.data), Date.now())
        if (name === 'snapshot' || name === 'row' || name === 'row-updated' || name === 'evicted') track()
        arm()
        emit()
      })
    }
    source.addEventListener('resnapshot', () => { source.close(); state = { ...state, stream: 'opening' }; arm(); emit(); retry = setTimeout(open, 500) })
    source.onerror = () => {
      state = { ...state, stream: 'lost' }
      arm()
      emit()
      // A failure the browser does not retry (a 404 or 5xx while Mind reloads): reopen, backing
      // off as Mind's own client does.
      if (s.readyState === EventSource.CLOSED && refs > 0) {
        clearTimeout(retry)
        retry = setTimeout(open, backoff)
        backoff = Math.min(backoff * 2, 8000)
      }
    }
  }
  shared = {
    getSnapshot() {
      const fresh = streamFreshness(state, Date.now()).state
      if (derivedFrom !== state || derivedFresh !== fresh) {
        if (derivedFrom !== state) derived = { ...buildUnfoldTrajectory({ ...state, arrivals }), arrivals, latestArrival, link: state.link, sources: state.sources, rowCount: state.rows.length }
        derivedFrom = state
        derivedFresh = fresh
        derived = { ...derived, stream: fresh }
      }
      return derived
    },
    subscribe(listener) {
      listeners.add(listener)
      if (refs++ === 0) open()
      return () => {
        listeners.delete(listener)
        if (--refs === 0) {
          clearTimeout(retry)
          clearTimeout(lease)
          retry = null
          backoff = 1000
          lease = null
          source?.close()
          source = null
          state = initial
          arrivals = new Map()
          derivedFrom = null
        }
      }
    },
  }
  return shared
}

const NOT_UNFOLD = Object.freeze({ notUnfold: true, turns: [], cards: new Map(), lastIndex: 0, counts: {}, strip: null, stream: 'none', arrivals: new Map(), latestArrival: 0 })
const notUnfoldSource = { getSnapshot: () => NOT_UNFOLD, subscribe: () => () => {} }

/** The data source for one conversation session: Unfold's feed only in the Unfold session. */
export function sourceFor(sessionId) {
  return sessionId === UNFOLD_SESSION ? sharedStream() : notUnfoldSource
}

// ---------------------------------------------------------------- icons (pinned lucide-static release)
// The element lists come from lucide-static 0.460.0 (src/icons.generated.mjs, made by
// tools/pin-icons.mjs; ISC, portions Feather MIT: notices in third_party/ and the bundle header).
export function Icon({ name, size = 13 }) {
  const els = ICONS[name] ?? ICONS.radio
  return jsx('svg', { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round', 'aria-hidden': 'true', children: els.map(([tag, attrs], i) => jsx(tag, { ...attrs }, i)) })
}

/** The coloured kind badge, in upstream's kindTag slot (icon + short label; tooltip = full label). */
export function kindTag(unfoldKind, css, primitives) {
  const k = KINDS[unfoldKind] ?? KINDS.signal
  return jsxs('span', {
    className: `${css.kindTag} unfold-kind`,
    'data-unfold-kind': unfoldKind,
    'data-role-kind': k.upstream,
    children: [
      jsx(primitives.Tooltip, { label: k.label, side: 'right', children: jsx('span', { className: css.kindTagIcon, 'aria-hidden': 'true', children: jsx(Icon, { name: k.icon }) }) }),
      jsx('span', { className: css.kindTagLabel, children: k.short }),
    ],
  })
}

export function kindLabel(unfoldKind) {
  return KINDS[unfoldKind]?.label ?? null
}

// ---------------------------------------------------------------- styles (scoped under .unfold-trajectory)
const vars = (i) => Object.entries(KINDS).map(([k, v]) => `--unfold-k-${k}:${v.color[i]};`).join('')
export const CSS = `
.unfold-trajectory{${vars(0)}}
@media (prefers-color-scheme:dark){.unfold-trajectory{${vars(1)}}}
[data-theme=dark] .unfold-trajectory,.dark .unfold-trajectory{${vars(1)}}
.unfold-trajectory .unfold-kind{--k:var(--unfold-k-signal);color:var(--k);background:color-mix(in srgb,var(--k) 14%,transparent);border:1px solid color-mix(in srgb,var(--k) 38%,transparent)}
${Object.keys(KINDS).map((k) => `.unfold-trajectory .unfold-kind[data-unfold-kind=${k}]{--k:var(--unfold-k-${k})}`).join('\n')}
.unfold-trajectory .unfold-kind[data-unfold-kind=typed]{background:transparent}
.unfold-trajectory .unfold-kind[data-unfold-kind=gap]{border-style:dashed}
.unfold-trajectory .unfold-kind[data-unfold-kind=signal]{opacity:.75}
${Object.keys(KINDS).map((k) => `.unfold-trajectory [data-timeline-span=${k}]{--unfold-u:var(--unfold-k-${k});background:var(--unfold-k-${k})!important;opacity:.92}`).join('\n')}
.unfold-trajectory [data-unfold-span=instant]{width:3px!important;min-width:3px!important;border-radius:2px}
.unfold-trajectory [data-unfold-span^=unknown]{background:linear-gradient(var(--unfold-u,currentColor) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--unfold-u,currentColor) 75%,transparent) 0 2px,transparent 2px 4px)!important;opacity:.9;overflow:visible!important}
.unfold-trajectory [data-unfold-span=unknown]{width:14px!important;min-width:14px!important}
.unfold-trajectory [data-unfold-span^=unknown]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.unfold-trajectory [data-unfold-span=unknown-seq]::after{left:6px}
.unfold-trajectory .unfold-strip{display:flex;flex-wrap:wrap;gap:6px;align-items:center;padding:4px 10px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-secondary)}
.unfold-trajectory .unfold-chip{display:inline-flex;align-items:center;gap:4px;padding:1px 8px;border-radius:999px;border:1px solid color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 40%,transparent);background:color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 10%,transparent);color:var(--k,var(--dsw-alias-label-secondary));cursor:pointer;user-select:none}
.unfold-trajectory .unfold-chip[aria-pressed=false]{opacity:.45;background:transparent}
.unfold-trajectory .unfold-chip b{font-weight:600}
.unfold-trajectory .unfold-stat{color:var(--dsw-alias-label-caption)}
.unfold-trajectory:focus{outline:none}
.unfold-trajectory .unfold-clock{color:var(--dsw-alias-label-caption)}
.unfold-trajectory .unfold-live{display:inline-flex;align-items:center;gap:5px;height:18px;padding:0 8px;border-radius:999px;border:1px solid var(--dsw-alias-border-l2);background:transparent;color:var(--dsw-alias-label-secondary);font:inherit;cursor:pointer}
.unfold-trajectory .unfold-live-dot{width:6px;height:6px;border-radius:50%;background:var(--dsw-alias-label-caption)}
.unfold-trajectory .unfold-live[aria-pressed=true]{color:var(--unfold-k-unfold);border-color:currentColor}
.unfold-trajectory .unfold-live[aria-pressed=true] .unfold-live-dot{background:currentColor}
.unfold-trajectory:not([data-hidden]) .unfold-live[aria-pressed=true][data-stream=live] .unfold-live-dot{animation:unfold-pulse 1.6s ease-in-out infinite}
.unfold-trajectory .unfold-live[aria-pressed=true]:not([data-stream=live]){color:var(--dsw-alias-label-secondary);border-color:var(--dsw-alias-border-l2)}
.unfold-trajectory .unfold-conn{display:inline-flex;align-items:center;gap:4px;color:var(--dsw-alias-label-caption)}
.unfold-trajectory .unfold-conn[data-stream=stale],.unfold-trajectory .unfold-conn[data-stream=lost]{color:var(--unfold-k-fault)}
.unfold-trajectory .unfold-live[data-new=yes]{color:var(--unfold-k-unfold);border-color:currentColor;font-weight:600}
.unfold-trajectory .unfold-live:focus-visible,.unfold-trajectory .unfold-chip:focus-visible{outline:1px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
@keyframes unfold-pulse{50%{opacity:.35}}
@media (prefers-reduced-motion:reduce){.unfold-trajectory .unfold-live-dot{animation:none!important}}
.unfold-trajectory .unfold-stat strong{color:var(--dsw-alias-label-primary);font-weight:600}
.unfold-trajectory .unfold-sep{flex:1}
.unfold-trajectory .unfold-clock-ov{padding:4px 8px 6px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-caption);max-height:40vh;overflow:auto}
.unfold-trajectory .unfold-cg{padding:3px 0 2px}
.unfold-trajectory .unfold-cg+.unfold-cg{margin-top:6px;border-top:1px dashed var(--dsw-alias-border-l4);padding-top:5px}
.unfold-trajectory .unfold-cg-head{font-size:10px;line-height:14px;color:var(--dsw-alias-label-caption);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.unfold-trajectory .unfold-cg-head strong{font-weight:600;color:var(--dsw-alias-label-secondary)}
.unfold-trajectory .unfold-cg-body{display:flex;position:relative}
.unfold-trajectory .unfold-cg-labels{flex:none;width:38px;display:flex;flex-direction:column;padding-top:2px;font-size:9px;line-height:13px;color:var(--dsw-alias-label-caption)}
.unfold-trajectory .unfold-cg-track{position:relative;flex:1;min-width:0;margin-right:22px;border-left:.5px solid var(--dsw-alias-border-l2);border-bottom:.5px solid var(--dsw-alias-border-l2)}
.unfold-trajectory .unfold-cg-ticks{display:flex;height:11px}
.unfold-trajectory .unfold-cg-tick-pad{flex:none;width:38px}
.unfold-trajectory .unfold-cg-tickrow{position:relative;flex:1;margin-right:22px}
.unfold-trajectory .unfold-cg-tickrow span{position:absolute;top:0;transform:translateX(-50%);font-size:9px;line-height:11px;white-space:nowrap;color:var(--dsw-alias-label-caption)}
.unfold-trajectory .unfold-cg-tickrow span:first-child{transform:none}
.unfold-trajectory .unfold-ci{position:absolute;height:9px;min-width:3px;padding:0;border:0;border-radius:2px;background:var(--k);opacity:.92;cursor:pointer}
.unfold-trajectory .unfold-ci[data-shape=tick]{width:3px}
.unfold-trajectory .unfold-ci[data-shape=zero]{width:3px;outline:1px solid var(--k);outline-offset:1px}
.unfold-trajectory .unfold-ci[data-shape=start]{width:14px;background:linear-gradient(var(--k) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.unfold-trajectory .unfold-ci[data-shape=start]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.unfold-trajectory .unfold-ci[data-shape=end]{width:14px;margin-left:-14px;background:linear-gradient(var(--k) 0 0) right/3px 100% no-repeat,repeating-linear-gradient(45deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.unfold-trajectory .unfold-ci[data-shape=end]::before{content:"?";position:absolute;right:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.unfold-trajectory .unfold-ci[data-selected]{outline:2px solid var(--dsw-alias-label-primary);outline-offset:1px;z-index:2}
.unfold-trajectory .unfold-ci[data-match]{box-shadow:0 0 0 2px color-mix(in srgb,var(--dsw-alias-state-business-primary) 70%,transparent)}
.unfold-trajectory .unfold-ci:focus-visible{outline:2px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
.unfold-trajectory .unfold-cg-note{margin-top:4px;font-size:10px;line-height:13px;color:var(--dsw-alias-label-caption)}
.unfold-trajectory button[hidden]{display:none!important}
.unfold-trajectory .unfold-kind{gap:3px}
.unfold-trajectory .unfold-kind>[class*=kindTagIcon]{opacity:1;width:13px;transform:none}
.unfold-trajectory [class*="uftY0dWHa_turnLabel"]{font:600 9px/12px var(--ds-font-family-code,ui-monospace,monospace)}
.unfold-trajectory .unfold-card{display:inline-flex;gap:4px;align-items:center;margin-left:10px;vertical-align:middle;color:var(--unfold-k-unfold)}
.unfold-trajectory .unfold-card-chip{font:var(--dsw-font-xxxs-11);padding:0 6px;border-radius:999px;background:color-mix(in srgb,var(--unfold-k-unfold) 12%,transparent);color:var(--dsw-alias-label-secondary)}
.unfold-trajectory .unfold-card-chip[data-chip=interrupt]{color:var(--unfold-k-interrupt);background:color-mix(in srgb,var(--unfold-k-interrupt) 14%,transparent)}
.unfold-trajectory .unfold-card-chip[data-chip=unknown]{color:var(--unfold-k-gap);background:transparent;border:1px dashed color-mix(in srgb,var(--unfold-k-gap) 50%,transparent)}
.unfold-trajectory .unfold-notunfold{margin:auto;max-width:420px;text-align:center;color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);padding:24px}
`
export const GROUP_COLOR = { you: 'voice', unfold: 'unfold', voice: 'speaking', control: 'command', signals: 'signal', faults: 'fault' }

let styled = false
export function ensureStyle() {
  if (styled || typeof document === 'undefined') return
  styled = true
  const tag = document.createElement('style')
  tag.dataset.plugin = '@automagik/unfold-trajectory-dsh'
  tag.dataset.pluginCss = '@automagik/unfold-trajectory-dsh/unfold.css'
  tag.textContent = CSS
  document.head.appendChild(tag)
}

const CONNECTION = { live: 'connected', stale: 'stream stale (no heartbeat)', lost: 'stream lost', opening: 'connecting', none: '' }

/** The session strip + kind filter chips row (sections 4.2, 4.5). */
export function StripRow({ data, enabled, onToggle, live = false, newSincePause = 0, onLive }) {
  const s = data.strip
  const fmt = (ms) => (ms == null ? null : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`)
  return jsxs('div', {
    className: 'unfold-strip',
    'data-unfold': 'trajectory-strip',
    children: [
      ...GROUPS.map((g) => jsxs('span', {
        role: 'button',
        tabIndex: 0,
        className: 'unfold-chip',
        'aria-pressed': enabled.has(g.id),
        style: { '--k': `var(--unfold-k-${GROUP_COLOR[g.id]})` },
        onClick: () => onToggle(g.id),
        onKeyDown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle(g.id) } },
        children: [g.label, ' ', jsx('b', { children: data.counts[g.id] ?? 0 })],
      }, g.id)),
      jsx('span', { className: 'unfold-sep' }),
      jsx('span', {
        className: 'unfold-conn',
        'data-unfold': 'trajectory-connection',
        'data-stream': data.stream,
        title: 'The Mind stream this view reads: live only while frames arrive within three heartbeats',
        children: CONNECTION[data.stream] ?? data.stream,
      }),
      jsxs('button', {
        type: 'button',
        className: 'unfold-live',
        'data-unfold': 'trajectory-live',
        'aria-pressed': live,
        'data-stream': data.stream,
        'data-new': !live && newSincePause > 0 ? 'yes' : undefined,
        title: 'Follow the newest record (l). Scrolling away pauses it. Keys: j/k move, Enter inspect, / search, f filters, l live',
        onClick: onLive,
        children: [jsx('span', { className: 'unfold-live-dot', 'aria-hidden': true }), live ? (data.stream === 'live' ? 'Live' : 'Following') : newSincePause > 0 ? `Paused · ${newSincePause} new` : 'Paused'],
      }),
      s ? jsxs('span', { className: 'unfold-stat', children: [jsx('strong', { children: s.turns }), ' turns'] }) : null,
      s ? jsxs('span', { className: 'unfold-stat', title: `first text received (agent clock: command admitted -> first text); not provider TTFT. Coverage ${s.firstTextCoverage}.`, children: ['first text ', jsx('strong', { children: fmt(s.avgFirstTextMs) ?? 'not measured' }), s.avgFirstTextMs == null ? null : jsx('span', { className: 'unfold-clock', children: ' (agent clock)' })] }) : null,
      s ? jsxs('span', { className: 'unfold-stat', children: [jsx('strong', { children: s.interrupts }), s.interrupts === 1 ? ' interrupt' : ' interrupts'] }) : null,
      s ? jsxs('span', { className: 'unfold-stat', children: [jsx('strong', { children: s.faults }), s.faults === 1 ? ' fault' : ' faults'] }) : null,
      s ? jsx('span', { className: 'unfold-stat', title: `${s.usageReports} provider usage report(s) seen; they name a connection, not a reply`, children: 'tokens: not linked yet' }) : null,
    ],
  })
}

// ---------------------------------------------------------------- Duration overview
// One group per (producer clock, lifetime), each with its own labelled axis, origin, scale and
// ticks (src/clock-layout.mjs). Groups are separated and never aligned:
// equal x across groups means nothing. Unknowns are markers with fixed-size hatched tails.
const fmtDur = (ms) => (ms == null ? '' : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`)
const SHAPE_TEXT = { span: (it) => `measured ${fmtDur(it.durationMs)}`, zero: () => 'measured 0', start: (it) => `end not observed; duration unavailable${it.why ? ` (${it.why})` : ''}`, end: (it) => `start not observed; duration unavailable${it.why ? ` (${it.why})` : ''}`, tick: () => 'an observed instant' }
export function ClockOverview({ layout, selectedIndex, searchMatchIndexes, onSelect }) {
  const LANE_H = 13
  const off = Object.entries(layout.unplaced)
  const offTotal = off.reduce((a, [, n]) => a + n, 0)
  return jsxs('section', {
    className: 'unfold-clock-ov',
    'data-unfold': 'clock-overview',
    'aria-label': 'Duration overview: one group per producer clock and lifetime, each on its own axis; groups are not aligned with each other',
    children: [
      ...layout.groups.map((g) => jsxs('div', {
        className: 'unfold-cg',
        'data-clock': g.clock,
        'data-life': g.life,
        children: [
          jsxs('div', { className: 'unfold-cg-head', children: [jsx('strong', { children: g.title }), jsx('span', { children: ` · ${g.axisLabel} · spans ${fmtDur(g.extentMs)}` })] }),
          jsxs('div', {
            className: 'unfold-cg-body',
            style: { height: g.lanes.length * LANE_H + 4 },
            children: [
              jsx('div', { className: 'unfold-cg-labels', children: g.lanes.map((l) => jsx('span', { style: { height: LANE_H }, children: l.name }, l.lane)) }),
              jsx('div', {
                className: 'unfold-cg-track',
                children: g.items.map((it, n) => {
                  const row = g.lanes.findIndex((l) => l.lane === it.lane)
                  const k = KINDS[it.kind]
                  const what = SHAPE_TEXT[it.shape](it)
                  return jsx('button', {
                    type: 'button',
                    className: 'unfold-ci',
                    'data-kind': it.kind,
                    'data-shape': it.shape,
                    'data-selected': selectedIndex === it.index || undefined,
                    'data-match': searchMatchIndexes?.has(it.index) || undefined,
                    style: { left: `${it.x0 * 100}%`, width: it.shape === 'span' ? `max(2px, ${(it.x1 - it.x0) * 100}%)` : undefined, top: row * LANE_H + 2, '--k': `var(--unfold-k-${it.kind})` },
                    title: `${it.name} · ${what} · ${g.title}`,
                    'aria-label': `${k?.label ?? it.kind}: ${it.name}, ${what}`,
                    onClick: () => onSelect?.(it.index),
                  }, `${it.recordId}-${n}`)
                }),
              }),
            ],
          }),
          jsxs('div', { className: 'unfold-cg-ticks', children: [jsx('span', { className: 'unfold-cg-tick-pad' }), jsx('div', { className: 'unfold-cg-tickrow', children: g.ticks.map((t) => jsx('span', { style: { left: `${t.x * 100}%` }, children: t.label }, t.ms)) })] }),
        ],
      }, `${g.clock}|${g.life}`)),
      layout.groups.length > 1 ? jsx('div', { className: 'unfold-cg-note', children: 'Each group has its own clock, origin and scale. Positions in different groups are not comparable in time.' }) : null,
      offTotal > 0 ? jsx('div', { className: 'unfold-cg-note', 'data-unfold': 'clock-unplaced', title: off.map(([why, n]) => `${n}: ${why}`).join('\n'), children: `Not placed on a producer clock: ${off.map(([why, n]) => `${n} (${why})`).join('; ')}` }) : null,
      layout.groups.length === 0 ? jsx('div', { className: 'unfold-cg-note', children: 'Nothing with a valid producer time yet.' }) : null,
    ],
  })
}

// ---------------------------------------------------------------- turn cards, inspector, locale
/** The short label of a between-turns section (fits the turn-label column; "Between turns" overlapped the badge). */
export const BETWEEN = 'Between'
let cards = new Map()
export function setCards(next) { cards = next ?? new Map() }
/** The turn label stays short ("Turn N"); the card's chips ride on the turn's first row. */
export function cardLabel(turn) {
  return cards.get(turn)?.label
}
/** The turn card chips: every chip measured or absent, never guessed. */
export function cardChips(turn) {
  const c = cards.get(turn)
  if (!c || !c.chips.length) return null
  return jsxs('span', { className: 'unfold-card', 'data-unfold': 'turn-card', children: [jsx(Icon, { name: c.icon, size: 11 }), ...c.chips.map((chip, i) => jsx('span', { className: 'unfold-card-chip', 'data-chip': /interrupt/.test(chip) ? 'interrupt' : /not attributed/.test(chip) ? 'unknown' : 'm', children: chip }, i))] })
}

const fmtMs = (ms) => (ms == null ? null : ms < 1000 ? `${ms.toFixed(ms < 10 ? 2 : 0)} ms` : `${(ms / 1000).toFixed(2)} s`)
const dl = (key, dt, dd, extra = {}) => jsxs('div', { 'data-unfold-detail': key, children: [jsx('dt', { children: dt }), jsx('dd', { ...extra, children: dd })] }, key)

/** Inspector Summary rows for an Unfold record: named timing, clock, evidence ids. */
export function inspectorRows(cell) {
  const k = KINDS[cell.unfoldKind] ?? KINDS.signal
  const rows = [dl('unfold-kind', 'Kind', k.label)]
  for (const [i, s] of (cell.unfoldTiming ?? []).entries()) {
    const what = s.ms != null ? `${fmtMs(s.ms)}${s.start && s.end ? ` · ${s.start.kind} → ${s.end.kind}` : ''}${s.clock ? ` (${s.clock} clock)` : ''}` : `not measured: ${s.why ?? 'unknown'}`
    rows.push(dl(`unfold-t${i}`, s.name[0].toUpperCase() + s.name.slice(1), what))
  }
  if (cell.unfoldKind === 'unfold') {
    rows.push(dl('unfold-play', 'Playback', cell.unfoldPlayback === 'reported' ? 'reported by the host (handed to the device; not proof it was heard)' : 'not reported'))
    rows.push(dl('unfold-status', 'Outcome', cell.unfoldWhy ? `not attributed: ${cell.unfoldWhy}` : cell.unfoldOutcomeWhy ? cell.unfoldOutcomeWhy : cell.unfoldOutcome ?? 'no end observed'))
    if (cell.unfoldCancelledBy?.length) rows.push(dl('unfold-cancel', 'Cancelled', `the host marked this reply cancelled (local cancellation, not a provider terminal): ${cell.unfoldCancelledBy.join(' ')}`))
    if (cell.unfoldStopRequested?.length) rows.push(dl('unfold-stop', 'Stop requested', `a stop was asked or sent (not confirmed; it settles nothing): ${cell.unfoldStopRequested.join(' ')}`))
    if (cell.unfoldPlaybackCleared?.length) rows.push(dl('unfold-cleared', 'Playback cleared', `the host cleared this reply's playback (output cleared; not proof the provider stopped): ${cell.unfoldPlaybackCleared.join(' ')}`))
    rows.push(dl('unfold-tokens', 'Tokens', "not available: the agent's usage report names no reply"))
    rows.push(dl('unfold-cost', 'Cost', 'not available: needs per-reply usage with billable categories'))
  }
  rows.push(dl('unfold-span', 'Overview', cell.unfoldSpan === 'measured' ? `measured on the ${cell.unfoldClock} clock: a bar on that clock's own axis in Duration; a uniform block in Sequence` : cell.unfoldSpan === 'unknown' ? 'start marker with a hatched "?": its length is not measured' : cell.unfoldPlacements?.length ? 'an instant: a tick in Duration' : 'an observation with no producer time: not placed in Duration'))
  if (cell.unfoldKind === 'command') rows.push(dl('unfold-cmd', 'Outcome', cell.unfoldOutcomeWhy ? `not attributed: ${cell.unfoldOutcomeWhy}` : cell.unfoldOutcome ?? 'no finish observed'))
  if (cell.unfoldKind === 'voice' || cell.unfoldKind === 'typed') rows.push(dl('unfold-assoc', 'Reply association', cell.unfoldAssociation ?? 'not available'))
  if (cell.unfoldKind === 'audio') rows.push(dl('unfold-audio', 'Playback', 'not a playback report: audio the agent generated; whether it played is reported by the host separately'))
  const groups = (cell.unfoldPlacements ?? []).map((p) => `${p.clock === 'host' ? 'host' : 'agent'} / ${p.life}`)
  rows.push(dl('unfold-order', 'Position', `Duration: ${groups.length ? `${[...new Set(groups)].join(', ')} (each its own producer axis, relative to its first retained event; groups are not aligned)` : `not placed (${cell.unfoldOffAxis ?? 'no producer time'})`}. Sequence: observation order on the Mind projection`))
  if (cell.unfoldFolded?.length) rows.push(dl('unfold-folded', 'Folded', jsx('span', { children: cell.unfoldFolded.map((f) => `${KINDS[f.kind]?.short ?? f.kind}: ${f.text}`).join('\n') }), { style: { whiteSpace: 'pre-wrap' } }))
  if (cell.unfoldRows?.length) rows.push(dl('unfold-ev', 'Evidence', cell.unfoldRows.join(' '), { style: { fontFamily: 'var(--dsw-font-family-mono, ui-monospace, monospace)', fontSize: 11, userSelect: 'all', wordBreak: 'break-all' } }))
  return rows
}

export const EN_KEYS = {
  ...STATUS_EN,
  'unfold.view': 'Unfold',
  'unfold.tab': 'Trajectory',
  'unfold.tabGuide': 'Every voice, reply, playback, interrupt and signal, on one timeline',
  'unfold.notUnfoldSession': "Unfold's trajectory belongs to the Unfold session. Open the Unfold conversation to see what Unfold heard, said and did.",
}
export const ZH_KEYS = {
  ...STATUS_ZH,
  'unfold.view': 'Unfold',
  'unfold.tab': '轨迹',
  'unfold.tabGuide': '每一段语音、回复、播放、打断和信号，都在同一条时间线上',
  'unfold.notUnfoldSession': 'Unfold 轨迹只属于 Unfold 会话。打开 Unfold 会话即可查看 Unfold 听到、说出和做过的事。',
}

// ---------------------------------------------------------------- Bot header button (placement)
// The Bot shell replaces DSH's session header (and with it the view-ring tabs), so in the Bot
// profile the Unfold trajectory is reached from a button in the Bot's chat header, shown only on the
// Unfold Main Bot (its session is unfold-root). It opens the "Trajectory" tab in the right sidebar
// (chat, composer and voice stay in place); the sidebar's Fullscreen gives it the full width.
export function makeHeaderButton(activate) {
  return function UnfoldTrajectoryHeaderButton({ bot }) {
    const session = bot?.sessionId ?? bot?.id
    if (session !== UNFOLD_SESSION) return null
    return jsxs('button', {
      type: 'button',
      className: 'bt-icon-btn unfold-traj-btn',
      'data-unfold': 'trajectory-toggle',
      'aria-label': 'Open the Unfold trajectory in the right sidebar',
      title: 'Unfold trajectory (what Unfold heard, said and did)',
      onClick: () => activate(),
      style: { display: 'inline-flex', alignItems: 'center', gap: 4, width: 'auto', padding: '0 8px', fontSize: 12 },
      children: [jsx(Icon, { name: 'audio-waveform', size: 14 }), jsx('span', { children: 'Trajectory' })],
    })
  }
}
