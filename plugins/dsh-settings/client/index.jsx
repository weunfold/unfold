// @rth/dsh-settings, browser half: the "RTH settings" panel inside DSH's web client. Same seat
// pattern as @rth/dsh-capabilities: a root-scoped keyed `main` seat under the panel id and a
// `sidebar.panellist` entry with that id. Settings belong to the box, not to a Session.
//
// The same panel is also a right-sidebar tab, "RTH settings", on the dsh-events pattern (the type in
// `sidebarRightTabs`, the body and chip title in the keyed `sidebar.right.pane.tab` and
// `sidebar.right.pane.tab.title` seats). It is needed because the Bot surface hides DSH's left
// panel list (`display: none`), so the left entry is not reachable there. A right-sidebar tab
// opens inside a Session's right sidebar, but its body reads nothing from that Session: it is the
// same box-wide view. Where no Session (so no right sidebar) is open, the tab cannot be opened and
// the left entry is the way in. Where the right-sidebar services are absent, only the right tab
// is skipped (optional inject); the left entry still registers.
//
// The panel claims nothing on its own. Every value arrives with its state, basis, source and
// read time, and src/display.mjs (PanelModel) decides what it may still claim as time passes:
// host values expire at their lease, event-derived values need the stream's confirmation
// (views and heartbeats), a stream error or a failed Refresh stops them at once. One local
// timer redraws at the next expiry; it fetches nothing. Data: one EventSource on
// /api/rth-settings/stream, and a Refresh button that GETs /api/rth-settings (one host read).
import { useCallback, useEffect, useRef, useState } from 'react'
import { BASE_CSS, Dot, Fold, ago, guarded, useNow, useStyle } from '../../rth-ui/ui.jsx'
import { PanelModel } from '../src/display.mjs'

export const PANEL_ID = 'rth.settings'
export const RTH_SETTINGS_TAB_ID = '@rth/dsh-settings'
export const RTH_SETTINGS_TAB_KIND = 'rth.settings'
const ROUTE = 'api/rth-settings'

export const inject = ['slots']

export function apply(ctx) {
  ctx.effect(() => ctx.slots.inject('main', () => ctx.slots.register(
    { name: 'main', key: PANEL_ID },
    guarded('rth-settings', SettingsPanel),
  )), 'rth-settings: main panel')
  ctx.effect(() => ctx.slots.inject('sidebar.panellist', () => ctx.slots.register(
    { name: 'sidebar.panellist', id: PANEL_ID, order: 55, label: () => 'Unfold settings' },
    PanelIcon,
  )), 'rth-settings: sidebar entry')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab', key: RTH_SETTINGS_TAB_ID },
    guarded('rth-settings', SettingsTabBody),
  )), 'rth-settings: right tab body')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
    { name: 'sidebar.right.pane.tab.title', key: RTH_SETTINGS_TAB_ID },
    SettingsTabTitle,
  )), 'rth-settings: right tab title')
  ctx.inject(['sidebarRightTabs', 'sidebarRight'], (scope) => {
    scope.effect(() => scope.sidebarRightTabs.register({
      id: RTH_SETTINGS_TAB_ID,
      kind: RTH_SETTINGS_TAB_KIND,
      priority: 'extension',
      title: () => 'Setup',
      guide: [{
        id: 'rth-settings',
        order: 92,
        title: () => 'Unfold settings',
        description: () => 'How Unfold is set up right now (read-only)',
      }],
    }), 'rth-settings: right tab type')
    // On unload, close our open tabs in every Session, as dsh-events does: DSH's layout would
    // otherwise keep each one's chip with no body behind it.
    scope.effect(() => () => {
      for (const tab of scope.sidebarRight.openTabs.getSnapshot()) {
        if (tab.kind !== RTH_SETTINGS_TAB_KIND) continue
        try {
          scope.sidebarRight.closeIn(tab.sessionId, tab.tabId)
        } catch {} // the Session or tab already went away
      }
    }, 'rth-settings: close open tabs on unload')
  })
}

function SettingsTabTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-rth="settings-title">{tab.title}</span>
}

function SettingsTabBody() {
  return <SettingsPanel compact />
}

function PanelIcon({ size = 16 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
      <path d="M2 4h7M12 4h2M2 8h2M7 8h7M2 12h9M14 12h0" />
      <circle cx="10.5" cy="4" r="1.5" />
      <circle cx="5.5" cy="8" r="1.5" />
      <circle cx="12.5" cy="12" r="1.5" />
    </svg>
  )
}

