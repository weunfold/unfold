// @automagik/unfold-session-dsh, browser half.
// 1. Puts the one Unfold root Session on screen (uiWorkspace.openSession), so the composer the
//    person types into is the one the host plugin projects Unfold into.
// 2. An "Unfold status" right-sidebar tab: the adapter's link to agentd, its DSH model calls
//    (always none) and the latest MEASURED candidate (the capability map, when it is loaded; not
//    an activation receipt). All of it comes from the host route /api/unfold-session/status
//    (behind DSH's fence). Loaded on open and on Refresh; no timer.
// 3. No voice control by the composer: there is no Start call / End call button. Voice is the
//    avatar in the Unfold presence tab (@automagik/unfold-events-dsh), which reads this plugin's
//    /api/unfold-session/call route with src/call-view.js. In Unfold's instance calls and the
//    microphone belong to Unfold's own app, so that route reads as unavailable.
// 4. Live listening: a "listening…" bubble above the composer of the Unfold conversation, from
//    the moment speech starts until its words land in a chat bubble, read from
//    /api/unfold-session/live (one EventSource while the conversation is shown; src/live.js on
//    the host). Drawn with the Bot's own user-bubble classes, so it looks like the bubble it
//    becomes.
// 5. Tab buttons in the Bot header: Presence, Mind, Events, Settings and Status open their
//    right-sidebar tab directly, in the same place and style as the Trajectory button, in the
//    Unfold root Session only.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ANSWER_WITH_ZH } from './strings.js'

export const STATUS_ID = '@automagik/unfold-session-dsh'
export const STATUS_KIND = 'unfold.status'
export let ROOT_SESSION = 'unfold-root'
let STATUS = 'api/unfold-session/status'
let LIVE = 'api/unfold-session/live'

export const inject = ['slots', 'sidebarRightTabs', 'sidebarRight']

