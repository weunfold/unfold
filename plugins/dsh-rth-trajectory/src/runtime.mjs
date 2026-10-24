// Browser runtime for the RTH trajectory fork: the one shared Mind subscription, the taxonomy badges
// and icons, and the RTH style sheet. Bundled by build.mjs into the forked client (region
// "rth/runtime") with React and DSH primitives taken from the module loader.
import { jsx, jsxs } from 'react/jsx-runtime'
import { DEFAULT_GROUPS, buildRthTrajectory, filterTurns, newSince } from './adapter.mjs'
import { STATUS_EN, STATUS_ZH, stateOf, timelineDetail } from './status.mjs'
import { GROUPS, KINDS, LANES } from './taxonomy.mjs'
import { ICONS } from './icons.generated.mjs'
import { layoutClockOverview } from './clock-layout.mjs'

export { DEFAULT_GROUPS, GROUPS, KINDS, LANES, buildRthTrajectory, filterTurns, layoutClockOverview, newSince, stateOf, timelineDetail }
export const RTH_SESSION = 'rth-root'

// ---------------------------------------------------------------- the shared subscription
// One EventSource on /api/rth-mind/stream for every RTH trajectory view in this page (ref-counted),
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

// Freshness lease, Mind's rule (dsh-mind/src/display.mjs freshness, STREAM_BEATS
// = 3; index.mjs: "readers treat 3 missed beats as a lost stream"): 'live' only while the last
// frame is within three heartbeats by the browser clock; 'stale' past it with no new input needed;
// 'lost' on a transport error; 'opening' until the first snapshot. Derived at read time; a timer
// only wakes subscribers at the boundary.
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
    source = new EventSource(new URL('api/rth-mind/stream', document.baseURI))
    for (const name of EVENTS) {
      source.addEventListener(name, (e) => {
        state = reduce(state, name, JSON.parse(e.data), Date.now())
        if (name === 'snapshot' || name === 'row' || name === 'row-updated' || name === 'evicted') track()
        arm()
        emit()
      })
    }
    source.addEventListener('resnapshot', () => { source.close(); state = { ...state, stream: 'opening' }; arm(); emit(); retry = setTimeout(open, 500) })
    source.onerror = () => { state = { ...state, stream: 'lost' }; arm(); emit() }
  }
  shared = {
    getSnapshot() {
      const fresh = streamFreshness(state, Date.now()).state
      if (derivedFrom !== state || derivedFresh !== fresh) {
        if (derivedFrom !== state) derived = { ...buildRthTrajectory({ ...state, arrivals }), arrivals, latestArrival, link: state.link, sources: state.sources, rowCount: state.rows.length }
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

const NOT_RTH = Object.freeze({ notRth: true, turns: [], cards: new Map(), lastIndex: 0, counts: {}, strip: null, stream: 'none', arrivals: new Map(), latestArrival: 0 })
const notRthSource = { getSnapshot: () => NOT_RTH, subscribe: () => () => {} }

/** The data source for one conversation session: RTH's feed only in the RTH session. */
export function sourceFor(sessionId) {
  return sessionId === RTH_SESSION ? sharedStream() : notRthSource
}

// ---------------------------------------------------------------- icons (pinned lucide-static release)
// The element lists come from lucide-static 0.460.0 (src/icons.generated.mjs, made by
// tools/pin-icons.mjs; ISC, portions Feather MIT: notices in third_party/ and the bundle header).
export function Icon({ name, size = 13 }) {
  const els = ICONS[name] ?? ICONS.radio
  return jsx('svg', { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round', 'aria-hidden': 'true', children: els.map(([tag, attrs], i) => jsx(tag, { ...attrs }, i)) })
}

/** The coloured kind badge, in upstream's kindTag slot (icon + short label; tooltip = full label). */
export function kindTag(rthKind, css, primitives) {
  const k = KINDS[rthKind] ?? KINDS.signal
  return jsxs('span', {
    className: `${css.kindTag} rth-kind`,
    'data-rth-kind': rthKind,
    'data-role-kind': k.upstream,
    children: [
      jsx(primitives.Tooltip, { label: k.label, side: 'right', children: jsx('span', { className: css.kindTagIcon, 'aria-hidden': 'true', children: jsx(Icon, { name: k.icon }) }) }),
      jsx('span', { className: css.kindTagLabel, children: k.short }),
    ],
  })
}

export function kindLabel(rthKind) {
  return KINDS[rthKind]?.label ?? null
}

// ---------------------------------------------------------------- styles (scoped under .rth-trajectory)
const vars = (i) => Object.entries(KINDS).map(([k, v]) => `--rth-k-${k}:${v.color[i]};`).join('')
export const CSS = `
.rth-trajectory{${vars(0)}}
@media (prefers-color-scheme:dark){.rth-trajectory{${vars(1)}}}
[data-theme=dark] .rth-trajectory,.dark .rth-trajectory{${vars(1)}}
.rth-trajectory .rth-kind{--k:var(--rth-k-signal);color:var(--k);background:color-mix(in srgb,var(--k) 14%,transparent);border:1px solid color-mix(in srgb,var(--k) 38%,transparent)}
${Object.keys(KINDS).map((k) => `.rth-trajectory .rth-kind[data-rth-kind=${k}]{--k:var(--rth-k-${k})}`).join('\n')}
.rth-trajectory .rth-kind[data-rth-kind=typed]{background:transparent}
.rth-trajectory .rth-kind[data-rth-kind=gap]{border-style:dashed}
.rth-trajectory .rth-kind[data-rth-kind=signal]{opacity:.75}
${Object.keys(KINDS).map((k) => `.rth-trajectory [data-timeline-span=${k}]{--rth-u:var(--rth-k-${k});background:var(--rth-k-${k})!important;opacity:.92}`).join('\n')}
.rth-trajectory [data-rth-span=instant]{width:3px!important;min-width:3px!important;border-radius:2px}
.rth-trajectory [data-rth-span^=unknown]{background:linear-gradient(var(--rth-u,currentColor) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--rth-u,currentColor) 75%,transparent) 0 2px,transparent 2px 4px)!important;opacity:.9;overflow:visible!important}
.rth-trajectory [data-rth-span=unknown]{width:14px!important;min-width:14px!important}
.rth-trajectory [data-rth-span^=unknown]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory [data-rth-span=unknown-seq]::after{left:6px}
.rth-trajectory .rth-strip{display:flex;flex-wrap:wrap;gap:6px;align-items:center;padding:4px 10px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-chip{display:inline-flex;align-items:center;gap:4px;padding:1px 8px;border-radius:999px;border:1px solid color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 40%,transparent);background:color-mix(in srgb,var(--k,var(--dsw-alias-label-secondary)) 10%,transparent);color:var(--k,var(--dsw-alias-label-secondary));cursor:pointer;user-select:none}
.rth-trajectory .rth-chip[aria-pressed=false]{opacity:.45;background:transparent}
.rth-trajectory .rth-chip b{font-weight:600}
.rth-trajectory .rth-stat{color:var(--dsw-alias-label-caption)}
.rth-trajectory:focus{outline:none}
.rth-trajectory .rth-clock{color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-live{display:inline-flex;align-items:center;gap:5px;height:18px;padding:0 8px;border-radius:999px;border:1px solid var(--dsw-alias-border-l2);background:transparent;color:var(--dsw-alias-label-secondary);font:inherit;cursor:pointer}
.rth-trajectory .rth-live-dot{width:6px;height:6px;border-radius:50%;background:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-live[aria-pressed=true]{color:var(--rth-k-rth);border-color:currentColor}
.rth-trajectory .rth-live[aria-pressed=true] .rth-live-dot{background:currentColor}
.rth-trajectory:not([data-hidden]) .rth-live[aria-pressed=true][data-stream=live] .rth-live-dot{animation:rth-pulse 1.6s ease-in-out infinite}
.rth-trajectory .rth-live[aria-pressed=true]:not([data-stream=live]){color:var(--dsw-alias-label-secondary);border-color:var(--dsw-alias-border-l2)}
.rth-trajectory .rth-conn{display:inline-flex;align-items:center;gap:4px;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-conn[data-stream=stale],.rth-trajectory .rth-conn[data-stream=lost]{color:var(--rth-k-fault)}
.rth-trajectory .rth-live[data-new=yes]{color:var(--rth-k-rth);border-color:currentColor;font-weight:600}
.rth-trajectory .rth-live:focus-visible,.rth-trajectory .rth-chip:focus-visible{outline:1px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
@keyframes rth-pulse{50%{opacity:.35}}
@media (prefers-reduced-motion:reduce){.rth-trajectory .rth-live-dot{animation:none!important}}
.rth-trajectory .rth-stat strong{color:var(--dsw-alias-label-primary);font-weight:600}
.rth-trajectory .rth-sep{flex:1}
.rth-trajectory .rth-clock-ov{padding:4px 8px 6px;border-bottom:.5px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-layer-1);font:var(--dsw-font-xxxs-11);color:var(--dsw-alias-label-caption);max-height:40vh;overflow:auto}
.rth-trajectory .rth-cg{padding:3px 0 2px}
.rth-trajectory .rth-cg+.rth-cg{margin-top:6px;border-top:1px dashed var(--dsw-alias-border-l4);padding-top:5px}
.rth-trajectory .rth-cg-head{font-size:10px;line-height:14px;color:var(--dsw-alias-label-caption);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.rth-trajectory .rth-cg-head strong{font-weight:600;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-cg-body{display:flex;position:relative}
.rth-trajectory .rth-cg-labels{flex:none;width:38px;display:flex;flex-direction:column;padding-top:2px;font-size:9px;line-height:13px;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-cg-track{position:relative;flex:1;min-width:0;margin-right:22px;border-left:.5px solid var(--dsw-alias-border-l2);border-bottom:.5px solid var(--dsw-alias-border-l2)}
.rth-trajectory .rth-cg-ticks{display:flex;height:11px}
.rth-trajectory .rth-cg-tick-pad{flex:none;width:38px}
.rth-trajectory .rth-cg-tickrow{position:relative;flex:1;margin-right:22px}
.rth-trajectory .rth-cg-tickrow span{position:absolute;top:0;transform:translateX(-50%);font-size:9px;line-height:11px;white-space:nowrap;color:var(--dsw-alias-label-caption)}
.rth-trajectory .rth-cg-tickrow span:first-child{transform:none}
.rth-trajectory .rth-ci{position:absolute;height:9px;min-width:3px;padding:0;border:0;border-radius:2px;background:var(--k);opacity:.92;cursor:pointer}
.rth-trajectory .rth-ci[data-shape=tick]{width:3px}
.rth-trajectory .rth-ci[data-shape=zero]{width:3px;outline:1px solid var(--k);outline-offset:1px}
.rth-trajectory .rth-ci[data-shape=start]{width:14px;background:linear-gradient(var(--k) 0 0) left/3px 100% no-repeat,repeating-linear-gradient(135deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.rth-trajectory .rth-ci[data-shape=start]::after{content:"?";position:absolute;left:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-ci[data-shape=end]{width:14px;margin-left:-14px;background:linear-gradient(var(--k) 0 0) right/3px 100% no-repeat,repeating-linear-gradient(45deg,color-mix(in srgb,var(--k) 75%,transparent) 0 2px,transparent 2px 4px)}
.rth-trajectory .rth-ci[data-shape=end]::before{content:"?";position:absolute;right:calc(100% + 2px);top:50%;transform:translateY(-50%);font:600 9px/1 system-ui;color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-ci[data-selected]{outline:2px solid var(--dsw-alias-label-primary);outline-offset:1px;z-index:2}
.rth-trajectory .rth-ci[data-match]{box-shadow:0 0 0 2px color-mix(in srgb,var(--dsw-alias-state-business-primary) 70%,transparent)}
.rth-trajectory .rth-ci:focus-visible{outline:2px solid var(--dsw-alias-state-business-primary);outline-offset:1px}
.rth-trajectory .rth-cg-note{margin-top:4px;font-size:10px;line-height:13px;color:var(--dsw-alias-label-caption)}
.rth-trajectory button[hidden]{display:none!important}
.rth-trajectory .rth-kind{gap:3px}
.rth-trajectory .rth-kind>[class*=kindTagIcon]{opacity:1;width:13px;transform:none}
.rth-trajectory [class*="rtjY0dWHa_turnLabel"]{font:600 9px/12px var(--ds-font-family-code,ui-monospace,monospace)}
.rth-trajectory .rth-card{display:inline-flex;gap:4px;align-items:center;margin-left:10px;vertical-align:middle;color:var(--rth-k-rth)}
.rth-trajectory .rth-card-chip{font:var(--dsw-font-xxxs-11);padding:0 6px;border-radius:999px;background:color-mix(in srgb,var(--rth-k-rth) 12%,transparent);color:var(--dsw-alias-label-secondary)}
.rth-trajectory .rth-card-chip[data-chip=interrupt]{color:var(--rth-k-interrupt);background:color-mix(in srgb,var(--rth-k-interrupt) 14%,transparent)}
.rth-trajectory .rth-card-chip[data-chip=unknown]{color:var(--rth-k-gap);background:transparent;border:1px dashed color-mix(in srgb,var(--rth-k-gap) 50%,transparent)}
.rth-trajectory .rth-notrth{margin:auto;max-width:420px;text-align:center;color:var(--dsw-alias-label-secondary);font:var(--dsw-font-xs-13);padding:24px}
`
export const GROUP_COLOR = { you: 'voice', rth: 'rth', voice: 'speaking', control: 'command', signals: 'signal', faults: 'fault' }

let styled = false
export function ensureStyle() {
  if (styled || typeof document === 'undefined') return
  styled = true
  const tag = document.createElement('style')
  tag.dataset.plugin = '@rth/dsh-rth-trajectory'
  tag.dataset.pluginCss = '@rth/dsh-rth-trajectory/rth.css'
  tag.textContent = CSS
  document.head.appendChild(tag)
}

const CONNECTION = { live: 'connected', stale: 'stream stale (no heartbeat)', lost: 'stream lost', opening: 'connecting', none: '' }

/** The session strip + kind filter chips row. */
export function StripRow({ data, enabled, onToggle, live = false, newSincePause = 0, onLive }) {
  const s = data.strip
  const fmt = (ms) => (ms == null ? null : ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`)
  return jsxs('div', {
    className: 'rth-strip',
    'data-rth': 'trajectory-strip',
    children: [
      ...GROUPS.map((g) => jsxs('span', {
        role: 'button',
        tabIndex: 0,
        className: 'rth-chip',
        'aria-pressed': enabled.has(g.id),
        style: { '--k': `var(--rth-k-${GROUP_COLOR[g.id]})` },
        onClick: () => onToggle(g.id),
        onKeyDown: (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle(g.id) } },
        children: [g.label, ' ', jsx('b', { children: data.counts[g.id] ?? 0 })],
      }, g.id)),
      jsx('span', { className: 'rth-sep' }),
      jsx('span', {
        className: 'rth-conn',
        'data-rth': 'trajectory-connection',
        'data-stream': data.stream,
        title: 'The Mind stream this view reads: live only while frames arrive within three heartbeats',
        children: CONNECTION[data.stream] ?? data.stream,
      }),
      jsxs('button', {
        type: 'button',
        className: 'rth-live',
        'data-rth': 'trajectory-live',
        'aria-pressed': live,
        'data-stream': data.stream,
        'data-new': !live && newSincePause > 0 ? 'yes' : undefined,
        title: 'Follow the newest record (l). Scrolling away pauses it. Keys: j/k move, Enter inspect, / search, f filters, l live',
        onClick: onLive,
        children: [jsx('span', { className: 'rth-live-dot', 'aria-hidden': true }), live ? (data.stream === 'live' ? 'Live' : 'Following') : newSincePause > 0 ? `Paused · ${newSincePause} new` : 'Paused'],
      }),
      s ? jsxs('span', { className: 'rth-stat', children: [jsx('strong', { children: s.turns }), ' turns'] }) : null,
      s ? jsxs('span', { className: 'rth-stat', title: `first text received (agent clock: command admitted -> first text); not provider TTFT. Coverage ${s.firstTextCoverage}.`, children: ['first text ', jsx('strong', { children: fmt(s.avgFirstTextMs) ?? 'not measured' }), s.avgFirstTextMs == null ? null : jsx('span', { className: 'rth-clock', children: ' (agent clock)' })] }) : null,
      s ? jsxs('span', { className: 'rth-stat', children: [jsx('strong', { children: s.interrupts }), s.interrupts === 1 ? ' interrupt' : ' interrupts'] }) : null,
      s ? jsxs('span', { className: 'rth-stat', children: [jsx('strong', { children: s.faults }), s.faults === 1 ? ' fault' : ' faults'] }) : null,
      s ? jsx('span', { className: 'rth-stat', title: `${s.usageReports} provider usage report(s) seen; they name a connection, not a reply`, children: 'tokens: not linked yet' }) : null,
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
    className: 'rth-clock-ov',
    'data-rth': 'clock-overview',
    'aria-label': 'Duration overview: one group per producer clock and lifetime, each on its own axis; groups are not aligned with each other',
    children: [
      ...layout.groups.map((g) => jsxs('div', {
        className: 'rth-cg',
        'data-clock': g.clock,
        'data-life': g.life,
        children: [
          jsxs('div', { className: 'rth-cg-head', children: [jsx('strong', { children: g.title }), jsx('span', { children: ` · ${g.axisLabel} · spans ${fmtDur(g.extentMs)}` })] }),
          jsxs('div', {
            className: 'rth-cg-body',
            style: { height: g.lanes.length * LANE_H + 4 },
            children: [
              jsx('div', { className: 'rth-cg-labels', children: g.lanes.map((l) => jsx('span', { style: { height: LANE_H }, children: l.name }, l.lane)) }),
              jsx('div', {
                className: 'rth-cg-track',
                children: g.items.map((it, n) => {
                  const row = g.lanes.findIndex((l) => l.lane === it.lane)
                  const k = KINDS[it.kind]
                  const what = SHAPE_TEXT[it.shape](it)
                  return jsx('button', {
                    type: 'button',
                    className: 'rth-ci',
                    'data-kind': it.kind,
                    'data-shape': it.shape,
                    'data-selected': selectedIndex === it.index || undefined,
                    'data-match': searchMatchIndexes?.has(it.index) || undefined,
                    style: { left: `${it.x0 * 100}%`, width: it.shape === 'span' ? `max(2px, ${(it.x1 - it.x0) * 100}%)` : undefined, top: row * LANE_H + 2, '--k': `var(--rth-k-${it.kind})` },
                    title: `${it.name} · ${what} · ${g.title}`,
                    'aria-label': `${k?.label ?? it.kind}: ${it.name}, ${what}`,
                    onClick: () => onSelect?.(it.index),
                  }, `${it.recordId}-${n}`)
                }),
              }),
            ],
          }),
          jsxs('div', { className: 'rth-cg-ticks', children: [jsx('span', { className: 'rth-cg-tick-pad' }), jsx('div', { className: 'rth-cg-tickrow', children: g.ticks.map((t) => jsx('span', { style: { left: `${t.x * 100}%` }, children: t.label }, t.ms)) })] }),
        ],
      }, `${g.clock}|${g.life}`)),
      layout.groups.length > 1 ? jsx('div', { className: 'rth-cg-note', children: 'Each group has its own clock, origin and scale. Positions in different groups are not comparable in time.' }) : null,
      offTotal > 0 ? jsx('div', { className: 'rth-cg-note', 'data-rth': 'clock-unplaced', title: off.map(([why, n]) => `${n}: ${why}`).join('\n'), children: `Not placed on a producer clock: ${off.map(([why, n]) => `${n} (${why})`).join('; ')}` }) : null,
      layout.groups.length === 0 ? jsx('div', { className: 'rth-cg-note', children: 'Nothing with a valid producer time yet.' }) : null,
    ],
  })
}

// ---------------------------------------------------------------- turn cards, inspector, locale
/** The short label of a between-turns section (short enough for the turn-label column). */
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
  return jsxs('span', { className: 'rth-card', 'data-rth': 'turn-card', children: [jsx(Icon, { name: c.icon, size: 11 }), ...c.chips.map((chip, i) => jsx('span', { className: 'rth-card-chip', 'data-chip': /interrupt/.test(chip) ? 'interrupt' : /not attributed/.test(chip) ? 'unknown' : 'm', children: chip }, i))] })
}

const fmtMs = (ms) => (ms == null ? null : ms < 1000 ? `${ms.toFixed(ms < 10 ? 2 : 0)} ms` : `${(ms / 1000).toFixed(2)} s`)
const dl = (key, dt, dd, extra = {}) => jsxs('div', { 'data-rth-detail': key, children: [jsx('dt', { children: dt }), jsx('dd', { ...extra, children: dd })] }, key)

/** Inspector Summary rows for an RTH record: named timing, clock, evidence ids. */
export function inspectorRows(cell) {
  const k = KINDS[cell.rthKind] ?? KINDS.signal
  const rows = [dl('rth-kind', 'Kind', k.label)]
  for (const [i, s] of (cell.rthTiming ?? []).entries()) {
    const what = s.ms != null ? `${fmtMs(s.ms)}${s.start && s.end ? ` · ${s.start.kind} → ${s.end.kind}` : ''}${s.clock ? ` (${s.clock === 'host' ? 'rth-host' : s.clock} clock)` : ''}` : `not measured: ${s.why ?? 'unknown'}`
    rows.push(dl(`rth-t${i}`, s.name[0].toUpperCase() + s.name.slice(1), what))
  }
  if (cell.rthKind === 'rth') {
    rows.push(dl('rth-play', 'Playback', cell.rthPlayback === 'reported' ? 'reported by rth-host (handed to the device; not proof it was heard)' : 'not reported'))
    rows.push(dl('rth-status', 'Outcome', cell.rthWhy ? `not attributed: ${cell.rthWhy}` : cell.rthOutcomeWhy ? cell.rthOutcomeWhy : cell.rthOutcome ?? 'no end observed'))
    if (cell.rthCancelledBy?.length) rows.push(dl('rth-cancel', 'Cancelled', `the host marked this reply cancelled (local cancellation, not a provider terminal): ${cell.rthCancelledBy.join(' ')}`))
    if (cell.rthStopRequested?.length) rows.push(dl('rth-stop', 'Stop requested', `a stop was asked or sent (not confirmed; it settles nothing): ${cell.rthStopRequested.join(' ')}`))
    if (cell.rthPlaybackCleared?.length) rows.push(dl('rth-cleared', 'Playback cleared', `the host cleared this reply's playback (output cleared; not proof the provider stopped): ${cell.rthPlaybackCleared.join(' ')}`))
    rows.push(dl('rth-tokens', 'Tokens', "not available: the agent's usage report names no reply"))
    rows.push(dl('rth-cost', 'Cost', 'not available: needs per-reply usage with billable categories'))
  }
  rows.push(dl('rth-span', 'Overview', cell.rthSpan === 'measured' ? `measured on the ${cell.rthClock} clock: a bar on that clock's own axis in Duration; a uniform block in Sequence` : cell.rthSpan === 'unknown' ? 'start marker with a hatched "?": its length is not measured' : cell.rthPlacements?.length ? 'an instant: a tick in Duration' : 'an observation with no producer time: not placed in Duration'))
  if (cell.rthKind === 'command') rows.push(dl('rth-cmd', 'Outcome', cell.rthOutcomeWhy ? `not attributed: ${cell.rthOutcomeWhy}` : cell.rthOutcome ?? 'no finish observed'))
  if (cell.rthKind === 'voice' || cell.rthKind === 'typed') rows.push(dl('rth-assoc', 'Reply association', cell.rthAssociation ?? 'not available'))
  if (cell.rthKind === 'audio') rows.push(dl('rth-audio', 'Playback', 'not a playback report: audio the agent generated; whether it played is reported by rth-host separately'))
  const groups = (cell.rthPlacements ?? []).map((p) => `${p.clock === 'host' ? 'rth-host' : 'agent'} / ${p.life}`)
  rows.push(dl('rth-order', 'Position', `Duration: ${groups.length ? `${[...new Set(groups)].join(', ')} (each its own producer axis, relative to its first retained event; groups are not aligned)` : `not placed (${cell.rthOffAxis ?? 'no producer time'})`}. Sequence: observation order on the Mind projection`))
  if (cell.rthFolded?.length) rows.push(dl('rth-folded', 'Folded', jsx('span', { children: cell.rthFolded.map((f) => `${KINDS[f.kind]?.short ?? f.kind}: ${f.text}`).join('\n') }), { style: { whiteSpace: 'pre-wrap' } }))
  if (cell.rthRows?.length) rows.push(dl('rth-ev', 'Evidence', cell.rthRows.join(' '), { style: { fontFamily: 'var(--dsw-font-family-mono, ui-monospace, monospace)', fontSize: 11, userSelect: 'all', wordBreak: 'break-all' } }))
  return rows
}

export const EN_KEYS = {
  ...STATUS_EN,
  'rth.view': 'Unfold',
  'rth.tab': 'Trajectory',
  'rth.tabGuide': 'Every voice, reply, playback, interrupt and signal, on one timeline',
  'rth.notRthSession': "Unfold's trajectory belongs to the Unfold session. Open the Unfold conversation to see what Unfold heard, said and did.",
}
export const ZH_KEYS = {
  ...STATUS_ZH,
  'rth.view': 'Unfold',
  'rth.tab': '轨迹',
  'rth.tabGuide': '每一段语音、回复、播放、打断和信号，都在同一条时间线上',
  'rth.notRthSession': 'Unfold 轨迹只属于 Unfold 会话。打开 Unfold 会话即可查看 Unfold 听到、说出和做过的事。',
}

// ---------------------------------------------------------------- Bot header button (placement)
// The Bot shell replaces DSH's session header (and with it the view-ring tabs), so in the Bot
// profile the RTH trajectory is reached from a button in the Bot's chat header, shown only on the
// RTH Main Bot (its session is rth-root). It opens the "Trajectory" tab in the right sidebar
// (chat, composer and voice stay in place); the sidebar's Fullscreen gives it the full width.
export function makeHeaderButton(activate) {
  return function RthTrajectoryHeaderButton({ bot }) {
    const session = bot?.sessionId ?? bot?.id
    if (session !== RTH_SESSION) return null
    return jsxs('button', {
      type: 'button',
      className: 'bt-icon-btn rth-traj-btn',
      'data-rth': 'trajectory-toggle',
      'aria-label': 'Open the Unfold trajectory in the right sidebar',
      title: 'Unfold trajectory (what Unfold heard, said and did)',
      onClick: () => activate(),
      style: { display: 'inline-flex', alignItems: 'center', gap: 4, width: 'auto', padding: '0 8px', fontSize: 12 },
      children: [jsx(Icon, { name: 'audio-waveform', size: 14 }), jsx('span', { children: 'Trajectory' })],
    })
  }
}
