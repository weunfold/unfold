// @rth/dsh-capabilities, browser half: the "What I can do" main panel inside DSH's web client.
// The pattern from upstream ui-plugin-manager: a root-scoped keyed `main` seat under the panel
// id, and a `sidebar.panellist` entry with the same id that selects it. The panel is global (it
// belongs to the profile, not to a Session).
//
// Data arrives by push only: one EventSource on /api/rth-capabilities/stream while the panel is
// mounted. A finished bench run (or anyone) POSTs /api/rth-capabilities/refresh, and the host
// pushes the new map to every open stream. Nothing here re-fetches on a timer.
import { useEffect, useState } from 'react'

export const PANEL_ID = 'rth.capabilities'
const STREAM = 'api/rth-capabilities/stream'

export const inject = ['slots']

export function apply(ctx) {
  ctx.effect(() => ctx.slots.inject('main', () => ctx.slots.register(
    { name: 'main', key: PANEL_ID },
    CapabilitiesPanel,
  )), 'rth-capabilities: main panel')
  ctx.effect(() => ctx.slots.inject('sidebar.panellist', () => ctx.slots.register(
    { name: 'sidebar.panellist', id: PANEL_ID, order: 50, label: () => 'What I can do' },
    PanelIcon,
  )), 'rth-capabilities: sidebar entry')
}

function PanelIcon({ size = 16 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden="true">
      <rect x="2" y="2" width="5" height="5" rx="1" />
      <rect x="9" y="2" width="5" height="5" rx="1" />
      <rect x="2" y="9" width="5" height="5" rx="1" />
      <path d="M9.5 11.5l1.5 1.5 3-3" />
    </svg>
  )
}

function useMapStream() {
  const [map, setMap] = useState(null)
  const [stream, setStream] = useState('opening')
  const [pushes, setPushes] = useState(0)
  useEffect(() => {
    const source = new EventSource(new URL(STREAM, document.baseURI))
    source.addEventListener('map', (e) => {
      setMap(JSON.parse(e.data))
      setPushes((n) => n + 1)
    })
    source.onopen = () => setStream('open')
    source.onerror = () => setStream('reconnecting')
    return () => source.close()
  }, [])
  return { map, stream, pushes }
}

const GROUPS = [
  ['capability', 'What I can do'],
  ['runtime', 'How I run'],
  ['moment', 'TML moments'],
  ['benchmark', 'TML benchmark rows'],
]
const STATUS_TEXT = {
  supported: 'supported',
  proxy: 'stand-in only',
  failing: 'failing',
  unverified: 'unverified evidence',
  unsupported: 'not yet',
}
const STATUS_TONE = {
  supported: 'rgba(46,160,67,0.18)',
  proxy: 'rgba(210,153,34,0.18)',
  failing: 'rgba(218,54,51,0.18)',
  unverified: 'rgba(210,153,34,0.18)',
  unsupported: 'rgba(127,127,127,0.12)',
}
const muted = { opacity: 0.65 }

function CapabilitiesPanel() {
  const { map, stream, pushes } = useMapStream()
  return (
    <div data-rth="capabilities" style={{ height: '100%', overflow: 'auto', padding: '24px 28px', fontSize: 14, lineHeight: 1.45 }}>
      <h1 style={{ fontSize: 22, margin: '0 0 4px' }}>What I can do</h1>
      <div style={muted}>Computed only from Unfold-Bench scorecards. A cell without evidence says so; nothing is assumed.</div>
      <div data-rth="capabilities-stream" data-state={stream} data-pushes={pushes} style={{ ...muted, fontSize: 12, marginTop: 4 }}>
        stream: {stream === 'open' ? 'live (pushed, no polling)' : stream} · maps received: {pushes}
      </div>
      {map ? <MapView map={map} /> : <div style={{ marginTop: 16 }}>Waiting for the first map…</div>}
    </div>
  )
}

function MapView({ map }) {
  const lmc = map.latest_measured_candidate
  return (
    <>
      <div data-rth="capabilities-runs" data-runs={map.runs} style={{ marginTop: 14, padding: '8px 10px', border: '1px solid rgba(127,127,127,0.3)', borderRadius: 6 }}>
        <div><b>{map.runs}</b> bench runs ({map.unverified_runs.length} unverified) · registry v{map.registry.version} {map.registry.sha256.slice(0, 12)}</div>
        <div style={muted}>
          {lmc ? `Latest measured candidate: ${lmc.backend}, agentd ${lmc.agentd_sha256.slice(0, 12)}, run ${lmc.run}. A measurement, not an activation receipt.` : 'No verified run yet.'}
        </div>
      </div>
      {GROUPS.map(([group, title]) => {
        const cells = map.cells.filter((c) => c.group === group)
        if (!cells.length) return null
        const counts = map.summary[group] ?? {}
        return (
          <section key={group} data-rth="capabilities-group" data-group={group} style={{ marginTop: 20 }}>
            <h2 style={{ fontSize: 16, margin: '0 0 2px' }}>{title}</h2>
            <div style={{ ...muted, fontSize: 12, marginBottom: 6 }} data-rth="capabilities-summary">
              {Object.entries(counts).filter(([, n]) => n).map(([s, n]) => `${n} ${STATUS_TEXT[s] ?? s}`).join(' · ')}
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 6 }}>
              {cells.map((c) => <Cell key={c.id} cell={c} />)}
            </div>
          </section>
        )
      })}
    </>
  )
}

function Cell({ cell }) {
  const score = cell.score === null ? null : `score ${cell.score.toFixed(2)}${cell.best === null ? '' : `, best ${cell.best.toFixed(2)} over ${cell.trend.length} runs`}`
  const metrics = cell.metrics.map((m) => `${m.metric} ${m.stat} ${m.value} ${m.unit}`).join(' · ')
  return (
    <div data-rth="capability-cell" data-id={cell.id} data-status={cell.status} style={{ background: STATUS_TONE[cell.status], borderRadius: 6, padding: '6px 8px' }}>
      <div style={{ fontWeight: 600 }}>{cell.title}</div>
      <div style={{ fontSize: 12 }}>{STATUS_TEXT[cell.status] ?? cell.status}{cell.reason_code ? ` (${cell.reason_code.replaceAll('_', ' ')})` : ''}</div>
      {score ? <div style={{ ...muted, fontSize: 12 }}>{score}</div> : null}
      {metrics ? <div style={{ ...muted, fontSize: 12 }}>{metrics}</div> : null}
    </div>
  )
}