/** The header tabs (6), in the order the sidebar's "New tab" lists them. */
export const UNFOLD_TABS = [
  { kind: 'unfold.presence', label: 'Presence', title: 'Unfold presence', icon: 'M12 4a8 8 0 1 0 0 16a8 8 0 1 0 0-16M9 11h.01M15 11h.01' },
  { kind: 'unfold.mind', label: 'Mind', title: 'Mind: what Unfold did and why', icon: 'M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01' },
  { kind: 'unfold.events', label: 'Events', title: 'Unfold events', icon: 'M3 12h4l3-7 4 14 3-7h4' },
  { kind: 'unfold.settings', label: 'Setup', title: 'Unfold setup (settings)', icon: 'M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2' },
  { kind: 'unfold.status', label: 'Status', title: 'Unfold status', icon: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 11v5M12 8h.01' },
]
const TabIcon = ({ d }) => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>

// Overrides for the Bot surface. They only match the Bot's own class names, so they are inert in
// plain DSH. (1) Speech belongs to Unfold's own app: the Bot's browser dictation mic and
// voice-chat button are hidden, so the browser never becomes a second microphone owner.
// (2) Unfold answers in its root Session without a DSH model: the Bot's "I don't have a model
// yet" dock and its sign-in / API key / custom model onboarding are hidden. No model and
// no provider is configured to make them go away.
export const BOT_OVERRIDES_CSS = [
  '.bt-mic,.bt-voice,.bt-mic-note{display:none!important}',
  '.bt-model-dock{display:none!important}',
].join('\n')

// The listening bubble and the header tabs. The bubble borrows the Bot's .bt-umsg/.bt-ustack/.bt-ubody
// (installed by the Bot surface); the fallback below is for plain DSH, where they are absent.
export const LIVE_CSS = [
  '.unfold-live{padding:2px 12px 6px;transition:opacity .2s ease}',
  '.unfold-live[data-leaving]{opacity:0}',
  '.unfold-live .unfold-live-body{display:inline-flex;align-items:center;gap:8px}',
  '.unfold-live:not(.bt-umsg){display:flex;justify-content:flex-end}',
  '.unfold-live:not(.bt-umsg) .unfold-live-body{padding:6px 12px;border-radius:14px;background:var(--fill-secondary,#8882)}',
  '.unfold-live-word{opacity:.75;font-style:italic}',
  '.unfold-live-dots i{display:inline-block;width:4px;height:4px;margin:0 1px;border-radius:2px;background:currentColor;opacity:.4;animation:unfold-live-dot 1.2s infinite ease-in-out}',
  '.unfold-live-dots i:nth-child(2){animation-delay:.2s}.unfold-live-dots i:nth-child(3){animation-delay:.4s}',
  '@keyframes unfold-live-dot{0%,80%,100%{opacity:.25}40%{opacity:.9}}',
  '.unfold-live-meter{display:inline-flex;align-items:flex-end;gap:2px;height:12px}',
  '.unfold-live-meter b{display:block;width:3px;border-radius:1px;background:currentColor;opacity:.2}',
  '.unfold-live-meter b[data-on]{opacity:.75}',
  '@media (prefers-reduced-motion:reduce){.unfold-live-dots i{animation:none;opacity:.6}.unfold-live{transition:none}}',
  // DSH's session header is pointer-events:none (the transcript scrolls under it) and the Bot turns
  // them back on only for its own pill, so a header button added through omni.headerButtons was
  // drawn but unclickable: the click fell through to the chat column below (found by
  // hit-testing the header). This includes the Trajectory button, matched by its data-unfold.
  '.bt-head .unfold-tabs,.bt-head .unfold-tab-btn,.bt-head [data-unfold=trajectory-toggle]{pointer-events:auto}',
  '.unfold-tabs{display:inline-flex;align-items:center;gap:2px}',
  '.unfold-tabs .unfold-tab-btn{display:inline-flex;align-items:center;gap:4px;width:auto;padding:0 7px;font-size:12px}',
  // Compact (the head is too narrow for words, e.g. with the right sidebar open): icons only, and
  // the Trajectory button's word hidden too, so nothing runs under the header's side button.
  '.unfold-tabs[data-compact] .unfold-tab-word{display:none}',
  '.unfold-tabs[data-compact] .unfold-tab-btn{padding:0 5px}',
  '.bt-head.unfold-head-compact [data-unfold=trajectory-toggle] span{display:none}',
].join('\n')

/** 0..5 bars for an onset level in dBFS (−50 dBFS and below: 0; −10 and above: 5). */
export function barsOf(dbfs) {
  if (!Number.isFinite(dbfs)) return 0
  return Math.max(0, Math.min(5, Math.round((dbfs + 50) / 8)))
}

/** Tab buttons for the Bot header: each opens its sidebar tab in the Unfold session. */
export function makeTabButtons(open) {
  return function UnfoldHeaderTabs({ bot }) {
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
        head.classList.toggle('unfold-head-compact', small)
      }
      const ro = new ResizeObserver(fit)
      ro.observe(head)
      fit()
      return () => { ro.disconnect(); head.classList.remove('unfold-head-compact') }
    }, [session])
    if (session !== ROOT_SESSION) return null
    return (
      <span ref={ref} className="unfold-tabs" data-unfold="header-tabs" data-compact={compact || undefined} role="group" aria-label="Unfold tabs">
        {UNFOLD_TABS.map((t) => (
          <button key={t.kind} type="button" className="bt-icon-btn unfold-tab-btn" data-unfold={`tab-${t.kind}`} title={t.title} aria-label={`Open ${t.title} in the right sidebar`} onClick={() => open(t.kind)}>
            <TabIcon d={t.icon} />
            <span className="unfold-tab-word">{t.label}</span>
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

/** The listening bubble (conversation.input.dock, Unfold session only). */
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
    <div className="bt-umsg unfold-live" data-unfold="listening" data-state={shown.state} data-leaving={leaving || undefined} aria-live="polite">
      <div className="bt-ustack">
        <div className="bt-ubody unfold-live-body">
          {shown.text ? <span data-unfold="listening-text">{shown.text}</span> : (
            <>
              <span className="unfold-live-word" data-unfold="listening-word">listening</span>
              <span className="unfold-live-dots" aria-hidden="true"><i /><i /><i /></span>
            </>
          )}
          {shown.level && !shown.text ? (
            <span className="unfold-live-meter" data-unfold="listening-meter" data-bars={bars} title={meterTitle} aria-label={meterTitle}>
              {[1, 2, 3, 4, 5].map((n) => <b key={n} data-on={n <= bars || undefined} style={{ height: 3 + n * 2 }} />)}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  )
}

export async function apply(ctx) {
  const response = await fetch('api/unfold-session/config', { cache: 'no-store' })
  if (!response.ok) throw new Error(`Unfold configuration ${response.status}`)
  const config = await response.json()
  if (config.enabled === false) return
  ROOT_SESSION = config.sessionId
  const route = config.route.replace(/^\//, '').replace(/\/$/, '')
  STATUS = `${route}/status`
  LIVE = `${route}/live`

  ctx.effect(() => {
    const el = document.createElement('style')
    el.id = 'unfold-bot-overrides'
    el.dataset.unfold = 'bot-overrides'
    el.textContent = `${BOT_OVERRIDES_CSS}\n${LIVE_CSS}`
    document.head.append(el)
    return () => el.remove()
  }, 'unfold-session: Bot overrides (the app owns speech, no model onboarding)')
  ctx.inject(['uiWorkspace'], (sctx) => {
    sctx.effect(() => {
      // Once, at load: the root Session is the conversation; after the first successful open,
      // navigation stays the person's. The first tries can throw "unknown session unfold-root"
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
    }, 'unfold-session: open the root Session')
  })
  ctx.effect(() => ctx.sidebarRightTabs.register({
    id: STATUS_ID,
    kind: STATUS_KIND,
    priority: 'extension',
    title: () => 'Unfold status',
    guide: [{ id: 'unfold-status', order: 91, title: () => 'Unfold status', description: () => 'The link to the agent and the latest measured candidate' }],
  }), 'unfold-session: tab type')
  ctx.effect(() => () => {
    for (const tab of ctx.sidebarRight.openTabs.getSnapshot()) {
      if (tab.kind !== STATUS_KIND) continue
      try {
        ctx.sidebarRight.closeIn(tab.sessionId, tab.tabId)
      } catch {}
    }
  }, 'unfold-session: close open tabs on unload')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register({ name: 'sidebar.right.pane.tab', key: STATUS_ID }, StatusBody)), 'unfold-session: tab body')
  // The listening bubble sits in the composer's dock zone, above the input, where the next user
  // bubble will appear; a list slot, session scope: it draws for the Unfold root Session only.
  ctx.effect(() => ctx.slots.inject('conversation.input.dock', () => ctx.slots.register({ name: 'conversation.input.dock', id: 'unfold-listening', order: -200 }, ListeningDock)), 'unfold-session: listening bubble')
  // The Bot profile: tab buttons in the chat header (absent in plain DSH, where this is a no-op).
  let omni = null
  try {
    // Resolved at run time through the shell's module table (package.json dsh.client.external).
    const id = ['@automagik', 'bots-dsh', 'client'].join('/') // not a literal: esbuild leaves it to the factory's require
    omni = (typeof require === 'function' ? require(id) : null)?.omni ?? null
  } catch {}
  // The Unfold Bot's model menu (the host's src/answer-with.js) can show these from the host:
  // the Bot team's dictionary words them in Chinese (./strings.js).
  if (typeof omni?.addStrings === 'function') ctx.effect(() => omni.addStrings(ANSWER_WITH_ZH), 'unfold-session: model menu copy')
  if (omni?.headerButtons !== undefined) ctx.effect(() => omni.headerButtons.register({
    id: 'unfold-tabs',
    order: 40,
    Component: makeTabButtons((kind) => {
      ctx.sidebarRight.openTabIn(ROOT_SESSION, kind)
      if (ctx.sidebarRight.isExpanded?.() === false) ctx.sidebarRight.toggleExpanded?.()
    }),
  }), 'unfold-session: Bot header tab buttons')
  ctx.effect(() => ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register({ name: 'sidebar.right.pane.tab.title', key: STATUS_ID }, StatusTitle)), 'unfold-session: tab title')
}

function StatusTitle({ useTabInfo }) {
  const { tab } = useTabInfo()
  return <span data-unfold="status-title">{tab.title}</span>
}

const short = (s, n = 12) => (typeof s === 'string' && s.length > n ? `${s.slice(0, n)}…` : (s ?? '—'))
const card = { border: '1px solid var(--border, #8884)', borderRadius: 8, padding: '8px 10px', margin: '8px 0' }
const label = { fontSize: 11, opacity: 0.7, textTransform: 'uppercase', letterSpacing: 0.4 }

function Row({ k, v, id }) {
  return (
    <div style={{ display: 'flex', gap: 8, fontSize: 12 }}>
      <span style={{ opacity: 0.7, minWidth: 96 }}>{k}</span>
      <span data-unfold={id} style={{ fontFamily: 'var(--font-mono, monospace)', wordBreak: 'break-all' }}>{v === null || v === undefined ? '—' : String(v)}</span>
    </div>
  )
}

function StatusBody() {
  const [s, setS] = useState(null)
  const [err, setErr] = useState(null)
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
  const link = s?.session?.adapter_link
  return (
    <div data-unfold="status" style={{ padding: 10, fontSize: 13 }}>
      <div style={{ display: 'flex', gap: 8 }}>
        <button data-unfold="status-refresh" onClick={() => void load()}>Refresh</button>
      </div>
      {err && <div data-unfold="status-error" style={{ color: 'var(--danger, #c33)' }}>status unavailable: {err}</div>}
      <div style={card} data-unfold="card-link">
        <div style={label}>Agent link (read only)</div>
        <Row k="link" v={link?.state} id="link-state" />
        <Row k="boot" v={link?.boot_id} id="boot-id" />
        <Row k="session" v={link?.session_id} id="session-id" />
        <Row k="generation" v={link?.command_generation} id="generation" />
        <Row k="DSH model calls" v={s ? `${s.dsh.model_calls.llm_stream} streams, ${s.dsh.model_calls.agent_request} requests` : null} id="model-calls" />
      </div>
      <div style={card} data-unfold="card-candidate">
        <div style={label}>Latest measured candidate (capability map, not an activation receipt)</div>
        <Row k="run" v={s?.candidate?.run ?? s?.candidate?.unavailable} id="candidate-run" />
        <Row k="backend" v={s?.candidate?.backend} id="candidate-backend" />
        <Row k="agentd" v={short(s?.candidate?.agentd_sha256, 16)} id="candidate-agentd" />
      </div>
    </div>
  )
}
