// The cosmic-dust creature, behind the presence renderer interface (client/presence-renderer.js).
// Its grain count and colour are fixed per instance; the tab makes a new one when the cosmic
// points change them. The face forms only when the scene has something to show (a speech
// candidate, device-rendered speech, a fault) and dissolves back into dust.
//
//   dustRenderer(canvas, { reducedMotion, grains }) -> { draw(scene), setReducedMotion(on),
//     setVisible(on), goodbye(ms), dispose(), stats() }
//
// `draw(scene)` takes `sceneFor(view, voice)` (src/avatar.mjs) and is called on a change only.
// Budget: the loop runs only while the scene is visible and motion is allowed, at
// FPS[scene.motion] (30 while something forms or changes, 12 for calm drift, 4 at rest), with
// setTimeout pacing below 30 fps so the page is not woken every display frame. Hidden, detached or
// reduced motion: no loop; reduced motion draws one still frame per scene change. dispose() stops
// every frame request, timer and observer it started (counted in globalThis.__rthAvatar).
import { FPS } from '../src/avatar.mjs'

const DEBUG = (globalThis.__rthAvatar ??= { live: 0, created: 0, frames: 0, rafs: 0, timeouts: 0, observers: 0 })

// ---- Shapes in a fixed 300 x 400 portrait face canvas (x in [-150, 150], y in [-200, 200]) ----
const HY = 40
const EY = -24, EX = 46, MY = 38, FACE = 0.9
function line(pts, width = 2, taper = false) {
  const L = [0]
  for (let i = 1; i < pts.length; i++) L.push(L[i - 1] + Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]))
  const total = L[L.length - 1] || 1
  return (u, rr) => {
    const d = u * total
    let lo = 0, hi = L.length - 1
    while (hi - lo > 1) { const m = (lo + hi) >> 1; if (L[m] < d) lo = m; else hi = m }
    const a = pts[lo], b = pts[hi], f = (d - L[lo]) / ((L[hi] - L[lo]) || 1)
    const dx = b[0] - a[0], dy = b[1] - a[1], len = Math.hypot(dx, dy) || 1
    const w = taper ? width * (0.25 + Math.sin(Math.PI * u) * 0.75) : width
    return [a[0] + dx * f - (dy / len) * rr * w, a[1] + dy * f + (dx / len) * rr * w]
  }
}
const ring = (cx, cy, rx, ry, width = 2) => (u, rr) => { const a = u * Math.PI * 2; return [cx + Math.cos(a) * (rx + rr * width), cy + Math.sin(a) * (ry + rr * width)] }
const fill = (cx, cy, rx, ry) => (u, rr) => { const a = u * Math.PI * 2, r = Math.sqrt(Math.abs(rr)); return [cx + Math.cos(a) * rx * r, cy + Math.sin(a) * ry * r] }
const both = (a, b) => (u, rr) => (u < 0.5 ? a(u * 2, rr) : b(u * 2 - 1, rr))
const arc = (cx, cy, w, bend, n = 14) => Array.from({ length: n + 1 }, (_, i) => { const x = -w + (2 * w * i) / n, q = x / w; return [cx + x, cy + bend * (1 - q * q)] })
const place = (fn, s, dx, dy) => (u, rr) => { const p = fn(u, rr); return [p[0] * s + dx, p[1] * s + dy] }
const keyed = (fn, key) => ((fn.key = key), fn)
function eyes(kind) {
  const L = -EX, R = EX
  const raw = {
    open: () => both(fill(L, EY, 11, 15), fill(R, EY, 11, 15)),
    happy: () => both(line(arc(L, EY + 5, 22, -12), 5.5, true), line(arc(R, EY + 5, 22, -12), 5.5, true)),
    wince: () => both(line([[L - 15, EY - 12], [L + 12, EY], [L - 15, EY + 12]], 3), line([[R + 15, EY - 12], [R - 12, EY], [R + 15, EY + 12]], 3)),
  }[kind]()
  return keyed(place(raw, FACE, 0, 24 - EY * FACE), `e:${kind}`)
}
function mouth(kind, amp = 0) {
  const raw = {
    smile: () => line(arc(0, MY, 42, 16), 5.5, true),
    speak: () => ring(0, MY + 5, 20 + amp * 5, 4 + amp * 18, 2.6),
    zigzag: () => line([[-30, MY + 8], [-15, MY], [0, MY + 10], [15, MY], [30, MY + 8]], 2.4),
  }[kind]()
  return keyed(place(raw, FACE, 0, 92 - MY * FACE), `m:${kind}:${kind === 'speak' ? amp.toFixed(2) : ''}`)
}
// Pending: dust circling the head, nothing claimed about how (pending, never thinking).
const orbitRing = (t) => keyed(ring(0, HY, 118 + Math.sin(t * 2) * 4, 140 + Math.sin(t * 2) * 3, 4), `s:orbit:${t.toFixed(2)}`)

