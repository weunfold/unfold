// The look shared by every Unfold sidebar tab (presence, Mind, settings, events). Bundled into each
// plugin's browser half by relative import; it is not a package and has no server half.
//
// One small vocabulary, so the tabs read as one product:
// - tokens on `.unfold-ui` (light and dark through the page's own color-scheme, via light-dark());
// - <Fold>: a disclosure whose open/closed state is remembered per viewer (localStorage, best
//   effort: a private window or blocked storage just starts closed);
// - <Boundary>: a tab that throws shows one calm line instead of an empty pane;
// - useStyle: one <style> per tab in <head> (replaced in place on a hot swap), never inside a slot;
// - useNow / ago: a clock for "12 s ago" that stops while the page is hidden.
import { Component, useEffect, useId, useState } from 'react'

export const BASE_CSS = `
.unfold-ui{--unfold-fg:light-dark(#0c2630,#e4f4f2);--unfold-muted:light-dark(#4f636c,#9db1b8);--unfold-faint:light-dark(#7d8d94,#6c8088);
 --unfold-line:light-dark(rgba(12,38,48,.11),rgba(228,244,242,.09));--unfold-soft:light-dark(rgba(12,38,48,.035),rgba(228,244,242,.035));
 --unfold-hover:light-dark(rgba(12,38,48,.06),rgba(228,244,242,.06));--unfold-accent:light-dark(#08808f,#62e6f2);
 --unfold-ok:light-dark(#1b7a4a,#74e3a9);--unfold-warn:light-dark(#995600,#ffc56e);--unfold-bad:light-dark(#b0281f,#ff9584);
 --unfold-mono:"SF Mono",ui-monospace,Menlo,monospace;--unfold-ease:cubic-bezier(.2,.7,.2,1);
 color:var(--unfold-fg);font-size:13px;line-height:1.5;-webkit-font-smoothing:antialiased;box-sizing:border-box;height:100%;overflow:auto}
.unfold-ui *,.unfold-ui *::before,.unfold-ui *::after{box-sizing:border-box}
.unfold-ui :focus-visible{outline:2px solid var(--unfold-accent);outline-offset:2px;border-radius:6px}
.unfold-ui button{font:inherit;color:inherit}
.unfold-muted{color:var(--unfold-muted)}
.unfold-faint{color:var(--unfold-faint)}
.unfold-num{font-variant-numeric:tabular-nums}
.unfold-mono{font-family:var(--unfold-mono);font-size:11px;line-height:1.45;overflow-wrap:anywhere}
.unfold-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:var(--unfold-faint);flex:none;vertical-align:middle}
.unfold-dot[data-tone=ok]{background:var(--unfold-ok)}
.unfold-dot[data-tone=warn]{background:var(--unfold-warn)}
.unfold-dot[data-tone=bad]{background:var(--unfold-bad)}
.unfold-dot[data-tone=accent]{background:var(--unfold-accent)}
.unfold-dot[data-pulse=true]{animation:unfold-pulse 2.4s var(--unfold-ease) infinite}
@keyframes unfold-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in oklab,currentColor 0%,transparent)}50%{box-shadow:0 0 0 4px color-mix(in oklab,var(--unfold-ok) 18%,transparent)}}
.unfold-fold{border-top:1px solid var(--unfold-line)}
.unfold-fold-head{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:8px;width:100%;padding:9px 2px;cursor:pointer;color:var(--unfold-muted);font-size:12.5px;border-radius:6px}
.unfold-fold-head:hover{color:var(--unfold-fg)}
.unfold-fold-head:focus-visible{outline:2px solid var(--unfold-accent);outline-offset:1px}
.unfold-chev{width:12px;height:12px;flex:none;transition:transform .2s var(--unfold-ease)}
.unfold-fold-head[aria-expanded=true] .unfold-chev{transform:rotate(90deg)}
.unfold-fold-body{display:grid;grid-template-rows:0fr;transition:grid-template-rows .22s var(--unfold-ease)}
.unfold-fold-body[data-open=true]{grid-template-rows:1fr}
.unfold-fold-body>div{overflow:hidden;min-height:0}
.unfold-fold-inner{padding:0 2px 12px}
.unfold-facts{display:grid;grid-template-columns:auto 1fr;gap:5px 14px;margin:0}
.unfold-facts dt{color:var(--unfold-muted)}
.unfold-facts dd{margin:0}
.unfold-crash{padding:16px;color:var(--unfold-muted)}
@media (prefers-reduced-motion: reduce){.unfold-ui *,.unfold-ui *::before,.unfold-ui *::after{transition:none!important;animation:none!important}}
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
    try { return globalThis.localStorage?.getItem(`unfold-ui:${key}`) ?? null } catch { return null }
  },
  set(key, value) {
    try { globalThis.localStorage?.setItem(`unfold-ui:${key}`, value) } catch {}
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
  <svg className="unfold-chev" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5 8 6l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
)

/** A remembered disclosure: a plain summary line, the rest only on request. */
export function Fold({ id, summary, children, defaultOpen = false, className = '', ...rest }) {
  const [open, setOpen] = useRemembered(`fold:${id}`, defaultOpen)
  const bodyId = useId()
  return (
    <section className={`unfold-fold ${className}`} data-open={open} {...rest}>
      <button type="button" className="unfold-fold-head" aria-expanded={open} aria-controls={bodyId} onClick={() => setOpen((o) => !o)}>
        <Chevron />
        <span>{summary}</span>
      </button>
      <div className="unfold-fold-body" id={bodyId} data-open={open}>
        <div><div className="unfold-fold-inner">{open ? children : null}</div></div>
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
    console.error(`[${this.props.name ?? 'unfold'}] the tab failed to draw`, error)
  }
  render() {
    if (!this.state.error) return this.props.children
    return (
      <div className="unfold-ui"><div className="unfold-crash" role="alert">This tab hit a problem and stopped drawing. Reopen it to try again.</div></div>
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
  <span className="unfold-dot" data-tone={tone} data-pulse={pulse} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true} />
)
