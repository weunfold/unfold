// The client's styles, on DSH's own design tokens so they follow its light and dark themes.
// Every class starts with `ub-`.
export const STYLES = `
.ub-row { display: inline-flex; align-items: center; gap: 2px; padding: 2px 4px; border: 0; border-radius: var(--dsw-radius-sm, 4px); background: none; color: var(--dsw-alias-label-tertiary, inherit); cursor: pointer; }
.ub-row:hover, .ub-footer-button:hover { background: var(--dsw-alias-interactive-bg-hover, rgba(127, 127, 127, 0.12)); }
.ub-row-on { color: var(--dsw-alias-state-business-primary, inherit); }
.ub-watched { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; color: var(--dsw-alias-state-business-primary, inherit); }
.ub-watched .ub-eye { width: 12px; height: 12px; }
.ub-row:focus-visible, .ub-footer-button:focus-visible, .ub-button:focus-visible { outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, Highlight); }
.ub-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--dsw-alias-label-caption, gray); opacity: 0.5; }
.ub-dot-watched { opacity: 1; }
.ub-dot-running { background: var(--dsw-alias-state-business-primary, royalblue); }
.ub-dot-error { background: var(--dsw-alias-state-error-primary, crimson); }
.ub-footer { position: relative; }
.ub-footer-button { display: inline-flex; align-items: center; gap: 6px; padding: 6px 8px; border: 0; border-radius: var(--dsw-radius-md, 6px); background: none; color: var(--dsw-alias-label-secondary, inherit); font: inherit; cursor: pointer; }
.ub-count { min-width: 16px; padding: 0 4px; border-radius: 8px; background: var(--dsw-alias-state-business-primary, royalblue); color: var(--dsw-alias-bg-base, white); font-size: 11px; text-align: center; }
.ub-board { position: fixed; left: 8px; bottom: 56px; z-index: 50; width: min(380px, calc(100vw - 16px)); max-height: 70vh; overflow: auto; padding: 12px; border: 1px solid var(--dsw-alias-border-l2, rgba(127, 127, 127, 0.3)); border-radius: var(--dsw-radius-lg, 8px); background: var(--dsw-alias-bg-layer-1, Canvas); color: var(--dsw-alias-label-primary, CanvasText); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18); }
.ub-board-head { display: flex; align-items: center; justify-content: space-between; }
.ub-board h2, .ub-settings h2 { margin: 0 0 8px; font-size: 15px; }
.ub-board h3, .ub-settings h3 { margin: 12px 0 4px; font-size: 13px; color: var(--dsw-alias-label-secondary, inherit); }
.ub-board ul, .ub-settings ul { margin: 0; padding: 0; list-style: none; }
.ub-board li, .ub-settings li { padding: 4px 0; }
.ub-agent-line { display: flex; align-items: center; gap: 6px; }
.ub-agent-name { font-weight: 500; }
.ub-badge { padding: 0 6px; border: 1px solid var(--dsw-alias-border-l3, rgba(127, 127, 127, 0.4)); border-radius: 8px; font-size: 11px; color: var(--dsw-alias-label-secondary, inherit); }
.ub-hint, .ub-link { margin: 2px 0; font-size: 12px; color: var(--dsw-alias-label-tertiary, inherit); }
.ub-link-on { color: var(--dsw-alias-state-business-primary, inherit); }
.ub-label { margin: 0 0 2px; font-size: 12px; font-weight: 600; color: var(--dsw-alias-label-secondary, inherit); }
.ub-marked { margin: 0 0 4px; font-size: 12px; font-weight: 600; color: var(--dsw-alias-state-error-primary, crimson); }
.ub-note { margin: 0 0 4px; white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; }
.ub-error { margin: 4px 0 0; font-size: 12px; color: var(--dsw-alias-state-error-primary, crimson); }
.ub-dock { display: flex; flex-direction: column; gap: 8px; margin: 0 0 8px; }
.ub-advice, .ub-draft, .ub-suggest { padding: 8px 10px; border: 1px solid var(--dsw-alias-border-l2, rgba(127, 127, 127, 0.3)); border-radius: var(--dsw-radius-md, 6px); background: var(--dsw-alias-bg-layer-1, transparent); }
/* Text from agents keeps its own direction: a stray right-to-left mark can't reorder the line around it. */
.ub-to { margin: 0 0 6px; font-weight: 600; unicode-bidi: plaintext; }
.ub-draft-text { white-space: pre-wrap; overflow-wrap: anywhere; unicode-bidi: plaintext; font-family: var(--ds-font-family-code, monospace); font-size: 13px; }
.ub-actions { display: flex; gap: 8px; margin-top: 8px; }
.ub-button { padding: 4px 10px; border: 1px solid var(--dsw-alias-border-l3, rgba(127, 127, 127, 0.4)); border-radius: var(--dsw-radius-sm, 4px); background: none; color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
.ub-primary { border-color: var(--dsw-alias-state-business-primary, royalblue); background: var(--dsw-alias-state-business-primary, royalblue); color: var(--dsw-alias-bg-base, white); }
.ub-quiet { border-color: transparent; }
.ub-button:disabled { opacity: 0.5; cursor: not-allowed; }
.ub-sub, .ub-change, .ub-notice { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
`

export function installStyles() {
  const style = document.createElement('style')
  style.dataset.unfoldBridge = ''
  style.textContent = STYLES
  document.head.append(style)
  return () => style.remove()
}