/** The scene as features to form: which groups gather into shapes, and how the free dust moves. */
function expressionOf(scene, t, amp) {
  const X = { eyes: null, mouth: null, sym: null, cloud: 'drift', dim: scene.dim, tint: scene.tint, jitter: 0 }
  switch (scene.form) {
    case 'scatter': X.cloud = 'scatter'; break
    case 'gather': X.cloud = 'gather'; break
    case 'disperse': X.cloud = 'disperse'; break
    case 'slow': X.cloud = 'slow'; break
    case 'settle': X.cloud = 'settle'; break
    case 'orbit': X.cloud = 'orbit'; X.sym = orbitRing(t); break
    case 'fault': X.jitter = 2; break
    default: X.cloud = 'drift'
  }
  if (scene.face === 'listen') { X.eyes = eyes('open'); X.mouth = mouth('smile') }
  else if (scene.face === 'speak') { X.eyes = eyes('happy'); X.mouth = mouth('speak', amp) }
  else if (scene.face === 'wince') { X.eyes = eyes('wince'); X.mouth = mouth('zigzag') }
  return X
}

// ---- Dust: one typed-array simulation ----
const CYAN = [95, 243, 255], GREY = [150, 140, 175], WARM = [255, 120, 80]
const GROUP_NAMES = ['eyes', 'mouth', 'sym', 'line', 'orb', 'cloud']
const SHARES = [0.10, 0.08, 0.12, 0.17, 0.10, 0.43]
const NU = 512, NR = 9, STAT = 8
const clamp = (v, a, b) => (v < a ? a : v > b ? b : v)
const sstep = (a, b, v) => { const t = clamp((v - a) / (b - a), 0, 1); return t * t * (3 - 2 * t) }
function hash2(a, b) { let n = (Math.imul(a, 374761393) + Math.imul(b, 668265263)) | 0; n = Math.imul(n ^ (n >>> 13), 1274126177); return ((n ^ (n >>> 16)) >>> 0) / 4294967296 }
function vnoise(x, y) {
  const xi = Math.floor(x), yi = Math.floor(y), u = x - xi, v = y - yi, su = u * u * (3 - 2 * u), sv = v * v * (3 - 2 * v)
  const a = hash2(xi, yi), b = hash2(xi + 1, yi), c = hash2(xi, yi + 1), d = hash2(xi + 1, yi + 1)
  return (a + (b - a) * su) * (1 - sv) + (c + (d - c) * su) * sv
}
const fbm = (x, y) => (vnoise(x, y) * 0.5 + vnoise(x * 2 + 5.3, y * 2 + 1.7) * 0.25 + vnoise(x * 4 + 11.1, y * 4 + 7.9) * 0.125) / 0.875
// Slow divergence-free curl flow (a few plane waves), sampled on a coarse grid once per frame.
const WAVES = [[180, 10, 0.21, 0.9], [120, 7, 1.9, -0.6], [260, 8, 3.7, 0.4], [90, 4, 5.1, 1.1]].map(([lam, disp, dir, w]) => {
  const k = 6.283 / lam
  return { kx: Math.cos(dir) * k, ky: Math.sin(dir) * k, A: disp / k, w: w * 0.25, p: dir * 3 }
})
const GX = 33, GY = 43
const MODES = { drift: 0, scatter: 1, settle: 2, gather: 3, slow: 4, orbit: 5, disperse: 6 }