function useSettings() {
  const model = useRef(null)
  model.current ??= new PanelModel()
  const [, redraw] = useState(0)
  const bump = useCallback(() => redraw((n) => n + 1), [])
  const fetching = useRef(false)
  // One GET at a time; its answer goes through the same ordering as stream views, so a late
  // answer never replaces a newer view.
  const refresh = useCallback(async () => {
    const m = model.current
    if (fetching.current) return
    fetching.current = true
    try {
      const res = await fetch(new URL(ROUTE, document.baseURI), { cache: 'no-store' })
      if (!res.ok) throw new Error(`status ${res.status}`)
      m.setView(await res.json(), Date.now())
    } catch (e) {
      m.refreshFailed(e?.message ?? e)
    } finally {
      fetching.current = false
    }
    bump()
  }, [bump])
  useEffect(() => {
    const m = model.current
    const source = new EventSource(new URL(`${ROUTE}/stream`, document.baseURI))
    source.addEventListener('settings', (e) => {
      m.setView(JSON.parse(e.data), Date.now())
      bump()
    })
    source.addEventListener('heartbeat', (e) => {
      m.heartbeat(JSON.parse(e.data), Date.now())
      // A beat naming a newer revision: this view is outdated; ask for the current one.
      if (m.needsSnapshot) void refresh()
      bump()
    })
    source.addEventListener('resnapshot', () => {
      m.streamLost(Date.now())
      bump()
    })
    source.onopen = () => {
      m.streamOpen()
      bump()
    }
    source.onerror = () => {
      m.streamLost(Date.now())
      bump()
    }
    return () => source.close()
  }, [bump, refresh])
  const now = Date.now()
  const drawn = model.current.render(now)
  const next = model.current.nextChange(now)
  // Time advances the evidence: draw again when the next value expires.
  useEffect(() => {
    if (next === null) return
    const t = setTimeout(bump, Math.max(0, next - Date.now()))
    return () => clearTimeout(t)
  })
  return { drawn, refresh }
}

const SETTINGS_CSS = `
.rth-set{padding:16px 16px 24px;display:flex;flex-direction:column;gap:14px}
.rth-set[data-placement=main]{padding:28px 32px;max-width:760px}
.rth-set-head{display:flex;align-items:baseline;gap:10px}
.rth-set-head h1{margin:0;font-size:15px;font-weight:620;letter-spacing:-.005em}
.rth-set[data-placement=main] .rth-set-head h1{font-size:20px}
.rth-set-fresh{display:flex;align-items:center;gap:7px;font-size:12px;color:var(--rth-muted)}
.rth-set-fresh button{all:unset;cursor:pointer;color:var(--rth-accent);font-size:12px;margin-left:4px}
.rth-set-fresh button:focus-visible{outline:2px solid var(--rth-accent)}
.rth-sum{display:flex;flex-direction:column;border:1px solid var(--rth-line);border-radius:12px;overflow:hidden}
.rth-sum-row{display:grid;grid-template-columns:minmax(0,9fr) minmax(0,11fr);gap:12px;padding:9px 12px;align-items:baseline}
.rth-sum-row+.rth-sum-row{border-top:1px solid var(--rth-line)}
.rth-sum-k{color:var(--rth-muted)}
.rth-sum-v{overflow-wrap:anywhere}
.rth-sum-v[data-state=stale],.rth-sum-v[data-state=unknown]{color:var(--rth-faint);font-style:italic}
.rth-sec{margin:4px 0 10px}
.rth-sec h3{margin:10px 0 4px;font-size:12px;font-weight:600;color:var(--rth-muted);letter-spacing:.02em}
.rth-leaf{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1.2fr);gap:10px;padding:4px 0;font-size:12px;border-top:1px solid var(--rth-line)}
.rth-leaf-v{overflow-wrap:anywhere;font-family:var(--rth-mono);font-size:11px}
.rth-leaf-v[data-state=stale],.rth-leaf-v[data-state=unknown]{color:var(--rth-faint)}
.rth-leaf-m{display:block;font-family:inherit;font-size:10.5px;color:var(--rth-faint)}
.rth-leaf pre{white-space:pre-wrap;margin:0;font-size:11px}
`

const STATE_TEXT = { live: 'current', source: 'from the source code', stale: 'out of date', unknown: 'not reported' }

/** The few things a person would ask about, in plain words, each from one leaf (or unsure). */
const SUMMARY = [
  ['Voice model', 'model', (v) => v],
  ['Instructions', 'policy', (v) => v],
  ['Who speaks when', 'turn_taking', (v) => (/^provider VAD/.test(v) ? 'The voice provider decides' : v)],
  ['Echo control', 'echo_cancellation', (v) => (/^apple_voice_processing/.test(v) ? "Apple's voice processing" : v)],
  ['Microphone mode', 'mic_mode', (v) => ({ voice_isolation: 'Voice isolation', standard: 'Standard', wide_spectrum: 'Wide spectrum' }[v?.active] ?? v?.active ?? null)],
  ['Memory when reconnecting', 'context_link', (v) => ({ restored: 'Picks up where it left off', partial: 'Remembers part of it', not_restored: 'Starts fresh' }[v] ?? v)],
  ['Running build', 'deployment_agree', (v) => (v === 'yes' ? 'The activated build' : v === 'no' ? "Not the activated build" : v)],
  ['Changes from here', null, () => 'Read-only for now'],
]

