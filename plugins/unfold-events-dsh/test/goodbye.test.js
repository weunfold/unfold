// The app's quit goodbye: the dust renderer's goodbye(ms) and the page hook.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { sceneFor } from '../src/avatar.js'

const pv = (pose) => ({ pose, label: pose, because: '', layers: { link: { state: 'live' } }, faults: [] })
const voice = { actual: 'off', desired: null, read: true, status: 'Voice asleep', mute: {} }
const until = async (what, ok, ms = 2000) => {
  const end = Date.now() + ms
  while (!ok()) {
    if (Date.now() > end) throw new Error(`${what} within ${ms} ms`)
    await new Promise((r) => setTimeout(r, 5))
  }
}

function stubCanvas() {
  const drawn = { grains: 0 } // grains painted in the last frame (2D path: one fillRect per visible grain, after the clear)
  const ctx2d = new Proxy({}, {
    get: (t, k) => (k in t ? t[k] : k === 'fillRect' ? () => { drawn.grains += 1 } : () => {}),
    set: (t, k, v) => { if (k === 'fillStyle' && v === '#0a0812') drawn.grains = -1; t[k] = v; return true },
  })
  const canvas = { width: 0, height: 0, dataset: {}, style: {}, getBoundingClientRect: () => ({ width: 200, height: 300 }), getContext: (k) => (k === '2d' ? ctx2d : null) }
  return { canvas, drawn }
}

test('goodbye: the dust disperses and fades out over ms, then nothing draws again', async () => {
  globalThis.requestAnimationFrame = (f) => setTimeout(() => f(performance.now()), 16)
  globalThis.cancelAnimationFrame = (id) => clearTimeout(id)
  const { dustRenderer } = await import('../src/client/dust-renderer.js')
  const { canvas, drawn } = stubCanvas()
  const r = dustRenderer(canvas, { grains: 2000 })
  try {
    r.draw(sceneFor(pv('idle'), voice)) // a resting scene: one still frame, no loop
    const lit = drawn.grains
    assert.ok(lit > 100, `the resting dust is lit (${lit} grains)`)
    assert.equal(r.stats().running, false)
    const f0 = r.stats().frames
    assert.equal(r.goodbye(300), true, 'on screen: the goodbye is seen')
    assert.equal(canvas.dataset.goodbye, 'dissolving')
    assert.equal(r.goodbye(300), true, 'a second call changes nothing')
    assert.ok(r.stats().running, 'the goodbye animates even from a resting scene')
    // Half way is a goodbye frame drawn after 150 ms, not whatever was drawn when a 150 ms sleep
    // ended: a busy loop can leave that at a frame from the start. Bounded, on ref'd timers.
    const half = performance.now() + 150
    let atHalf = null
    await until('a goodbye frame half way through', () => {
      if (atHalf === null && performance.now() >= half) atHalf = r.stats().frames
      return atHalf !== null && (r.stats().frames > atHalf || canvas.dataset.goodbye === 'done')
    })
    const mid = drawn.grains
    await until('the end of the goodbye', () => canvas.dataset.goodbye === 'done')
    assert.equal(r.stats().running, false, 'the loop ends with the goodbye')
    const frames = r.stats().frames - f0
    assert.ok(frames >= 8, `animated: ${frames} frames in 300 ms`)
    assert.ok(mid < lit, `fading: ${lit} -> ${mid} grains lit half way`)
    assert.ok(drawn.grains <= lit * 0.02, `gone at the end: ${drawn.grains} of ${lit} grains still lit`)
    r.draw(sceneFor(pv('pending'), voice)) // a scene change after the goodbye starts nothing
    r.setVisible(true)
    await new Promise((res) => setTimeout(res, 100))
    assert.equal(r.stats().running, false)
    assert.equal(r.stats().frames - f0, frames, 'no frame after the goodbye')
  } finally {
    r.dispose()
  }
  // Marked hidden (the flag can be stale in the native window): it still says goodbye; with no
  // scene yet there is nothing to dissolve. Reduced motion: a CSS fade.
  const h = stubCanvas()
  const hidden = dustRenderer(h.canvas, { grains: 500 })
  assert.equal(hidden.goodbye(300), false, 'no scene drawn yet: nothing to dissolve')
  hidden.draw(sceneFor(pv('idle'), voice))
  hidden.setVisible(false)
  assert.equal(hidden.goodbye(300), true)
  await until('the end of the hidden goodbye', () => h.canvas.dataset.goodbye === 'done')
  hidden.dispose()
  const m = stubCanvas()
  const still = dustRenderer(m.canvas, { grains: 500, reducedMotion: true })
  still.draw(sceneFor(pv('idle'), voice))
  const sf = still.stats().frames
  assert.equal(still.goodbye(300), true)
  assert.deepEqual([m.canvas.style.opacity, m.canvas.style.transition], ['0', 'opacity 300ms ease-in'])
  await new Promise((res) => setTimeout(res, 50))
  assert.equal(still.stats().frames, sf, 'reduced motion: no animation frames, only the fade')
  still.dispose()
  const dbg = globalThis.__unfoldAvatar
  assert.deepEqual([dbg.live, dbg.rafs, dbg.timeouts, dbg.observers], [0, 0, 0, 0], 'dispose leaves nothing behind')
  delete globalThis.requestAnimationFrame
  delete globalThis.cancelAnimationFrame
})

test('the page hook: every mounted presence registers, the count on screen is returned, unmount removes it', async () => {
  const src = (await import('node:fs')).readFileSync(new URL('../src/client/index.jsx', import.meta.url), 'utf8')
  assert.match(src, /globalThis\.__unfoldGoodbye \?\?= \(ms\) => \[\.\.\.globalThis\.__unfoldGoodbyes\]\.filter\(\(f\) => f\(ms\)\)\.length/)
  assert.match(src, /byes\.add\(bye\)\n {4}globalThis\.__unfoldGoodbye[^\n]*\n {4}return \(\) => byes\.delete\(bye\)/)
})