function makeDust(cap) {
  const F = () => new Float32Array(cap)
  const px = F(), py = F(), vx = F(), vy = F(), hx = F(), hy = F(), ha = F(), hr = F(), ph = F(), fu = F(), fr = F(), dz = F()
  const iu = new Uint16Array(cap), jr = new Uint8Array(cap), grp = new Uint8Array(cap), alt = new Uint8Array(cap)
  const pos = new Float32Array(cap * 2), stat = new Float32Array(cap * STAT)
  const curlX = new Float32Array(GX * GY), curlY = new Float32Array(GX * GY)
  const cum = []
  SHARES.reduce((a, v, k) => (cum[k] = a + v), 0)
  for (let i = 0; i < cap; i++) {
    // Low-discrepancy group assignment: every prefix of the grains has the same group mix.
    const q = (i * 0.6180339887498949 + 0.31) % 1
    let g = 0
    while (g < 5 && q >= cum[g]) g++
    grp[i] = g
    let a = 0, r = 0, x = 0, y = 0
    for (let t = 0; t < 8; t++) {
      a = Math.random() * 6.283; r = Math.pow(Math.random(), 0.6); x = Math.cos(a) * r * 145; y = Math.sin(a) * r * 195
      if (Math.random() < 0.18 + 0.82 * sstep(0.36, 0.68, fbm(x / 36 + 3.7, y / 36 + 9.2))) break
    }
    ha[i] = a; hr[i] = r; hx[i] = x; hy[i] = y
    const u = Math.random(), rr = (Math.random() * 2 - 1) * (Math.random() < 0.18 ? 1.5 : 1)
    const tu = u * (NU - 1); iu[i] = Math.min(NU - 2, Math.floor(tu)); fu[i] = tu - iu[i]
    const tr = (rr + 1) * 0.5 * (NR - 1); jr[i] = clamp(Math.floor(tr), 0, NR - 2); fr[i] = tr - jr[i]
    px[i] = pos[2 * i] = x; py[i] = pos[2 * i + 1] = y
    ph[i] = Math.random() * 6.283; alt[i] = Math.random() < 0.04 ? 1 : 0
    const lr = Math.random(), layer = lr < 0.62 ? 0 : lr < 0.94 ? 1 : 2
    dz[i] = [0.7, 1, 1.6][layer]
    const k = i * STAT
    stat[k] = g; stat[k + 1] = Math.random(); stat[k + 2] = ph[i]
    stat[k + 3] = 0.5 + Math.pow(Math.random(), 2.2)
    stat[k + 4] = 0.45 + Math.random() * 0.55
    stat[k + 5] = layer
    stat[k + 6] = 1 - 0.7 * clamp((r - 0.8) / 0.2, 0, 1)
    stat[k + 7] = -1
  }
  const luts = GROUP_NAMES.slice(0, 5).map(() => new Float32Array(NU * NR * 2)), seams = luts.map(() => new Uint8Array(NU))
  const keys = new Array(5).fill(null), act = new Array(6).fill(null), actS = new Array(6).fill(null)
  function buildLUT(fn, L, S) {
    for (let i = 0; i < NU; i++) {
      const u = i / (NU - 1)
      for (let j = 0; j < NR; j++) { const p = fn(u, -1 + (2 * j) / (NR - 1)), o = (i * NR + j) * 2; L[o] = p[0]; L[o + 1] = p[1] }
    }
    const mid = (NR - 1) >> 1
    for (let i = 0; i < NU - 1; i++) {
      const o = (i * NR + mid) * 2, n2 = o + NR * 2, dx = L[n2] - L[o], dy = L[n2 + 1] - L[o + 1]
      S[i] = dx * dx + dy * dy > 36 ? 1 : 0
    }
  }
  function buildCurl(sec) {
    for (let j = 0; j < GY; j++) for (let i = 0; i < GX; i++) {
      const x = -160 + i * 10, y = -210 + j * 10
      let dx = 0, dy = 0
      for (const v of WAVES) { const c = v.A * Math.cos(v.kx * x + v.ky * y + v.w * sec + v.p); dx += c * v.ky; dy -= c * v.kx }
      curlX[j * GX + i] = dx; curlY[j * GX + i] = dy
    }
  }
  let rs = 0x2545f491
  const RND = 2.3283064365386963e-10
  return {
    cap, pos, stat,
    /** Step the first n grains toward the expression's targets; `still` snaps them (reduced motion). */
    step(X, n, sec, dt, still) {
      for (let g = 0; g < 5; g++) {
        const fn = X[GROUP_NAMES[g]]
        if (!fn) { act[g] = null; continue }
        if (!fn.key || fn.key !== keys[g]) { buildLUT(fn, luts[g], seams[g]); keys[g] = fn.key || null }
        act[g] = luts[g]; actS[g] = seams[g]
      }
      const k = still ? 1 : 0.075, damp = still ? 0 : 0.8, f60 = dt * 60
      const breathe = still ? 0 : Math.sin(sec * 1.5) * 1.6
      const jit = X.jitter && !still ? X.jitter * 4 : 0
      const mode = MODES[X.cloud] || 0
      const spin = still ? 0 : sec * (mode === 4 ? 0.012 : mode === 5 ? 0.16 : 0.035)
      const flow = !still && (mode === 0 || mode === 4 || mode === 5), flowAmt = mode === 4 ? 0.4 : 1
      if (flow) buildCurl(sec)
      for (let i = 0; i < n; i++) {
        const L = act[grp[i]]
        let tx, ty
        if (L) {
          const S = actS[grp[i]], a = iu[i], q = fr[i]
          let f = fu[i]
          if (S[a]) f = f < 0.5 ? 0 : 1
          const b0 = (a * NR + jr[i]) * 2, b1 = b0 + NR * 2
          const x0 = L[b0] + (L[b0 + 2] - L[b0]) * q, y0 = L[b0 + 1] + (L[b0 + 3] - L[b0 + 1]) * q
          const x1 = L[b1] + (L[b1 + 2] - L[b1]) * q, y1 = L[b1 + 1] + (L[b1 + 3] - L[b1 + 1]) * q
          tx = x0 + (x1 - x0) * f; ty = y0 + (y1 - y0) * f + breathe
          if (alt[i] && !still) { tx += Math.sin(sec * 2 + ph[i]) * 6; ty += Math.cos(sec * 1.7 + ph[i]) * 6 }
        } else if (mode === 1) { const j = still ? 0 : Math.sin(sec * 4 + ph[i] * 9) * 6; tx = hx[i] * 1.03 + j; ty = hy[i] * 1.02 - j }
        else if (mode === 2) { tx = hx[i] * 1.02; ty = 182 + Math.abs(hy[i]) * 0.06 - hr[i] * 14 }
        else if (mode === 3) { const a2 = (ha[i] + spin * (1.3 - hr[i])) * 2, r = hr[i] * 0.45; tx = Math.cos(a2) * r * 150; ty = HY * 0.8 + Math.sin(a2) * r * 170 }
        else if (mode === 6) { tx = hx[i] * 1.45 + Math.cos(ha[i]) * 30; ty = hy[i] * 1.3 + Math.sin(ha[i]) * 30 }
        else {
          const a2 = ha[i] + spin * (1.3 - hr[i]) * dz[i]
          tx = Math.cos(a2) * hr[i] * 145; ty = Math.sin(a2) * hr[i] * 195 + (mode === 4 ? 10 : 0)
          if (flow) {
            const gx = clamp((tx + 160) / 10, 0, GX - 1.001), gy = clamp((ty + 210) / 10, 0, GY - 1.001), ci = gx | 0, cj = gy | 0, u = gx - ci, v = gy - cj
            const o = cj * GX + ci, s = dz[i] * flowAmt
            tx += ((curlX[o] * (1 - u) + curlX[o + 1] * u) * (1 - v) + (curlX[o + GX] * (1 - u) + curlX[o + GX + 1] * u) * v) * s
            ty += ((curlY[o] * (1 - u) + curlY[o + 1] * u) * (1 - v) + (curlY[o + GX] * (1 - u) + curlY[o + GX + 1] * u) * v) * s
          }
        }
        if (jit) { rs = (Math.imul(rs, 1664525) + 1013904223) | 0; tx += rs * RND * jit; rs = (Math.imul(rs, 1664525) + 1013904223) | 0; ty += rs * RND * jit }
        let x, y
        if (still) { x = tx; y = ty }
        else {
          rs = (Math.imul(rs, 1664525) + 1013904223) | 0; const nx = rs * RND * 0.3
          rs = (Math.imul(rs, 1664525) + 1013904223) | 0; const ny = rs * RND * 0.3
          const nvx = (vx[i] + (tx - px[i]) * k) * damp + nx, nvy = (vy[i] + (ty - py[i]) * k) * damp + ny
          vx[i] = nvx; vy[i] = nvy; x = px[i] + nvx * f60; y = py[i] + nvy * f60
        }
        x = x < -148 ? -148 : x > 148 ? 148 : x; y = y < -198 ? -198 : y > 198 ? 198 : y
        px[i] = x; py[i] = y; pos[2 * i] = x; pos[2 * i + 1] = y
      }
    },
    formed(g) { return !!act[g] },
  }
}
const GAINS = [0.62, 0.62, 0.6, 0.6, 0, 0.42]
const granOf = (n) => clamp(Math.pow(40000 / Math.max(1, n), 0.35), 0.8, 2.2)
const energyOf = (n) => clamp(Math.pow(60000 / Math.max(1, n), 0.3), 0.7, 1.8)

