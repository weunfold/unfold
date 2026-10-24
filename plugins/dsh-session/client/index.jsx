// @rth/dsh-session, browser half.
// 1. Puts the one RTH root Session on screen (uiWorkspace.openSession), so the composer the
//    person types into is the one the host plugin projects RTH into.
// 2. An "RTH status" right-sidebar tab, the same two-stage path as RTH events. It shows three
//    things kept apart: the ACTIVE artifact with its activation receipt (controller record),
//    the latest MEASURED candidate (capability map, not an activation receipt), and the
//    self-repo HEAD with its dirty state. All of it comes from the host route
//    /api/rth-session/status (behind DSH's fence); the browser never reads the controller
//    store. Loaded on open and on Refresh; no timer.
// 3. A Stop button that asks the host owner to stop speech and cancel. It is a convenience:
//    the native stop lives in the host owner and works without this page.
// 4. No voice control by the composer: RTH is realtime all the time, there is no call button.
//    Voice is the avatar in the RTH presence tab (@rth/dsh-events): a tap wakes or sleeps
//    rth-host's devices through this plugin's /api/rth-session/call and /api/rth-session/mute
//    routes, read and shown with src/call-view.mjs. The RTH menu and tray keep Wake voice /
//    Sleep voice. Typed conversation and Stop speaking are unaffected.
// 5. Live listening: a "listening…" bubble above the composer of the RTH conversation, from the
//    moment speech starts until its words land in a chat bubble, read from /api/rth-session/live
//    (one EventSource while the conversation is shown; src/live.mjs on the host). Drawn with the
//    Bot's own user-bubble classes, so it looks like the bubble it becomes. Its small meter is the
//    level AT SPEECH ONSET, the only level rth-host measures; it says so on hover, and it is not a
//    live level.
// 6. Tab buttons in the Bot header, so a tab opens in one step:
//    Presence, Mind, Events, Settings and Status open their right-sidebar tab directly, in the same
//    place and style as the Trajectory button, RTH Main Bot (rth-root) only.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

export const RTH_STATUS_ID = '@rth/dsh-session'
export const RTH_STATUS_KIND = 'rth.status'
export const ROOT_SESSION = 'rth-root'
const STATUS = 'api/rth-session/status'
const STOP = 'api/rth-session/stop'
const LIVE = 'api/rth-session/live'

export const inject = ['slots', 'sidebarRightTabs', 'sidebarRight']

