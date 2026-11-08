// Cross-plugin stylesheet coexistence. Every Unfold tab injects its stylesheet into the same DSH document, in whatever order the tabs open. A class used
// by more than one stylesheet must only be styled under that stylesheet's own root (its prefix), so
// no tab can restyle another's elements. The shared base sheet (unfold-ui BASE_CSS) is exempt: it is
// the one intended global layer. The rendered check (both open orders, computed position and boxes)
// is a browser check; this is the static guard that runs in the gate. The sibling Unfold plugins'
// sheets are checked whenever they sit beside this package (apps/<name>/src/client).
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

const url = (p) => new URL(`../../${p}`, import.meta.url)
const read = (p) => readFileSync(url(p), 'utf8')
const EVENTS = 'unfold-events-dsh/src/client/index.jsx'
const SHEETS = {
  mind: { file: 'unfold-mind-dsh/src/client/index.jsx', name: 'MIND_CSS', prefix: 'unfold-mind' },
  presence: { file: EVENTS, name: 'PRESENCE_CSS', prefix: 'unfold-pres', sibling: true },
  events: { file: EVENTS, name: 'EVENTS_CSS', prefix: 'unfold-ev', sibling: true },
  settings: { file: 'unfold-settings-dsh/src/client/index.jsx', name: 'SETTINGS_CSS', prefix: 'unfold-set', sibling: true },
  base: { file: 'unfold-mind-dsh/src/client/ui.jsx', name: 'BASE_CSS', prefix: null },
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
  return Object.fromEntries(Object.entries(SHEETS).filter(([, v]) => !v.sibling || existsSync(url(v.file))).map(([k, v]) => {
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
  assert.ok(all.mind.rules.filter((r) => r.classes.includes('unfold-cap')).every((r) => r.first.includes('unfold-mind')), "Mind's .unfold-cap rules are under .unfold-mind")
  if (!all.presence) return
  const caps = all.presence.rules.filter((r) => r.classes.includes('unfold-pres-cap'))
  assert.ok(caps.length >= 3 && caps.every((r) => r.first.includes('unfold-pres')), 'the avatar caps are .unfold-pres .unfold-pres-cap')
  assert.ok(!all.presence.rules.some((r) => r.classes.includes('unfold-cap')), 'Presence no longer styles .unfold-cap')
  assert.match(read(EVENTS), /className="unfold-pres-cap top"/)
  assert.match(read(EVENTS), /className="unfold-pres-cap bot"/)
})

test('guard: the earlier collision (a global .unfold-cap in both Mind and Presence) is caught', () => {
  const mind = selectorsOf('.unfold-cap{margin-top:3px}')
  const pres = selectorsOf('.unfold-cap{position:absolute;left:22%}')
  const collide = mind.some((r) => r.classes.includes('unfold-cap') && !r.first.some((f) => f.startsWith('unfold-mind'))) && pres.some((r) => r.classes.includes('unfold-cap') && !r.first.some((f) => f.startsWith('unfold-pres')))
  assert.ok(collide)
})