// ---- WebGL: gl.POINTS, soft gaussian grains, a three-level GPU bloom, a deep-space haze ----
const VS_PTS = `
  precision highp float;
  attribute vec2 aPos; attribute vec4 aInfo; attribute vec4 aA;
  uniform vec2 uScale; uniform float uFormed[6]; uniform float uGain[6];
  uniform float uTint, uDim, uStill, uSec, uPx, uGran, uEnergy; uniform vec3 uCol;
  varying vec3 vCol; varying float vA, vSize, vSig;
  void main() {
    int gi = int(aInfo.x + 0.5);
    float hue = aInfo.y, ph = aInfo.z, layer = aA.y;
    float formed = 0.0, gain = 0.0;
    for (int k = 0; k < 6; k++) if (k == gi) { formed = uFormed[k]; gain = uGain[k]; }
    vec3 col;
    if (uTint > 0.5 && uTint < 1.5) col = uCol * (0.85 + 0.3 * hue);
    else if (formed > 0.5 && uTint > 1.5) col = vec3(1.0, 0.471, 0.314);
    else col = formed > 0.5 ? uCol : uCol * mix(0.55, 1.15, hue);
    float tw = 1.0;
    if (uStill < 0.5) tw = 0.6 + 0.4 * sin(uSec * (1.2 + fract(ph * 7.31) * 4.5) + ph * 3.0);
    float a, s;
    if (formed > 0.5) { a = aA.x * gain; s = aInfo.w * uGran; }
    else {
      a = aA.x * aA.z * uGain[5] * (layer < 0.5 ? 0.5 : (layer < 1.5 ? 1.0 : 0.16));
      s = layer < 0.5 ? aInfo.w * 0.65 * uGran : (layer < 1.5 ? aInfo.w * uGran : 2.2 + aInfo.w * 1.4 * uGran);
    }
    a = min(1.0, a * uDim * tw) * uEnergy;
    s *= uPx;
    float sig = max(0.42, s * 0.5);
    float e = s * 0.5 / sig; a *= min(1.0, e * e);
    vSig = sig; vSize = max(2.0, ceil(sig * 6.0)); gl_PointSize = vSize;
    vCol = col; vA = a;
    gl_Position = a > 0.0 ? vec4(aPos * uScale, 0.0, 1.0) : vec4(2.0, 2.0, 2.0, 1.0);
  }`
const FS_PTS = `
  precision mediump float;
  varying vec3 vCol; varying float vA, vSize, vSig;
  void main() {
    vec2 d = (gl_PointCoord - 0.5) * vSize;
    float w = exp(-dot(d, d) / (2.0 * vSig * vSig));
    gl_FragColor = vec4(vCol * (vA * w), 1.0);
  }`
const VS_FULL = `
  attribute vec2 aXY; varying vec2 vUv;
  void main() { vUv = aXY * 0.5 + 0.5; gl_Position = vec4(aXY, 0.0, 1.0); }`
