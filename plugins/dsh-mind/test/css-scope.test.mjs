// Cross-plugin stylesheet coexistence. Every RTH tab
// injects its stylesheet into the same DSH document, in whatever order the tabs open. A class used
// by more than one stylesheet must only be styled under that stylesheet's own root (its prefix), so
// no tab can restyle another's elements. The shared base sheet (rth-ui BASE_CSS) is exempt: it is
// the one intended global layer. This is the static guard; the rendered check (both open orders,
// computed position and boxes) needs a browser.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const read = (p) => readFileSync(new URL(`../../${p}`, import.meta.url), 'utf8')
const SHEETS = {
  mind: { file: 'dsh-mind/client/index.jsx', name: 'MIND_CSS', prefix: 'rth-mind' },
  presence: { file: 'dsh-events/client/index.jsx', name: 'PRESENCE_CSS', prefix: 'rth-pres' },
  events: { file: 'dsh-events/client/index.jsx', name: 'EVENTS_CSS', prefix: 'rth-ev' },
  settings: { file: 'dsh-settings/client/index.jsx', name: 'SETTINGS_CSS', prefix: 'rth-set' },
  base: { file: 'rth-ui/ui.jsx', name: 'BASE_CSS', prefix: null },
}

/** [{selector, classes, first}] for each comma-separated selector of a stylesheet (no nesting). */
export function selectorsOf(css) {
  const out = []
  const flat = css.replace(/\$\{[^}]*\}/g, '').replace(/@keyframes[^{]*\{(?:[^{}]*\{[^}]*\})*[^}]*\}/g, '')
  for (const rule of flat.split('}')) {
    const head = rule.split('{')
    if (head.length < 2) continue
    for (const sel of head.at(-2).split(',')) {
      const s = sel.trim()
      if (!s || s.startsWith('@')) continue
      const first = s.split(/[\s>+~]+/)[0]
      out.push({ selector: s, classes: [...s.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]), first: [...first.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]) })
    }
  }
  return out
}

function sheets() {
  return Object.fromEntries(Object.entries(SHEETS).map(([k, v]) => {
    const m = new RegExp(`const ${v.name} = \`([\\s\\S]*?)\n\``).exec(read(v.file))
    assert.ok(m, `${v.name} found in ${v.file}`)
    return [k, { ...v, rules: selectorsOf(m[1]) }]
  }))
}

test('no class is styled by two plugin stylesheets unless each styles it under its own root', () => {
  const all = sheets()
  const users = {}
  for (const [k, s] of Object.entries(all)) for (const r of s.rules) for (const c of r.classes) (users[c] ??= new Set()).add(k)
  const problems = []
  for (const [c, ks] of Object.entries(users)) {
    if (ks.size < 2) continue
    for (const k of ks) {
      const s = all[k]
      if (!s.prefix) continue
      for (const r of s.rules.filter((x) => x.classes.includes(c))) {
        const scoped = r.first.some((f) => f === s.prefix || f.startsWith(`${s.prefix}-`))
        if (!scoped) problems.push(`${k}: "${r.selector}" styles .${c} (also used by ${[...ks].filter((x) => x !== k).join(', ')}) without its root .${s.prefix}`)
      }
    }
  }
  assert.deepEqual(problems, [])
})

test("Presence's decorative caps and Mind's captions are different, scoped classes", () => {
  const all = sheets()
  const caps = all.presence.rules.filter((r) => r.classes.includes('rth-pres-cap'))
  assert.ok(caps.length >= 3 && caps.every((r) => r.first.includes('rth-pres')), 'the avatar caps are .rth-pres .rth-pres-cap')
  assert.ok(!all.presence.rules.some((r) => r.classes.includes('rth-cap')), 'Presence does not style .rth-cap')
  assert.ok(all.mind.rules.filter((r) => r.classes.includes('rth-cap')).every((r) => r.first.includes('rth-mind')), "Mind's .rth-cap rules are under .rth-mind")
  assert.match(read('dsh-events/client/index.jsx'), /className="rth-pres-cap top"/)
  assert.match(read('dsh-events/client/index.jsx'), /className="rth-pres-cap bot"/)
})

test('guard: a global .rth-cap in both Mind and Presence is caught', () => {
  const mind = selectorsOf('.rth-cap{margin-top:3px}')
  const pres = selectorsOf('.rth-cap{position:absolute;left:22%}')
  const collide = mind.some((r) => r.classes.includes('rth-cap') && !r.first.some((f) => f.startsWith('rth-mind'))) && pres.some((r) => r.classes.includes('rth-cap') && !r.first.some((f) => f.startsWith('rth-pres')))
  assert.ok(collide)
})
