// Builds client.js, the DSH client half: upstream @deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.3
// (MIT, Copyright (c) 2026 DeepSeek; upstream/lib/client.js, pinned below) with a documented patch
// set, plus the Unfold runtime (src/*.mjs, bundled with esbuild). Every patch must match exactly the
// expected number of times or the build fails; NOTICE.md lists them by region.
//
//   node scripts/build-client.mjs                 writes client.js
//   node scripts/build-client.mjs --check         exits 1 when client.js differs from a fresh build
//   node scripts/build-client.mjs --out <file>    writes <file> instead
//
// client.js is committed, so `link:` and git installs need no build step.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'

const HERE = dirname(dirname(fileURLToPath(import.meta.url)))
const UPSTREAM = join(HERE, 'upstream/lib/client.js')
const UPSTREAM_SHA256 = '391740afdfde2e8d' // prefix; the full value is checked in test/build.test.js
const OUTPUT = join(HERE, 'client.js')
// DSH's client loader matches a bundle's id to the npm package that ships it.
const PACKAGE_NAME = JSON.parse(readFileSync(join(HERE, 'package.json'), 'utf8')).name
const sha = (s) => createHash('sha256').update(s).digest('hex')

let src = readFileSync(UPSTREAM, 'utf8')
if (!sha(src).startsWith(UPSTREAM_SHA256)) throw new Error(`upstream/lib/client.js is not the pinned upstream bundle (${sha(src)})`)