const FS_DOWN = `
  precision mediump float;
  uniform sampler2D uTex; uniform vec2 uTexel; varying vec2 vUv;
  void main() {
    gl_FragColor = 0.25 * (texture2D(uTex, vUv + uTexel * vec2(-1.0, -1.0)) + texture2D(uTex, vUv + uTexel * vec2(1.0, -1.0))
                         + texture2D(uTex, vUv + uTexel * vec2(-1.0, 1.0)) + texture2D(uTex, vUv + uTexel * vec2(1.0, 1.0)));
  }`
const FS_BLUR = `
  precision mediump float;
  uniform sampler2D uTex; uniform vec2 uDir; varying vec2 vUv;
  void main() {
    vec2 o1 = uDir * 1.3846154, o2 = uDir * 3.2307692;
    gl_FragColor = texture2D(uTex, vUv) * 0.2270270
      + (texture2D(uTex, vUv + o1) + texture2D(uTex, vUv - o1)) * 0.3162162
      + (texture2D(uTex, vUv + o2) + texture2D(uTex, vUv - o2)) * 0.0702703;
  }`
const FS_COMP = `
  #ifdef GL_FRAGMENT_PRECISION_HIGH
  precision highp float;
  #else
  precision mediump float;
  #endif
  uniform sampler2D uScene, uB1, uB2, uB3;
  uniform vec2 uRes; uniform float uSec, uGain, uExpo, uNeb; uniform vec3 uNebA, uNebB;
  varying vec2 vUv;
  float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
  float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
  }
  float fbm(vec2 p) { float v = 0.0, a = 0.5; for (int i = 0; i < 5; i++) { v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; } return v; }
  void main() {
    vec3 scene = texture2D(uScene, vUv).rgb;
    vec3 bloom = texture2D(uB1, vUv).rgb * 0.8 + texture2D(uB2, vUv).rgb * 0.7 + texture2D(uB3, vUv).rgb * 0.6;
    vec3 c = 1.0 - exp(-(scene + bloom) * uGain * uExpo);
    vec2 q = (vUv - 0.5) * 2.0; float d = length(q) / 1.41421;
    vec3 bg = mix(vec3(0.078, 0.059, 0.118), vec3(0.027, 0.024, 0.043), clamp(d / 0.75, 0.0, 1.0));
    vec2 np = vec2(vUv.x * uRes.x / uRes.y, vUv.y) * 2.6;
    float n1 = fbm(np + vec2(uSec * 0.010, -uSec * 0.007)), n2 = fbm(np * 1.4 + vec2(-uSec * 0.008, 4.0));
    bg += (uNebA * smoothstep(0.42, 0.9, n1) + uNebB * smoothstep(0.48, 0.95, n2)) * uNeb * (1.0 - 0.7 * clamp(d, 0.0, 1.0));
    vec3 o = 1.0 - (1.0 - bg) * (1.0 - c);
    o += (hash(gl_FragCoord.xy) - 0.5) / 255.0;
    gl_FragColor = vec4(o, 1.0);
  }`

