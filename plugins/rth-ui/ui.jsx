// The look shared by every RTH sidebar tab (presence, Mind, settings, events). Bundled into each
// plugin's browser half by relative import; it is not a package and has no server half.
//
// One small vocabulary, so the tabs read as one product:
// - tokens on `.rth-ui` (light and dark through the page's own color-scheme, via light-dark());
// - <Fold>: a disclosure whose open/closed state is remembered per viewer (localStorage, best
//   effort: a private window or blocked storage just starts closed);
// - <Boundary>: a tab that throws shows one calm line instead of an empty pane;
// - useStyle: one <style> per tab in <head> (replaced in place on a hot swap), never inside a slot;
// - useNow / ago: a clock for "12 s ago" that stops while the page is hidden.
import { Component, useEffect, useId, useState } from 'react'

export const BASE_CSS = `
.rth-ui{--rth-fg:light-dark(#0c2630,#e4f4f2);--rth-muted:light-dark(#4f636c,#9db1b8);--rth-faint:light-dark(#7d8d94,#6c8088);
 --rth-line:light-dark(rgba(12,38,48,.11),rgba(228,244,242,.09));--rth-soft:light-dark(rgba(12,38,48,.035),rgba(228,244,242,.035));
 --rth-hover:light-dark(rgba(12,38,48,.06),rgba(228,244,242,.06));--rth-accent:light-dark(#08808f,#62e6f2);
 --rth-ok:light-dark(#1b7a4a,#74e3a9);--rth-warn:light-dark(#995600,#ffc56e);--rth-bad:light-dark(#b0281f,#ff9584);
 --rth-mono:"SF Mono",ui-monospace,Menlo,monospace;--rth-ease:cubic-bezier(.2,.7,.2,1);
 color:var(--rth-fg);font-size:13px;line-height:1.5;-webkit-font-smoothing:antialiased;box-sizing:border-box;height:100%;overflow:auto}
.rth-ui *,.rth-ui *::before,.rth-ui *::after{box-sizing:border-box}
.rth-ui :focus-visible{outline:2px solid var(--rth-accent);outline-offset:2px;border-radius:6px}
.rth-ui button{font:inherit;color:inherit}
.rth-muted{color:var(--rth-muted)}
.rth-faint{color:var(--rth-faint)}
.rth-num{font-variant-numeric:tabular-nums}
.rth-mono{font-family:var(--rth-mono);font-size:11px;line-height:1.45;overflow-wrap:anywhere}
.rth-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--rth-faint);flex:none;vertical-align:middle}
.rth-dot[data-tone=ok]{background:var(--rth-ok)}
.rth-dot[data-tone=warn]{background:var(--rth-warn)}
.rth-dot[data-tone=bad]{background:var(--rth-bad)}
.rth-dot[data-tone=accent]{background:var(--rth-accent)}
.rth-dot[data-pulse=true]{animation:rth-pulse 2.4s var(--rth-ease) infinite}
@keyframes rth-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in oklab,currentColor 0%,transparent)}50%{box-shadow:0 0 0 4px color-mix(in oklab,var(--rth-ok) 18%,transparent)}}
.rth-fold{border-top:1px solid var(--rth-line)}
.rth-fold-head{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:8px;width:100%;padding:9px 2px;cursor:pointer;color:var(--rth-muted);font-size:12.5px;border-radius:6px}
.rth-fold-head:hover{color:var(--rth-fg)}
.rth-fold-head:focus-visible{outline:2px solid var(--rth-accent);outline-offset:1px}
.rth-chev{width:12px;height:12px;flex:none;transition:transform .2s var(--rth-ease)}
.rth-fold-head[aria-expanded=true] .rth-chev{transform:rotate(90deg)}
.rth-fold-body{display:grid;grid-template-rows:0fr;transition:grid-template-rows .22s var(--rth-ease)}
.rth-fold-body[data-open=true]{grid-template-rows:1fr}
.rth-fold-body>div{overflow:hidden;min-height:0}
.rth-fold-inner{padding:0 2px 12px}
.rth-facts{display:grid;grid-template-columns:auto 1fr;gap:5px 14px;margin:0}
.rth-facts dt{color:var(--rth-muted)}
.rth-facts dd{margin:0}
.rth-crash{padding:16px;color:var(--rth-muted)}
@media (prefers-reduced-motion: reduce){.rth-ui *,.rth-ui *::before,.rth-ui *::after{transition:none!important;animation:none!important}}
`