function plain(d, map) {
  if (!d) return { text: 'Not reported', state: 'unknown' }
  if (d.state === 'stale') return { text: 'Not sure right now', state: 'stale' }
  if (d.state === 'unknown' || d.leaf.value == null) return { text: 'Not reported', state: 'unknown' }
  const v = map(d.leaf.value)
  return { text: v == null ? 'Not reported' : String(v), state: d.state }
}

function SettingsPanel({ compact = false }) {
  useStyle('rth-ui-base', BASE_CSS)
  useStyle('rth-ui-settings', SETTINGS_CSS)
  const { drawn, refresh } = useSettings()
  const now = useNow(5000)
  const leaves = new Map((drawn?.sections ?? []).flatMap((s) => s.leaves.map((d) => [d.leaf.id, d])))
  const host = drawn?.sources?.host
  const live = drawn?.stream === 'open' && host?.ok
  return (
    <div className="rth-ui rth-set" data-rth="settings" data-placement={compact ? 'right-tab' : 'main'}>
      <div className="rth-set-head">
        <h1>How Unfold is set up</h1>
      </div>
      <div className="rth-set-fresh" data-rth="settings-stream" data-state={drawn?.stream ?? 'opening'}>
        <Dot tone={live ? 'ok' : 'neutral'} />
        <span>{!drawn ? 'Reading…' : live ? `Up to date · checked ${ago(host.read_at, now)}` : drawn.stream !== 'open' ? 'Out of touch, reconnecting' : `Not sure right now · last checked ${host?.read_at ? ago(host.read_at, now) : 'never'}`}</span>
        <button type="button" data-rth="settings-refresh" onClick={refresh}>Check now</button>
        {drawn?.refreshError ? <span data-rth="settings-error" className="rth-faint">(couldn't check: {drawn.refreshError})</span> : null}
      </div>
      {drawn ? (
        <>
          <div className="rth-sum" data-rth="settings-summary">
            {SUMMARY.map(([k, id, map]) => {
              const p = id ? plain(leaves.get(id), map) : { text: map(), state: 'live' }
              return <div className="rth-sum-row" key={k}><span className="rth-sum-k">{k}</span><span className="rth-sum-v" data-state={p.state} data-leaf={id ?? ''}>{p.text}</span></div>
            })}
          </div>
          <div>
            <Fold id="settings:all" summary="All settings">
              {drawn.sections.map((s) => (
                <section key={s.id} className="rth-sec" data-rth="settings-section" data-section={s.id}>
                  <h3>{s.title}</h3>
                  {s.leaves.map((d) => <Leaf key={d.leaf.id} d={d} now={now} />)}
                </section>
              ))}
            </Fold>
            <Fold id="settings:technical" summary="Technical details">
              <Technical drawn={drawn} now={now} />
            </Fold>
          </div>
        </>
      ) : null}
    </div>
  )
}

function Technical({ drawn, now }) {
  const h = drawn.sources.host
  const a = drawn.sources.agentd
  return (
    <div className="rth-sec" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div data-rth="settings-revisions" style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        {drawn.revisions.map((r) => (
          <div key={r.key} data-rev={r.key} data-state={r.state} className="rth-leaf" style={{ borderTop: 0 }}>
            <span className="rth-muted">{r.leaf.label}</span>
            <span className="rth-leaf-v" data-state={r.state}>{r.text}<span className="rth-leaf-m">{STATE_TEXT[r.state] ?? r.state}</span></span>
          </div>
        ))}
      </div>
      <div data-rth="settings-sources" className="rth-mono rth-faint">
        host status: {h.ok ? `read ${ago(h.read_at, now)}` : `not current (${h.error})`} · agent events: {a.error ?? `observer ${a.link}, boot ${a.boot ?? 'not positively known'}`} · stream: {drawn.stream}
      </div>
      <div data-rth="settings-edits" className="rth-faint">
        Edits: {drawn.edits.pending.length} pending, {drawn.edits.applied.length} applied, {drawn.edits.rejected.length} rejected. {drawn.edits.note}
      </div>
      <div data-rth="settings-gaps">
        <div className="rth-muted" style={{ marginBottom: 2 }}>Not reported anywhere yet</div>
        <ul style={{ margin: 0, paddingLeft: 16, fontSize: 11.5 }} className="rth-faint">
          {drawn.gaps.map((g) => <li key={g.id}><b>{g.what}</b>: {g.detail}</li>)}
        </ul>
      </div>
    </div>
  )
}

function Leaf({ d, now }) {
  const { leaf, state, text, note } = d
  const long = typeof leaf.value === 'string' && leaf.value.length > 80 && state === 'live'
  return (
    <div className="rth-leaf">
      <span data-rth="settings-label" className="rth-muted">{leaf.label}</span>
      <span className="rth-leaf-v" data-rth="settings-leaf" data-id={leaf.id} data-state={state}>
        {long ? <pre>{text}</pre> : text}
        <span className="rth-leaf-m">{STATE_TEXT[state] ?? state} · {leaf.source}{leaf.observed_at ? ` · ${ago(leaf.observed_at, now)}` : ''}{note ? ` · ${note}` : ''}</span>
      </span>
    </div>
  )
}