function makeWebGL(canvas, dust, size, base = CYAN) {
  const attrs = { alpha: false, antialias: false, depth: false, stencil: false, premultipliedAlpha: false, preserveDrawingBuffer: false, powerPreference: 'low-power' }
  let gl = canvas.getContext('webgl2', attrs)
  const gl2 = !!gl
  if (!gl) gl = canvas.getContext('webgl', attrs)
  if (!gl) return null
  let progs, bufPos, bufStat, bufTri, fmt, targets = null, lost = false
  function compile(vsSrc, fsSrc) {
    const sh = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS) && !gl.isContextLost()) throw new Error(gl.getShaderInfoLog(s)); return s }
    const p = gl.createProgram()
    gl.attachShader(p, sh(gl.VERTEX_SHADER, vsSrc)); gl.attachShader(p, sh(gl.FRAGMENT_SHADER, fsSrc))
    ;['aPos', 'aInfo', 'aA'].forEach((n, i) => gl.bindAttribLocation(p, i, n))
    gl.bindAttribLocation(p, 0, 'aXY')
    gl.linkProgram(p)
    if (!gl.getProgramParameter(p, gl.LINK_STATUS) && !gl.isContextLost()) throw new Error(gl.getProgramInfoLog(p))
    const u = {}
    const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS) || 0
    for (let i = 0; i < n; i++) { const name = gl.getActiveUniform(p, i).name; u[name.replace('[0]', '')] = gl.getUniformLocation(p, name) }
    return { p, u }
  }
  function pickFormat() {
    if (gl2 && gl.getExtension('EXT_color_buffer_float')) return { internal: gl.RGBA16F, type: gl.HALF_FLOAT, hdr: true }
    return { internal: gl.RGBA, type: gl.UNSIGNED_BYTE, hdr: false }
  }
  function texture(w, h) {
    const t = gl.createTexture()
    gl.bindTexture(gl.TEXTURE_2D, t)
    gl.texImage2D(gl.TEXTURE_2D, 0, fmt.internal, w, h, 0, gl.RGBA, fmt.type, null)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    return t
  }
  function target(w, h) {
    const tex = texture(w, h), fb = gl.createFramebuffer()
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb)
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0)
    const ok = gl.checkFramebufferStatus(gl.FRAMEBUFFER) === gl.FRAMEBUFFER_COMPLETE
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    return { tex, fb, w, h, ok }
  }
  const allTargets = () => [targets.scene, ...targets.levels.flat()]
  function freeTargets() {
    if (!targets) return
    allTargets().forEach((t) => { gl.deleteTexture(t.tex); gl.deleteFramebuffer(t.fb) })
    targets = null
  }
  function ensureTargets() {
    const { W, H } = size
    if (targets && targets.w === W && targets.h === H) return true
    for (let attempt = 0; attempt < 2; attempt++) {
      freeTargets()
      const lv = (d) => [Math.max(1, Math.ceil(W / d)), Math.max(1, Math.ceil(H / d))]
      targets = { w: W, h: H, scene: target(W, H), levels: [2, 4, 8].map((d) => [target(...lv(d)), target(...lv(d))]) }
      if (allTargets().every((t) => t.ok)) return true
      if (!fmt.hdr) break
      fmt = { internal: gl.RGBA, type: gl.UNSIGNED_BYTE, hdr: false }
    }
    return false
  }
  function setup() {
    lost = false; targets = null
    fmt = pickFormat()
    progs = { pts: compile(VS_PTS, FS_PTS), down: compile(VS_FULL, FS_DOWN), blur: compile(VS_FULL, FS_BLUR), comp: compile(VS_FULL, FS_COMP) }
    const buf = (data, usage) => { const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b); gl.bufferData(gl.ARRAY_BUFFER, data, usage); return b }
    bufPos = buf(dust.pos, gl.DYNAMIC_DRAW); bufStat = buf(dust.stat, gl.STATIC_DRAW)
    bufTri = buf(new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
  }
  try { setup() } catch (err) { console.warn('rth avatar: WebGL setup failed', err); return null }
  const onLost = (e) => { e.preventDefault(); lost = true }
  const onRestored = () => { try { setup() } catch (err) { console.warn(err) } }
  canvas.addEventListener('webglcontextlost', onLost)
  canvas.addEventListener('webglcontextrestored', onRestored)
  function pass(prog, dst, src, name, v) {
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fb); gl.viewport(0, 0, dst.w, dst.h)
    gl.useProgram(prog.p)
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, src.tex); gl.uniform1i(prog.u.uTex, 0)
    gl.uniform2f(prog.u[name], v[0], v[1])
    gl.drawArrays(gl.TRIANGLES, 0, 3)
  }
  const formedArr = new Float32Array(6), gainArr = new Float32Array(6)
  return {
    name: gl2 ? 'WebGL2' : 'WebGL1',
    draw(X, n, sec, still) {
      if (lost || gl.isContextLost() || !ensureTargets()) return
      const { W, H, DPR } = size
      const scale = Math.min(W / 300, H / 400) * 0.98, sx = (scale * 2) / W, sy = (-scale * 2) / H
      const out = fmt.hdr ? 1 : 0.3
      const tint = X.tint === 'grey' ? 1 : X.tint === 'warm' ? 2 : 0
      gl.bindFramebuffer(gl.FRAMEBUFFER, targets.scene.fb); gl.viewport(0, 0, W, H)
      gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT)
      gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE)
      const P = progs.pts
      gl.useProgram(P.p)
      for (let g = 0; g < 6; g++) { formedArr[g] = dust.formed(g) ? 1 : 0; gainArr[g] = GAINS[g] * out }
      gl.uniform1fv(P.u.uFormed, formedArr); gl.uniform1fv(P.u.uGain, gainArr)
      gl.uniform2f(P.u.uScale, sx, sy)
      const col = tint === 1 ? GREY : base
      gl.uniform3f(P.u.uCol, col[0] / 255, col[1] / 255, col[2] / 255)
      gl.uniform1f(P.u.uTint, tint); gl.uniform1f(P.u.uDim, X.dim)
      gl.uniform1f(P.u.uStill, still ? 1 : 0); gl.uniform1f(P.u.uSec, sec % 3600)
      gl.uniform1f(P.u.uPx, DPR >= 1.5 ? 1.3 : 1); gl.uniform1f(P.u.uGran, granOf(n)); gl.uniform1f(P.u.uEnergy, energyOf(n))
      gl.bindBuffer(gl.ARRAY_BUFFER, bufPos); gl.bufferSubData(gl.ARRAY_BUFFER, 0, dust.pos.subarray(0, n * 2))
      gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
      gl.bindBuffer(gl.ARRAY_BUFFER, bufStat)
      gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 4, gl.FLOAT, false, STAT * 4, 0)
      gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2, 4, gl.FLOAT, false, STAT * 4, 16)
      if (n > 0) gl.drawArrays(gl.POINTS, 0, n)
      gl.disableVertexAttribArray(1); gl.disableVertexAttribArray(2)
      gl.disable(gl.BLEND)
      gl.bindBuffer(gl.ARRAY_BUFFER, bufTri); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
      let src = targets.scene
      for (const [a, b] of targets.levels) {
        pass(progs.down, a, src, 'uTexel', [1 / src.w, 1 / src.h])
        pass(progs.blur, b, a, 'uDir', [1 / a.w, 0])
        pass(progs.blur, a, b, 'uDir', [0, 1 / a.h])
        src = a
      }
      gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, W, H)
      const C = progs.comp
      gl.useProgram(C.p)
      ;[targets.scene, ...targets.levels.map((l) => l[0])].forEach((t, i) => { gl.activeTexture(gl.TEXTURE0 + i); gl.bindTexture(gl.TEXTURE_2D, t.tex) })
      gl.uniform1i(C.u.uScene, 0); gl.uniform1i(C.u.uB1, 1); gl.uniform1i(C.u.uB2, 2); gl.uniform1i(C.u.uB3, 3)
      gl.uniform2f(C.u.uRes, W, H); gl.uniform1f(C.u.uSec, still ? 0 : sec % 3600)
      gl.uniform1f(C.u.uGain, 1 / out); gl.uniform1f(C.u.uExpo, 1.25)
      const nA = tint === 1 ? GREY : tint === 2 ? WARM : base, nB = tint === 1 ? GREY : base.map((v) => v * 0.7)
      gl.uniform3f(C.u.uNebA, nA[0] / 255, nA[1] / 255, nA[2] / 255); gl.uniform3f(C.u.uNebB, nB[0] / 255, nB[1] / 255, nB[2] / 255)
      gl.uniform1f(C.u.uNeb, 0.075 * Math.min(1, X.dim))
      gl.drawArrays(gl.TRIANGLES, 0, 3)
      gl.activeTexture(gl.TEXTURE0)
    },
    dispose() {
      canvas.removeEventListener('webglcontextlost', onLost)
      canvas.removeEventListener('webglcontextrestored', onRestored)
      if (!gl.isContextLost()) {
        freeTargets()
        Object.values(progs).forEach((p) => gl.deleteProgram(p.p))
        ;[bufPos, bufStat, bufTri].forEach((b) => gl.deleteBuffer(b))
      }
    },
  }
}