/** One <style id> in <head> per tab family; a hot swap replaces its text instead of adding another. */
export function useStyle(id, css) {
  useEffect(() => {
    if (typeof document === 'undefined' || !document.head) return
    let el = document.getElementById(id)
    if (!el) {
      el = document.createElement('style')
      el.id = id
      document.head.append(el)
    }
    if (el.textContent !== css) el.textContent = css
  }, [id, css])
}

const store = {
  get(key) {
    try { return globalThis.localStorage?.getItem(`rth-ui:${key}`) ?? null } catch { return null }
  },
  set(key, value) {
    try { globalThis.localStorage?.setItem(`rth-ui:${key}`, value) } catch {}
  },
}

/** A small piece of per-viewer UI state (which folds are open, a chosen view) that survives reloads. */
export function useRemembered(key, initial) {
  const [value, setValue] = useState(() => {
    const raw = store.get(key)
    if (raw === null) return initial
    try { return JSON.parse(raw) } catch { return initial }
  })
  const set = (next) => {
    setValue((prev) => {
      const v = typeof next === 'function' ? next(prev) : next
      store.set(key, JSON.stringify(v))
      return v
    })
  }
  return [value, set]
}

const Chevron = () => (
  <svg className="rth-chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
)

/** A remembered disclosure: a plain summary line, the rest only on request. */
export function Fold({ id, summary, children, defaultOpen = false, className = '', ...rest }) {
  const [open, setOpen] = useRemembered(`fold:${id}`, defaultOpen)
  const bodyId = useId()
  return (
    <section className={`rth-fold ${className}`} data-open={open} {...rest}>
      <button type="button" className="rth-fold-head" aria-expanded={open} aria-controls={bodyId} onClick={() => setOpen((o) => !o)}>
        <Chevron />
        <span>{summary}</span>
      </button>
      <div className="rth-fold-body" id={bodyId} data-open={open}>
        <div><div className="rth-fold-inner">{open ? children : null}</div></div>
      </div>
    </section>
  )
}

/** A tab body that throws renders one calm line (and logs the error) instead of an empty pane. */
export class Boundary extends Component {
  constructor(props) {
    super(props)
    this.state = { error: null }
  }
  static getDerivedStateFromError(error) {
    return { error }
  }
  componentDidCatch(error) {
    console.error(`[${this.props.name ?? 'rth'}] the tab failed to draw`, error)
  }
  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="rth-ui"><div className="rth-crash" role="alert">This tab hit a problem and stopped drawing. Reopen it to try again.</div></div>
    )
  }
}

/** Wrap a tab body component so a crash in it is contained. */
export function guarded(name, Body) {
  const Guarded = (props) => <Boundary name={name}><Body {...props} /></Boundary>
  Guarded.displayName = `Guarded(${name})`
  return Guarded
}

/** A wall clock that ticks every `ms` while the page is visible (for relative times). */
export function useNow(ms = 1000) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    let t = null
    const run = () => {
      clearInterval(t)
      if (document.visibilityState === 'hidden') return
      setNow(Date.now())
      t = setInterval(() => setNow(Date.now()), ms)
    }
    run()
    document.addEventListener('visibilitychange', run)
    return () => {
      clearInterval(t)
      document.removeEventListener('visibilitychange', run)
    }
  }, [ms])
  return now
}

/** "just now", "12 s ago", "4 min ago", then the clock time. */
export function ago(at, now) {
  if (!Number.isFinite(at)) return 'unknown'
  const s = Math.max(0, Math.round((now - at) / 1000))
  if (s < 5) return 'just now'
  if (s < 60) return `${s} s ago`
  const m = Math.round(s / 60)
  if (m < 60) return `${m} min ago`
  return `at ${clock(at)}`
}

export const clock = (at) => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
export const clockS = (at) => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })

export const Dot = ({ tone = 'neutral', pulse = false, label }) => (
  <span className="rth-dot" data-tone={tone} data-pulse={pulse} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true} />
)