/** The header tabs (6), in the order the sidebar's "New tab" lists them. */
export const RTH_TABS = [
  { kind: 'rth.presence', label: 'Presence', title: 'Unfold presence', icon: 'M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M9 11h.01M15 11h.01' },
  { kind: 'rth.mind', label: 'Mind', title: 'Mind: what Unfold did and why', icon: 'M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01' },
  { kind: 'rth.events', label: 'Events', title: 'Unfold events', icon: 'M3 12h4l3-7 4 14 3-7h4' },
  { kind: 'rth.settings', label: 'Setup', title: 'Unfold setup (settings)', icon: 'M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2' },
  { kind: 'rth.status', label: 'Status', title: 'Unfold status', icon: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v5M12 8h.01' },
]
const TabIcon = ({ d }) => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>

// RTH-owned overrides for the vendored Bot surface (shell/vendor/omni-ui-dsh, unmodified). They
// only match the Bot's own class names, so they are inert in plain DSH. (1) Speech belongs to the
// native host owner (rth-host --voice): the Bot's browser dictation mic and voice-chat button are
// hidden, so the browser never becomes a second microphone owner. (2) RTH answers in rth-root
// without a DSH model: the Bot's "I don't have a model yet" dock and its sign-in / API key / custom
// model onboarding are hidden. No model and no provider is configured to make them go away.
export const BOT_OVERRIDES_CSS = [
  '.bt-mic,.bt-voice,.bt-mic-note{display:none!important}',
  '.bt-model-dock{display:none!important}',
].join('\n')

// The listening bubble and the header tabs. The bubble borrows the Bot's .bt-umsg/.bt-ustack/.bt-ubody
// (installed by the Bot surface); the fallback below is for plain DSH, where they are absent.
export const LIVE_CSS = [
  '.rth-live{padding:2px 12px 6px;transition:opacity .2s ease}',
  '.rth-live[data-leaving]{opacity:0}',
  '.rth-live .rth-live-body{display:inline-flex;align-items:center;gap:8px}',
  '.rth-live:not(.bt-umsg){display:flex;justify-content:flex-end}',
  '.rth-live:not(.bt-umsg) .rth-live-body{padding:6px 12px;border-radius:14px;background:var(--fill-secondary,#8882)}',
  '.rth-live-word{opacity:.75;font-style:italic}',
  '.rth-live-dots i{display:inline-block;width:4px;height:4px;margin:0 1px;border-radius:2px;background:currentColor;opacity:.4;animation:rth-live-dot 1.2s infinite ease-in-out}',
  '.rth-live-dots i:nth-child(2){animation-delay:.2s}.rth-live-dots i:nth-child(3){animation-delay:.4s}',
  '@keyframes rth-live-dot{0%,80%,100%{opacity:.25}40%{opacity:.9}}',
  '.rth-live-meter{display:inline-flex;align-items:flex-end;gap:2px;height:12px}',
  '.rth-live-meter b{display:block;width:3px;border-radius:1px;background:currentColor;opacity:.2}',
  '.rth-live-meter b[data-on]{opacity:.75}',
  '@media (prefers-reduced-motion:reduce){.rth-live-dots i{animation:none;opacity:.6}.rth-live{transition:none}}',
  // DSH's session header is pointer-events:none (the transcript scrolls under it) and the Bot turns
  // them back on only for its own pill, so a header button added through omni.headerButtons would
  // be drawn but unclickable: the click falls through to the chat column below. This includes the
  // Trajectory button, matched by its data-rth.
  '.bt-head .rth-tabs,.bt-head .rth-tab-btn,.bt-head [data-rth=trajectory-toggle]{pointer-events:auto}',
  '.rth-tabs{display:inline-flex;align-items:center;gap:2px}',
  '.rth-tabs .rth-tab-btn{display:inline-flex;align-items:center;gap:4px;width:auto;padding:0 7px;font-size:12px}',
  // Compact (the head is too narrow for words, e.g. with the right sidebar open): icons only, and
  // the Trajectory button's word hidden too, so nothing runs under the header's side button.
  '.rth-tabs[data-compact] .rth-tab-word{display:none}',
  '.rth-tabs[data-compact] .rth-tab-btn{padding:0 5px}',
  '.bt-head.rth-head-compact [data-rth=trajectory-toggle] span{display:none}',
].join('\n')

/** 0..5 bars for an onset level in dBFS (−50 dBFS and below: 0; −10 and above: 5). */
export function barsOf(dbfs) {
  if (!Number.isFinite(dbfs)) return 0
  return Math.max(0, Math.min(5, Math.round((dbfs + 50) / 8)))
}

/** Tab buttons for the Bot header: each opens its sidebar tab in the RTH session. */
export function makeTabButtons(open) {
  return function RthHeaderTabs({ bot }) {
    const session = bot?.sessionId ?? bot?.id
    const ref = useRef(null)
    const full = useRef(0)
    const [compact, setCompact] = useState(false)
    // Words only while they fit: the head centres its row, so the whole row (the Bot's pill, these
    // tabs, the Trajectory button) must fit inside it with room for the side button on each side.
    useLayoutEffect(() => {
      const el = ref.current
      const head = el?.closest('.bt-head')
      if (!el || !head || typeof ResizeObserver === 'undefined') return undefined
      const fit = () => {
        if (!el.hasAttribute('data-compact')) full.current = el.getBoundingClientRect().width
        const others = [...head.children].filter((c) => c !== el && getComputedStyle(c).position !== 'absolute').reduce((w, c) => w + c.getBoundingClientRect().width, 0)
        const side = [...head.children].filter((c) => getComputedStyle(c).position === 'absolute').reduce((w, c) => Math.max(w, c.getBoundingClientRect().width), 0)
        const small = head.clientWidth < others + full.current + 2 * (side + 16)
        setCompact(small)
        head.classList.toggle('rth-head-compact', small)
      }
      const ro = new ResizeObserver(fit)
      ro.observe(head)
      fit()
      return () => { ro.disconnect(); head.classList.remove('rth-head-compact') }
    }, [session])
    if (session !== ROOT_SESSION) return null
    return (
      <span ref={ref} className="rth-tabs" data-rth="header-tabs" data-compact={compact || undefined} role="group" aria-label="Unfold tabs">
        {RTH_TABS.map((t) => (
          <button key={t.kind} type="button" className="bt-icon-btn rth-tab-btn" data-rth={`tab-${t.kind}`} title={t.title} aria-label={`Open ${t.title} in the right sidebar`} onClick={() => open(t.kind)}>
            <TabIcon d={t.icon} />
            <span className="rth-tab-word">{t.label}</span>
          </button>
        ))}
      </span>
    )
  }
}

/**
 * The live view's state, from the host's push stream: one EventSource and at most two timers while
 * mounted. What it shows never outlives its evidence:
 *   - speech: each view carries ttl_ms, its remaining lifetime when sent; it ends here at that
 *     deadline even if the host's idle never arrives (a suspended page, a lost connection);
 *   - connection: any frame (a view or the host's 5 s beat) renews a lease; silence beyond it, or a
 *     stream error, drops what is shown at once; a beat never extends speech;
 *   - a session change or unmount drops it, and nothing old is revived: only a new snapshot or
 *     view, which the host computes fresh, shows anything again.
 */
function useLive(on) {
  const leaseMs = 12000 // any frame (a view or the 5 s beat) renews it
  const capMs = 30000 // no speech view is kept longer than the host's own cap
  const [v, setV] = useState(null)
  useEffect(() => {
    if (!on || typeof EventSource === 'undefined') {
      setV(null)
      return undefined
    }
    const later = typeof setTimeout === 'function' ? setTimeout : null
    const cancel = typeof clearTimeout === 'function' ? clearTimeout : () => {}
    let speech = null
    let lease = null
    let live = true
    const drop = () => { if (live) setV(null) }
    const renew = () => {
      cancel(lease)
      lease = later ? later(drop, leaseMs) : null
    }
    const es = new EventSource(LIVE)
    const take = (e) => {
      let x
      try { x = JSON.parse(e.data) } catch { return }
      renew()
      cancel(speech)
      speech = null
      if (x && x.state && x.state !== 'idle') {
        // A view without its lifetime (an older host) gets a short bounded one, never an open-ended one.
        const ttl = Number.isFinite(x.ttl_ms) ? Math.max(0, Math.min(x.ttl_ms, capMs)) : 8000
        if (ttl <= 0) return drop()
        speech = later ? later(drop, ttl) : null
      }
      setV(x)
    }
    es.addEventListener('snapshot', take)
    es.addEventListener('live', take)
    es.addEventListener('beat', renew)
    es.addEventListener('error', () => { cancel(speech); speech = null; drop() })
    renew()
    return () => {
      live = false
      cancel(speech)
      cancel(lease)
      es.close()
      setV(null)
    }
  }, [on])
  return v
}

/** The listening bubble (conversation.input.dock, RTH session only). */
export function ListeningDock({ sessionId }) {
  const on = sessionId === ROOT_SESSION
  const v = useLive(on)
  const [shown, setShown] = useState(null)
  const [leaving, setLeaving] = useState(false)
  const timer = useRef(null)
  useEffect(() => {
    clearTimeout(timer.current)
    if (v && v.state !== 'idle') {
      setShown(v)
      setLeaving(false)
    } else if (shown) {
      setLeaving(true)
      timer.current = setTimeout(() => { setShown(null); setLeaving(false) }, 220)
    }
    return () => clearTimeout(timer.current)
  }, [v])
  if (!on || !shown) return null
  const bars = shown.level ? barsOf(shown.level.dbfs) : 0
  const meterTitle = shown.level ? `${shown.level.basis}: ${shown.level.dbfs} dBFS (not a live level)` : null
  return (
    <div className="bt-umsg rth-live" data-rth="listening" data-state={shown.state} data-leaving={leaving || undefined} aria-live="polite">
      <div className="bt-ustack">
        <div className="bt-ubody rth-live-body">
          {shown.text ? <span data-rth="listening-text">{shown.text}</span> : (
            <>
              <span className="rth-live-word" data-rth="listening-word">listening</span>
              <span className="rth-live-dots" aria-hidden="true"><i /><i /><i /></span>
            </>
          )}
          {shown.level && !shown.text ? (
            <span className="rth-live-meter" data-rth="listening-meter" data-bars={bars} title={meterTitle} aria-label={meterTitle}>
              {[1, 2, 3, 4, 5].map((n) => <b key={n} data-on={n <= bars || undefined} style={{ height: 3 + n * 2 }} />)}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  )
}

export function apply(ctx) {
  ctx.effect(() => {
    const el = document.createElement('style')
    el.id = 'rth-bot-overrides'
    el.dataset.rth = 'bot-overrides'
    el.textContent = `${BOT_OVERRIDES_CSS}\n${LIVE_CSS}`
    document.head.append(el)
    return () => el.remove()
  }, 'rth-session: Bot overrides (native speech owner, no model onboarding)')
  ctx.inject(['uiWorkspace'], (sctx) => {
    sctx.effect(() => {
      // Once, at load: the root Session is the conversation; after the first successful open,
      // navigation stays the person's. The first tries can throw "unknown session rth-root"
      // while the client's session catalog has not listed it yet (the host creates it at
      // start), so retry a bounded number of times (about 15 s), then give up quietly.
      let tries = 0
      let timer = null
      const attempt = () => {
        try {
          sctx.uiWorkspace.openSession(ROOT_SESSION)
          return
        } catch {}
        if (++tries < 60) timer = setTimeout(attempt, 250)
      }
      timer = setTimeout(attempt, 0)
      return () => clearTimeout(timer)
    }, 'rth-session: open the root Session')
  })
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: RTH_STATUS_ID,
    kind: RTH_STATUS_KIND,
    priority: 'extension',
    title: () => 'Unfold status',
    guide: [{ id: 'rth-status', order: 91, title: () => 'Unfold status', description: () => 'Active artifact, latest measured candidate, self-repo' }],
  }), 'rth-session: tab type')
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== RTH_STATUS_KIND) continue
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId)
      } catch {}
    }
  }, 'rth-session: close open tabs on unload')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register({ name: 'sidebar.right.pane.tab', key: RTH_STATUS_ID }, StatusBody)), 'rth-session: tab body')
  // The listening bubble sits in the composer's dock zone, above the input, where the next user
  // bubble will appear; a list slot, session scope: it draws for rth-root only.
  ctx.effect(() => ctx.slots.inject('conversation.input.dock', () => ctx.slots.register({ name: 'conversation.input.dock', id: 'rth-listening', order: -200 }, ListeningDock)), 'rth-session: listening bubble')
  // The Bot profile: tab buttons in the chat header (absent in plain DSH, where this is a no-op).
  let omni = null
  try {
    // Resolved at run time through the shell's module table (package.json dsh.client.external).
    const id = ['@automagik', 'omni-dsh', 'client'].join('/') // not a literal: esbuild leaves it to the factory's require
    omni = (typeof require === 'function' ? require(id) : null)?.omni ?? null
  } catch {}
  if (omni?.headerButtons !== undefined) ctx.effect(() => omni.headerButtons.register({
    id: 'rth-tabs',
    order: 40,
    Component: makeTabButtons((kind) => {
      ctx.sidebarRight.openTabIn(ROOT_SESSION, kind)
      if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.()
    }),
  }), 'rth-session: Bot header tab buttons')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register({ name: 'sidebar.right.pane.tab.title', key: RTH_STATUS_ID }, StatusTitle)), 'rth-session: tab title')
}

function StatusTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-rth="status-title">{tab.title}</span>
}

const short = (s, n = 12) => (typeof s === 'string' && s.length > n ? `${s.slice(0, n)}…` : (s ?? '—'))
const card = { border: '1px solid var(--border, #8884)', borderRadius: 8, padding: '8px 10px', margin: '8px 0' }
const label = { fontSize: 11, opacity: 0.7, textTransform: 'uppercase', letterSpacing: 0.4 }

function Row({ k, v, rth }) {
  return (
    <div style={{ display: 'flex', gap: 8, fontSize: 12 }}>
      <span style={{ opacity: 0.7, minWidth: 96 }}>{k}</span>
      <span data-rth={rth} style={{ fontFamily: 'var(--font-mono, monospace)', wordBreak: 'break-all' }}>{v === null || v === undefined ? '—' : String(v)}</span>
    </div>
  )
}

function StatusBody() {
  const [s, setS] = useState(null)
  const [err, setErr] = useState(null)
  const [stop, setStop] = useState(null)
  const load = useCallback(async () => {
    try {
      const r = await fetch(STATUS, { cache: 'no-store' })
      if (!r.ok) throw new Error(`status ${r.status}`)
      setS(await r.json())
      setErr(null)
    } catch (e) {
      setErr(String(e.message ?? e))
    }
  }, [])
  useEffect(() => { void load() }, [load])
  const onStop = async () => {
    const r = await fetch(STOP, { method: 'POST' })
    setStop(r.ok ? 'stop sent to the host owner' : `stop failed (${r.status})`)
  }
  const active = s?.active?.active
  const link = s?.owner?.link
  return (
    <div data-rth="status" style={{ padding: 10, fontSize: 13 }}>
      <div style={{ display: 'flex', gap: 8 }}>
        <button data-rth="status-refresh" onClick={() => void load()}>Refresh</button>
        <button data-rth="status-stop" onClick={() => void onStop()}>Stop speaking</button>
      </div>
      {stop && <div data-rth="stop-result" style={{ fontSize: 12, marginTop: 4 }}>{stop}</div>}
      {err && <div data-rth="status-error" style={{ color: 'var(--danger, #c33)' }}>status unavailable: {err}</div>}
      <div style={card} data-rth="card-link">
        <div style={label}>Host owner and agent link</div>
        <Row k="link" v={link?.state} rth="link-state" />
        <Row k="boot" v={link?.boot_id} rth="boot-id" />
        <Row k="session" v={link?.session_id} rth="session-id" />
        <Row k="generation" v={link?.command_generation} rth="generation" />
        <Row k="backend" v={link?.backend} rth="backend" />
        <Row k="voice" v={s?.owner?.host?.voice} rth="voice" />
        <Row k="voice state" v={s?.owner?.host?.call ? `${s.owner.host.call.state} (desired ${s.owner.host.call.desired}): ${s.owner.host.call.reason}` : null} rth="call-state" />
        <Row k="DSH model calls" v={s ? `${s.dsh.model_calls.llm_stream} streams, ${s.dsh.model_calls.agent_request} requests` : null} rth="model-calls" />
      </div>
      <div style={card} data-rth="card-active">
        <div style={label}>Active artifact (controller record)</div>
        {active ? (
          <>
            <Row k="deployment" v={active.deployment_id} rth="deployment-id" />
            <Row k="artifact" v={short(active.artifact_id, 16)} rth="artifact-id" />
            <Row k="attestation" v={active.attestation} rth="attestation" />
            <Row k="receipt" v={short(active.receipt?.sha256, 16)} rth="receipt-sha" />
            <Row k="receipt boot" v={active.receipt?.boot_id} rth="receipt-boot" />
            <Row k="backend" v={active.receipt?.backend} rth="receipt-backend" />
          </>
        ) : (
          <Row k="active" v={s?.active?.note ?? s?.active?.error ?? (s ? 'none' : null)} rth="deployment-id" />
        )}
      </div>
      <div style={card} data-rth="card-candidate">
        <div style={label}>Latest measured candidate (capability map, not an activation receipt)</div>
        <Row k="run" v={s?.candidate?.run ?? s?.candidate?.unavailable} rth="candidate-run" />
        <Row k="backend" v={s?.candidate?.backend} rth="candidate-backend" />
        <Row k="agentd" v={short(s?.candidate?.agentd_sha256, 16)} rth="candidate-agentd" />
      </div>
      <div style={card} data-rth="card-source">
        <div style={label}>Self-repo (source)</div>
        <Row k="HEAD" v={short(s?.source?.head, 12) ?? s?.source?.note} rth="repo-head" />
        <Row k="dirty" v={s?.source ? String(s.source.dirty ?? s.source.note ?? s.source.error) : null} rth="repo-dirty" />
        {s?.source?.dirty_paths?.length ? <Row k="paths" v={s.source.dirty_paths.join(', ')} rth="repo-paths" /> : null}
      </div>
    </div>
  )
}