// ---- Canvas 2D fallback: the same dust as small additive squares, capped at 4,000 grains ----
const CAP_2D = 4000
function makeCanvas2D(canvas, dust, size, base = CYAN) {
  const ctx = canvas.getContext('2d')
  if (!ctx) return null
  return {
    name: 'Canvas 2D',
    draw(X, n0, sec, still) {
      const { W, H } = size
      const n = Math.min(n0, CAP_2D)
      const scale = Math.min(W / 300, H / 400) * 0.98, cx = W / 2, cy = H / 2
      ctx.globalCompositeOperation = 'source-over'
      ctx.fillStyle = '#0a0812'
      ctx.fillRect(0, 0, W, H)
      ctx.globalCompositeOperation = 'lighter'
      const st = dust.stat, pos = dust.pos
      for (let i = 0; i < n; i++) {
        const k = i * STAT, g = st[k], formed = dust.formed(g), layer = st[k + 5]
        const col = X.tint === 'grey' ? GREY : formed && X.tint === 'warm' ? WARM : base
        const tw = still ? 1 : 0.75 + Math.sin(sec * 3 + st[k + 2] * 3) * 0.25
        const a = Math.min(1, st[k + 4] * X.dim * tw * (formed ? GAINS[g] : GAINS[5] * st[k + 6] * (layer === 0 ? 0.5 : layer === 1 ? 1 : 0.16)) * 2)
        if (a <= 0.004) continue
        ctx.fillStyle = `rgba(${col[0]},${col[1]},${col[2]},${a.toFixed(3)})`
        const sz = Math.max(1, st[k + 3] * 1.6 * (formed ? 1.2 : layer === 2 ? 2 : 1))
        ctx.fillRect(cx + pos[2 * i] * scale - sz / 2, cy + pos[2 * i + 1] * scale - sz / 2, sz, sz)
      }
      ctx.globalCompositeOperation = 'source-over'
    },
    dispose() {},
  }
}

/** The creature's colour by its CP colour tier (one more colour every 10 levels); the newest tier leads. */
export const TIER_RGB = { cyan: [95, 243, 255], magenta: [255, 95, 210], violet: [169, 139, 255], amber: [255, 197, 110], rose: [255, 143, 168], mint: [127, 240, 192], 'ice-blue': [184, 230, 255], coral: [255, 143, 106], lime: [200, 240, 110], 'white-gold': [255, 233, 184] }