/** The patch set: [region, why, find (string or RegExp), replace, expected count]. */
export const PATCHES = [
  ['top (module requires)', 'load the Unfold runtime (taxonomy, adapter, shared stream, badges, styles) inside the module factory', 'let _deepseek_ai_dsh_client_ui_primitives = require("@deepseek-ai/dsh-client-ui-primitives");', 'let _deepseek_ai_dsh_client_ui_primitives = require("@deepseek-ai/dsh-client-ui-primitives");\n\t\tconst UNFOLD = (__UNFOLD_RUNTIME__)(require);', 1],
  ['locales.js', 'own dictionary namespace (never the upstream one)', 'const NS = "trajectory";', 'const NS = "unfold-trajectory";', 1],
  ['locales.js', 'Unfold keys added to both dictionaries', 'const zh = {', 'const zh = {\n\t\t\t...UNFOLD.ZH_KEYS,', 1],
  ['locales.js', 'Unfold keys added to both dictionaries', 'const en = {', 'const en = {\n\t\t\t...UNFOLD.EN_KEYS,', 1],
  ['TrajectoryTable.js', 'turn header cards: the Unfold turn card replaces "Turn N" when one exists; the between-turns label is the short "Between"', 'return turn === null ? t("section.betweenTurns") : t("turn.label", { turn });', 'return turn === null ? UNFOLD.BETWEEN : UNFOLD.cardLabel(turn) ?? t("turn.label", { turn });', 1],
  ['TrajectoryTable.js', 'ledger row: the coloured Unfold kind badge for Unfold records', /className: TrajectoryTable_module_css_default\.kindSlot,(\s+)children: \(0, react_jsx_runtime\.jsxs\)\("span", \{/, 'className: TrajectoryTable_module_css_default.kindSlot,$1children: record.cell.unfoldKind !== void 0 ? UNFOLD.kindTag(record.cell.unfoldKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsxs)("span", {', 1],
  ['TrajectoryTable.js', 'inspector header: the Unfold kind badge', '}) : selected !== void 0 && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)("span", {', '}) : selected !== void 0 && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [selected.cell.unfoldKind !== void 0 ? UNFOLD.kindTag(selected.cell.unfoldKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsx)("span", {', 1],
  ['TrajectoryTable.js', 'inspector Summary: no token rows for Unfold records (tokens are not attributed)', 'selected.cell.kind === "message" && (0, react_jsx_runtime.jsx)(TokenRows, {', 'selected.cell.unfoldKind === void 0 && selected.cell.kind === "message" && (0, react_jsx_runtime.jsx)(TokenRows, {', 1],
  ['TrajectoryTable.js', 'inspector Summary: the Unfold Timing block (named endpoints, clock, evidence ids, "not measured: reason")', /\(selected\.cell\.kind === "user" \|\| selected\.cell\.kind === "context"\) && (\(0, react_jsx_runtime\.jsxs\)\("div", \{ children: \[\(0, react_jsx_runtime\.jsx\)\("dt", \{ children: t\("timing\.duration"\) \}\), \(0, react_jsx_runtime\.jsx\)\("dd", \{ children: formatElapsedSeconds\(selected\.cell\.timeSeconds, t\) \}\)\] \}\))/, 'selected.cell.unfoldKind === void 0 && (selected.cell.kind === "user" || selected.cell.kind === "context") && $1,\n\t\t\t\t\t\t\t\t\t\t\t...(selected.cell.unfoldKind !== void 0 ? UNFOLD.inspectorRows(selected.cell, react_jsx_runtime) : [])', 1],
  ['timeline.js', 'Unfold records: their own kind, lane (five lanes) and span honesty (measured / instant / unknown)', /kind: cell\.kind,(\s+)label: cell\.text,(\s+)lane: laneFor\(cell\.kind\)/g, 'kind: cell.unfoldKind ?? cell.kind,$1label: cell.text,$2lane: cell.unfoldLane ?? laneFor(cell.kind),$2unfoldSpan: cell.unfoldSpan', 2],
  ['TrajectoryTable.js', 'turn header card: the measured chips on the turn\'s first row', /(children: resultText\s*\}\)\]\s*\}\))\]/, '$1, record.turnStart === true && record.cell.unfoldKind !== void 0 ? UNFOLD.cardChips(record.turn) : null]', 1],
  ['TrajectoryTimeline.js', 'span honesty attribute (unknown = start marker + hatched, non-scaled "?" tail in every mode; instant = tick in timed modes)', '"data-timeline-span": span.kind,', '"data-timeline-span": span.kind,\n\t\t\t\t\t\t\t\t\t\t\t"data-unfold-span": mode === "sequence" ? span.unfoldSpan === "unknown" ? "unknown-seq" : void 0 : span.unfoldSpan,', 1],
  ['TrajectoryTimeline.js', 'tooltip heading for Unfold kinds', 'case "subtool": return t("kind.subtool");\n\t\t\t}', 'case "subtool": return t("kind.subtool");\n\t\t\t\tdefault: return UNFOLD.kindLabel(kind) ?? kind;\n\t\t\t}', 1],
  ['TrajectoryTimeline.js', 'five lane labels: Hear, Think, Speak, Act, Life', /\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.input"\) \}\),\s+\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.model"\) \}\),\s+\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.tools"\) \}\)/, '...UNFOLD.LANES.map((lane) => (0, react_jsx_runtime.jsx)("span", { children: lane }, lane))', 1],
  ['TrajectoryTimeline.module.css', 'room for five lanes', '._1p9O6q_selectionEdges[data-dragging=true]:before', '._1p9O6q_plot{height:78px}._1p9O6q_labels span:nth-child(4){top:49px}._1p9O6q_labels span:nth-child(5){top:63px}._1p9O6q_selectionEdges[data-dragging=true]:before', 1],
  ['TrajectoryView.js', 'the Unfold view (same toolbar, overview, ledger and inspector; Unfold data; kind filter chips and session strip)', '\t\t//#endregion\n\t\t//#region lib/types/client/index.js', '\t\t//#endregion\n__UNFOLD_VIEW__\n\t\t//#region lib/types/client/index.js', 1],
  ['index.js', 'registration: its own conversation.view entry "unfold-trajectory" labelled "Unfold", its own right-sidebar tab kind "unfold.trajectory" and the Bot header "Trajectory" button that opens it; no upstream Definitions, no uiSession hook, upstream Trajectory untouched', /\t\t\/\/#region lib\/types\/client\/index\.js[\s\S]*?\t\t\/\/#endregion\n\t\texports\.apply = apply;/, '__UNFOLD_INDEX__\n\t\texports.apply = apply;', 1],
  ['all', 'own loader id, style-tag ids and plugin dataset (never the upstream ones)', /@deepseek-ai\/dsh-client-ui-trajectory/g, PACKAGE_NAME, 9],
  ['duration-store.js, string-wrapping-store.js', 'own persisted-preference names', /name: "dsh\.trajectory\./g, 'name: "unfold.trajectory.', 2],
  ['TrajectoryTable.js', 'status: Unfold records take their state from the validated Unfold outcome, never the upstream "complete" fallback', 'function stateOf(record) {\n\t\t\tif (record.cell.isError) return "error";', 'function stateOf(record) {\n\t\t\tif (record.cell.unfoldKind !== void 0) return UNFOLD.stateOf(record.cell);\n\t\t\tif (record.cell.isError) return "error";', 1],
  ['TrajectoryTable.js', 'status labels for the Unfold-only states (pending, cancelled, not attributed, observed, unclassified)', 'function statusLabel(state, t) {\n\t\t\tif (state === "error")', 'function statusLabel(state, t) {\n\t\t\tif (typeof state === "string" && state.startsWith("unfold-")) return t(`unfold.status.${state.slice(7)}`);\n\t\t\tif (state === "error")', 1],
  ['TrajectoryTable.js', 'no upstream "Started / session timestamps" timing block for Unfold records (their timing is the Unfold Timing block)', 'function RecordTiming({ record, preview = false, t }) {\n', 'function RecordTiming({ record, preview = false, t }) {\n\t\t\tif (record.cell.unfoldKind !== void 0) return null;\n', 1],
  ['TrajectoryTimeline.js', 'tooltip detail: a measured duration only, never observed_at -> observed_at + duration', 'function timelineRecordDetail(cell) {\n', 'function timelineRecordDetail(cell) {\n\t\t\tif (cell.unfoldKind !== void 0) return UNFOLD.timelineDetail(cell);\n', 1],
  ['timeline.js', 'timed ranges: an Unfold record is an instant at its observation time, never observed_at + a producer duration', 'function cellRange(cell) {\n\t\t\tif (!finite(cell.startedAt)) return null;\n', 'function cellRange(cell) {\n\t\t\tif (!finite(cell.startedAt)) return null;\n\t\t\tif (cell.unfoldKind !== void 0) return {\n\t\t\t\tstart: cell.startedAt,\n\t\t\t\tend: cell.startedAt\n\t\t\t};\n', 1],
  ['TrajectoryToolbar.js', 'duration toggle hook (unfoldSequenceOnly): keeps the toggle and maps Duration to the producer-clock overview; the time toggle stays hidden upstream', 'function TrajectoryToolbar({ actualDuration,', 'function TrajectoryToolbar({ unfoldSequenceOnly, actualDuration,', 1],
  ['TrajectoryToolbar.js', 'duration toggle hook (unfoldSequenceOnly): keeps the toggle and maps Duration to the producer-clock overview; the time toggle stays hidden upstream', '"aria-pressed": actualDuration,', 'hidden: unfoldSequenceOnly === true,\n\t\t\t\t\t\t\t\t"aria-pressed": actualDuration,', 1],
  ['CSS-module regions', 'region comments name each CSS module by its path in the upstream repository, never the machine that built it', /(\/\/#region \\0dsh-css:)(?:\/[^\n]*?\/)?(?=packages\/client\/)/g, '$1', 4],
]

function apply(text, [region, why, find, replace, expected]) {
  if (typeof find === 'string') {
    const n = text.split(find).length - 1
    if (n !== expected) throw new Error(`patch "${why}" (${region}) matched ${n} times, expected ${expected}`)
    return text.split(find).join(replace)
  }
  const n = [...text.matchAll(new RegExp(find.source, find.flags.includes('g') ? find.flags : `${find.flags}g`))].length
  if (n !== expected) throw new Error(`patch "${why}" (${region}) matched ${n} times, expected ${expected}`)
  return text.replace(find, replace)
}
for (const p of PATCHES) src = apply(src, p)

// CSS-module class hashes get our own prefix, so no upstream rule can ever reach these elements
// (and ours never reach upstream's) when both plugins are loaded in one document.
const prefixes = new Set()
for (const m of src.matchAll(/_module_css_default = \{([\s\S]*?)\n\t\t\};/g)) {
  for (const e of m[1].matchAll(/"([\w-]+)": "([\w-]+)"/g)) if (e[2].endsWith(e[1])) prefixes.add(e[2].slice(0, -e[1].length))
}
if (prefixes.size !== 4) throw new Error(`expected 4 CSS-module prefixes, found ${[...prefixes].join(', ')}`)
for (const p of prefixes) src = src.split(p).join(`uft${p.replace(/^_/, '')}`)

// The Unfold runtime, bundled (React and DSH primitives come from the module loader's require).
const rt = await build({ entryPoints: [join(HERE, 'src/runtime.mjs')], bundle: true, write: false, format: 'cjs', platform: 'browser', target: 'es2022', external: ['react', 'react/jsx-runtime', '@deepseek-ai/dsh-client-ui-primitives'], legalComments: 'inline', logLevel: 'warning', absWorkingDir: HERE })
const runtime = `function (require) {\n\t\t\tvar module = { exports: {} };\n\t\t\tvar exports = module.exports;\n${rt.outputFiles[0].text}\n\t\t\treturn module.exports;\n\t\t}`
src = src.replace('__UNFOLD_RUNTIME__', () => runtime)
src = src.replace('__UNFOLD_VIEW__', () => readFileSync(join(HERE, 'src/view.js'), 'utf8'))
src = src.replace('__UNFOLD_INDEX__', () => readFileSync(join(HERE, 'src/index-region.js'), 'utf8'))

const block = (file) => readFileSync(join(HERE, file), 'utf8').trim().split('\n').map((l) => ` * ${l}`.trimEnd()).join('\n')
const lucide = JSON.parse(readFileSync(join(HERE, 'third_party/lucide-static-0.460.0/SOURCE.json'), 'utf8'))
const feather = JSON.parse(readFileSync(join(HERE, 'third_party/feather-icons-4.29.2/SOURCE.json'), 'utf8'))
const diff = JSON.parse(readFileSync(join(HERE, 'third_party/diff-9.0.0/SOURCE.json'), 'utf8'))
const header = `/*!
 * ${PACKAGE_NAME}: a fork of @deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.3
 * (upstream lib/client.js sha256 ${sha(readFileSync(UPSTREAM, 'utf8'))}).
 * Modified by the Automagik contributors; the changed regions are listed in NOTICE.md.
 * Upstream license:
 *
${block('LICENSE')}
 *
 * Badge icons: element data from lucide-static ${lucide.version} (${lucide.integrity}),
 * pinned in src/icons.generated.mjs. Lucide license:
 *
${block('third_party/lucide-static-0.460.0/LICENSE')}
 *
 * Portions of Lucide are Feather (feather-icons ${feather.version}, ${feather.integrity}). Feather license:
 *
${block('third_party/feather-icons-4.29.2/LICENSE')}
 *
 * Upstream's bundle includes jsdiff (${diff.package} ${diff.version}, ${diff.integrity}). jsdiff license:
 *
${block('third_party/diff-9.0.0/LICENSE')}
 */
`
const next = header + src
const out = process.argv.includes('--out') ? process.argv[process.argv.indexOf('--out') + 1] : OUTPUT
if (process.argv.includes('--check')) {
  let current = null
  try { current = readFileSync(OUTPUT, 'utf8') } catch { /* missing counts as stale */ }
  if (current !== next) {
    console.error('client.js is stale: run `npm run build` in plugins/unfold-trajectory-dsh and commit the result')
    process.exit(1)
  }
} else {
  writeFileSync(out, next)
  console.log(`${out}: ${next.length} bytes, sha256 ${sha(next)}; ${PATCHES.length} patches, ${prefixes.size} CSS prefixes renamed`)
}
