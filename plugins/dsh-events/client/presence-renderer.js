// The presence renderer interface. The reducer decides WHAT is true; a renderer only draws it.
// Any look (this placeholder, the cosmic-dust creature in dust-renderer.js) implements the same
// shape, so swapping the look touches nothing else:
//
//   createRenderer(canvas, { reducedMotion }) -> {
//     draw(scene),           // called on a state change only; scene = src/avatar.mjs
//                            // sceneFor(view, voice): the presence view() and the actual voice
//                            // state, with `pose` and `label` carried over for simple looks
//     setReducedMotion(on),  // prefers-reduced-motion changed: no breathing, bobbing or drift
//     setVisible(on),        // optional: the tab or window was hidden or shown; hidden draws nothing
//     dispose(),             // the tab closed: stop every timer or frame request it started
//   }
//
// Contract for every renderer: never draw a sign of life over `unknown` (no blink, no breath);
// never show thinking, a mood or a confidence; carry state by shape, not colour alone (the
// tab's text label says it in words); no flashing above 3 per second; with reduced motion,
// pose changes are instant and nothing moves continuously.

/** The honest placeholder: one static shape per pose class and the pose label. No motion. */
export function placeholderRenderer(canvas) {
  const ctx = canvas.getContext('2d')
  let last = null
  const paint = () => {
    if (!ctx || !last) return
    const { width: w, height: h } = canvas
    const ink = getComputedStyle(canvas).color || '#888'
    ctx.clearRect(0, 0, w, h)
    ctx.strokeStyle = ink
    ctx.fillStyle = ink
    ctx.lineWidth = 2
    const [cx, cy, r] = [w / 2, h / 2 - 8, Math.min(w, h) / 4]
    const disc = () => {
      ctx.beginPath()
      ctx.arc(cx, cy, r, 0, Math.PI * 2)
    }
    switch (last.pose) {
      case 'unknown': // a faint dashed ring with "?": nothing alive is drawn over missing data
        ctx.globalAlpha = 0.45
        ctx.setLineDash([4, 4])
        disc()
        ctx.stroke()
        ctx.setLineDash([])
        ctx.font = `${Math.round(r)}px sans-serif`
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText('?', cx, cy)
        break
      case 'pending': // a ring: something is open, nothing is claimed about how
        disc()
        ctx.stroke()
        break
      case 'wince': // a disc with a bar cut out
        disc()
        ctx.fill()
        ctx.clearRect(cx - r, cy - 3, 2 * r, 6)
        break
      case 'sleepy':
      case 'asleep': // the lower half filled
        disc()
        ctx.stroke()
        ctx.beginPath()
        ctx.arc(cx, cy, r, 0, Math.PI)
        ctx.fill()
        break
      default: // idle, speaking: a plain disc
        disc()
        ctx.fill()
    }
    ctx.globalAlpha = 1
    ctx.font = '13px sans-serif'
    ctx.textAlign = 'center'
    ctx.textBaseline = 'alphabetic'
    ctx.fillText(last.label, cx, h - 8)
  }
  return {
    draw(view) {
      last = view
      paint()
    },
    setReducedMotion() {}, // nothing moves in the placeholder
    setVisible() {},
    dispose() {
      last = null
    },
  }
}