/** The dust creature behind the presence renderer interface. */
export function dustRenderer(canvas, { reducedMotion = false, grains = 10_000, colour = 'cyan' } = {}) {
  const base = TIER_RGB[colour] ?? CYAN
  DEBUG.live += 1
  DEBUG.created += 1
  const dust = makeDust(grains)
  const size = { W: 1, H: 1, DPR: 1 }
  // Measure first (the GL targets are sized from it), then the state the loop reads, and only then
  // the observer: a resize that paints must never run before that state exists.
  const measure = () => {
    const r = canvas.getBoundingClientRect()
    size.DPR = Math.min(2, globalThis.devicePixelRatio || 1)
    const w = Math.max(1, Math.round(r.width * size.DPR)), h = Math.max(1, Math.round(r.height * size.DPR))
    if (w === size.W && h === size.H) return false
    size.W = w; size.H = h; canvas.width = w; canvas.height = h
    return true
  }
  measure()
  const gfx = makeWebGL(canvas, dust, size, base) ?? makeCanvas2D(canvas, dust, size, base)
  let scene = null
  let reduced = reducedMotion
  let visible = true
  let disposed = false
  let raf = 0
  let timer = 0
  let last = 0
  let shown = 0 // grains drawn now; eases toward the scene's density
  let frames = 0
  let bye = null // the app's quit: { t0, ms } once goodbye() runs; nothing else draws after it
  const resize = () => { if (measure()) paintStill() }
  const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(resize) : null
  if (ro) { ro.observe(canvas); DEBUG.observers += 1 }
  const moving = () => (FPS[scene?.motion] ?? FPS.calm) > 0 // a 0 fps scene (unknown) is one still frame
  const want = () => !disposed && visible && !reduced && scene && gfx && moving()
  const target = () => Math.round(grains * (scene?.density ?? 1))
  function frame(ts, still) {
    const sec = ts / 1000
    const dt = last ? Math.min(0.05, (ts - last) / 1000) : 0.016
    last = ts
    const goal = target()
    shown = still ? goal : Math.round(shown + (goal - shown) * Math.min(1, dt * 3))
    if (Math.abs(goal - shown) < 50) shown = goal
    const amp = scene.face === 'speak' && !still ? 0.35 + 0.65 * Math.abs(Math.sin(sec * 7.3) * Math.sin(sec * 3.1 + 1)) : 0.5
    const X = expressionOf(scene, still ? 0 : sec, amp)
    if (bye) { const k = clamp((ts - bye.t0) / bye.ms, 0, 1); X.eyes = X.mouth = X.sym = null; X.cloud = 'disperse'; X.dim *= (1 - k) * (1 - k) }
    dust.step(X, Math.max(shown, goal), still ? 0 : sec, dt, still)
    gfx.draw(X, shown, sec, still)
    frames += 1
    DEBUG.frames += 1
  }
  function paintStill() {
    if (!disposed && visible && scene && gfx) frame(performance.now(), true)
  }
  function cancel() {
    if (raf) { cancelAnimationFrame(raf); raf = 0; DEBUG.rafs -= 1 }
    if (timer) { clearTimeout(timer); timer = 0; DEBUG.timeouts -= 1 }
  }
  function tick(ts) {
    raf = 0
    DEBUG.rafs -= 1
    if (!want()) return
    const fps = FPS[scene.motion] ?? FPS.calm
    if (fps >= 30 && last && ts - last < 1000 / fps - 2) return schedule()
    frame(ts, false)
    schedule()
  }
  function schedule() {
    if (!want() || raf || timer) return
    const fps = FPS[scene.motion] ?? FPS.calm
    if (fps >= 30) { raf = requestAnimationFrame(tick); DEBUG.rafs += 1; return }
    timer = setTimeout(() => {
      timer = 0
      DEBUG.timeouts -= 1
      if (!want()) return
      raf = requestAnimationFrame(tick)
      DEBUG.rafs += 1
    }, Math.round(1000 / fps))
    DEBUG.timeouts += 1
  }
  return {
    name: gfx?.name ?? 'none',
    draw(next) {
      const changed = !scene || next.key !== scene.key
      scene = next
      canvas.dataset.scene = next.key
      if (bye) return
      if (reduced || !moving()) { cancel(); if (changed) paintStill(); return }
      if (changed) { cancel(); last = 0 }
      schedule()
    },
    setReducedMotion(on) {
      reduced = Boolean(on)
      if (bye) return
      cancel()
      if (reduced || !moving()) paintStill()
      else schedule()
    },
    /** The tab or window is hidden (or the canvas is off screen): stop rendering until it shows. */
    setVisible(on) {
      visible = Boolean(on)
      if (bye) return
      if (!visible) { cancel(); return }
      last = 0
      if (reduced || !moving()) paintStill()
      else schedule()
    },
    /** The app's quit: the face lets go and the dust disperses and fades out over `ms` (a fade under
     *  reduced motion). True when it started. Not gated on `visible`: a stale flag would silently
     *  skip the goodbye, and a page that is really hidden runs no animation frame anyway. */
    goodbye(ms = 500) {
      if (disposed || !gfx || !scene) return false
      if (bye) return true
      cancel()
      bye = { t0: performance.now(), ms }
      canvas.dataset.goodbye = 'dissolving'
      if (reduced) { canvas.style.transition = `opacity ${ms}ms ease-in`; canvas.style.opacity = '0'; return true }
      last = 0
      const step = (ts) => {
        raf = 0
        DEBUG.rafs -= 1
        if (disposed) return
        frame(ts, false)
        if (ts - bye.t0 < ms) { raf = requestAnimationFrame(step); DEBUG.rafs += 1 } else canvas.dataset.goodbye = 'done'
      }
      raf = requestAnimationFrame(step)
      DEBUG.rafs += 1
      return true
    },
    stats: () => ({ renderer: gfx?.name ?? 'none', grains, shown, frames, running: Boolean(raf || timer), fps: scene && want() ? FPS[scene.motion] : 0 }),
    dispose() {
      if (disposed) return
      disposed = true
      cancel()
      if (ro) { ro.disconnect(); DEBUG.observers -= 1 }
      gfx?.dispose()
      DEBUG.live -= 1
    },
  }
}
