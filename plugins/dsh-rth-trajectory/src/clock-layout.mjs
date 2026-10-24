// Duration overview layout. Pure; rendered by RTH.ClockOverview and tested in node.
//
// 1. A group is one (producer clock, lifetime): rth-host / process, or agent / boot. Each has its
//    own labelled axis and boundary; a restart is another group; no item crosses groups. Unknown
//    identity is listed as unplaced, never put in a guessed group.
// 2. The five semantic lanes live inside each group (only the lanes a group uses are shown).
// 3. Positions and widths come from the same validated producer timestamps (src/adapter.mjs
//    placementsOf): x = (t - origin) / extent, origin = the group's first retained valid timestamp
//    ("relative to first retained event"; retained windows are partial history). observed_at only
//    orders the groups (first seen first), never positions an item.
// 4. Independent axes: every group has its own origin, scale and ticks; there is no shared cursor,
//    "now" line or cross-group alignment. Equal x across groups means nothing.
// 5. Unknowns keep no invented length: 'start' and 'end' markers have fixed-size hatched tails in
//    the renderer; 'zero' is a measured-0 tick; nothing is extended to "now".
// 6. Uncompressed producer time inside a group; groups are bounded by the retained rows.

const LANES = ['Hear', 'Think', 'Speak', 'Act', 'Life']
const STEPS = [1, 2, 5]

/** Round tick positions for an extent in ms: about four to six labelled ticks from 0. */
export function ticksFor(extentMs) {
  const span = Math.max(1, extentMs)
  let step = 1
  for (let p = 0; p < 12; p++) {
    const base = 10 ** p
    const s = STEPS.map((k) => k * base).find((v) => span / v <= 6)
    if (s) { step = s; break }
  }
  const out = []
  for (let t = 0; t <= span + 1e-9; t += step) out.push({ ms: t, x: t / span, label: t === 0 ? '0' : t < 1000 ? `+${t} ms` : `+${(t / 1000).toFixed(t % 1000 ? 1 : 0)} s` })
  return out
}

/**
 * @param {object[]} turns the (filtered) trajectory turns; cells carry rthPlacements / rthOffAxis
 * @returns {{groups: object[], unplaced: Record<string, number>}}
 */
export function layoutClockOverview(turns) {
  const groups = new Map()
  const unplaced = {}
  for (const t of turns) for (const g of t.groups) for (const c of g.cells) {
    if (!c.rthPlacements?.length) {
      if (c.rthOffAxis) unplaced[c.rthOffAxis] = (unplaced[c.rthOffAxis] ?? 0) + 1
      continue
    }
    for (const p of c.rthPlacements) {
      const key = `${p.clock}|${p.life}`
      if (!groups.has(key)) groups.set(key, { clock: p.clock, life: p.life, items: [] })
      groups.get(key).items.push({ index: c.index, recordId: c.recordId, kind: c.rthKind, lane: p.lane, name: p.name, shape: p.shape, start: p.start, end: p.end, why: p.why, startRow: p.startRow, endRow: p.endRow })
    }
  }
  const out = []
  const counts = { host: 0, agent: 0 }
  for (const g of groups.values()) {
    const times = g.items.flatMap((i) => [i.start, i.end]).filter((v) => typeof v === 'number' && Number.isFinite(v))
    const origin = Math.min(...times)
    const extent = Math.max(1, Math.max(...times) - origin)
    const x = (t) => (t - origin) / extent
    const n = ++counts[g.clock]
    out.push({
      clock: g.clock,
      life: g.life,
      title: g.clock === 'host' ? `rth-host · process ${n}` : `agent · boot ${n}`,
      axisLabel: `${g.clock === 'host' ? 'rth-host clock' : 'agent clock'}, its own axis, relative to first retained event`,
      origin,
      extentMs: Math.max(...times) - origin,
      ticks: ticksFor(extent),
      lanes: LANES.map((name, lane) => ({ name, lane })).filter((l) => g.items.some((i) => i.lane === l.lane)),
      items: g.items.map((i) => ({
        ...i,
        x0: i.start != null ? x(i.start) : x(i.end),
        x1: i.shape === 'span' || i.shape === 'zero' ? x(i.end) : null,
        durationMs: i.shape === 'span' || i.shape === 'zero' ? i.end - i.start : null,
      })),
    })
  }
  return { groups: out, unplaced }
}
