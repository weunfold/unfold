#!/usr/bin/env node
// Build the RTH trajectory client: upstream @deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.2
// (MIT, Copyright (c) 2026 DeepSeek; upstream/lib/client.js, pinned below) with a documented patch
// set, plus the RTH runtime (src/*.mjs, bundled with esbuild). Every patch must match exactly the
// expected number of times or the build fails; NOTICE.md lists them by region.
//
//   node plugins/dsh-rth-trajectory/build.mjs [--out <file>]     (default lib/client.js)
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const UPSTREAM = join(HERE, 'upstream/lib/client.js')
const UPSTREAM_SHA256 = 'b11426f89d3b4302' // prefix; the full value is checked in test/build.test.mjs
const OUT = process.argv.includes('--out') ? process.argv[process.argv.indexOf('--out') + 1] : join(HERE, 'lib/client.js')
const sha = (s) => createHash('sha256').update(s).digest('hex')

let src = readFileSync(UPSTREAM, 'utf8')
if (!sha(src).startsWith(UPSTREAM_SHA256)) throw new Error(`upstream/lib/client.js is not the pinned upstream bundle (${sha(src)})`)

/** The patch set: [region, why, find (string or RegExp), replace, expected count]. */
export const PATCHES = [
  ['top (module requires)', 'load the RTH runtime (taxonomy, adapter, shared stream, badges, styles) inside the module factory', 'let _deepseek_ai_dsh_client_ui_primitives = require("@deepseek-ai/dsh-client-ui-primitives");', 'let _deepseek_ai_dsh_client_ui_primitives = require("@deepseek-ai/dsh-client-ui-primitives");\n\t\tconst RTH = (__RTH_RUNTIME__)(require);', 1],
  ['locales.js', 'own dictionary namespace (never the upstream one)', 'const NS = "trajectory";', 'const NS = "rth-trajectory";', 1],
  ['locales.js', 'RTH keys added to both dictionaries', 'const zh = {', 'const zh = {\n\t\t\t...RTH.ZH_KEYS,', 1],
  ['locales.js', 'RTH keys added to both dictionaries', 'const en = {', 'const en = {\n\t\t\t...RTH.EN_KEYS,', 1],
  ['TrajectoryTable.js', 'turn header cards: the RTH turn card replaces "Turn N" when one exists; the between-turns label is the short "Between"', 'return turn === null ? t("section.betweenTurns") : t("turn.label", { turn });', 'return turn === null ? RTH.BETWEEN : RTH.cardLabel(turn) ?? t("turn.label", { turn });', 1],
  ['TrajectoryTable.js', 'ledger row: the coloured RTH kind badge for RTH records', /className: TrajectoryTable_module_css_default\.kindSlot,(\s+)children: \(0, react_jsx_runtime\.jsxs\)\("span", \{/, 'className: TrajectoryTable_module_css_default.kindSlot,$1children: record.cell.rthKind !== void 0 ? RTH.kindTag(record.cell.rthKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsxs)("span", {', 1],
  ['TrajectoryTable.js', 'inspector header: the RTH kind badge', '}) : selected !== void 0 && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [(0, react_jsx_runtime.jsx)("span", {', '}) : selected !== void 0 && (0, react_jsx_runtime.jsxs)(react_jsx_runtime.Fragment, { children: [selected.cell.rthKind !== void 0 ? RTH.kindTag(selected.cell.rthKind, TrajectoryTable_module_css_default, _deepseek_ai_dsh_client_ui_primitives) : (0, react_jsx_runtime.jsx)("span", {', 1],
  ['TrajectoryTable.js', 'inspector Summary: no token rows for RTH records (tokens are not attributed)', 'selected.cell.kind === "message" && (0, react_jsx_runtime.jsx)(TokenRows, {', 'selected.cell.rthKind === void 0 && selected.cell.kind === "message" && (0, react_jsx_runtime.jsx)(TokenRows, {', 1],
  ['TrajectoryTable.js', 'inspector Summary: the RTH Timing block (named endpoints, clock, evidence ids, "not measured: reason")', /\(selected\.cell\.kind === "user" \|\| selected\.cell\.kind === "context"\) && (\(0, react_jsx_runtime\.jsxs\)\("div", \{ children: \[\(0, react_jsx_runtime\.jsx\)\("dt", \{ children: t\("timing\.duration"\) \}\), \(0, react_jsx_runtime\.jsx\)\("dd", \{ children: formatElapsedSeconds\(selected\.cell\.timeSeconds, t\) \}\)\] \}\))/, 'selected.cell.rthKind === void 0 && (selected.cell.kind === "user" || selected.cell.kind === "context") && $1,\n\t\t\t\t\t\t\t\t\t\t\t...(selected.cell.rthKind !== void 0 ? RTH.inspectorRows(selected.cell, react_jsx_runtime) : [])', 1],
  ['timeline.js', 'RTH records: their own kind, lane (five lanes) and span honesty (measured / instant / unknown)', /kind: cell\.kind,(\s+)label: cell\.text,(\s+)lane: laneFor\(cell\.kind\)/g, 'kind: cell.rthKind ?? cell.kind,$1label: cell.text,$2lane: cell.rthLane ?? laneFor(cell.kind),$2rthSpan: cell.rthSpan', 2],
  ['TrajectoryTable.js', 'turn header card: the measured chips on the turn\'s first row', /(children: resultText\s*\}\)\]\s*\}\))\]/, '$1, record.turnStart === true && record.cell.rthKind !== void 0 ? RTH.cardChips(record.turn) : null]', 1],
  ['TrajectoryTimeline.js', 'span honesty attribute (unknown = start marker + hatched, non-scaled "?" tail in every mode; instant = tick in timed modes)', '"data-timeline-span": span.kind,', '"data-timeline-span": span.kind,\n\t\t\t\t\t\t\t\t\t\t\t"data-rth-span": mode === "sequence" ? span.rthSpan === "unknown" ? "unknown-seq" : void 0 : span.rthSpan,', 1],
  ['TrajectoryTimeline.js', 'tooltip heading for RTH kinds', 'case "subtool": return t("kind.subtool");\n\t\t\t}', 'case "subtool": return t("kind.subtool");\n\t\t\t\tdefault: return RTH.kindLabel(kind) ?? kind;\n\t\t\t}', 1],
  ['TrajectoryTimeline.js', 'five lane labels: Hear, Think, Speak, Act, Life', /\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.input"\) \}\),\s+\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.model"\) \}\),\s+\(0, react_jsx_runtime\.jsx\)\("span", \{ children: t\("column\.tools"\) \}\)/, '...RTH.LANES.map((lane) => (0, react_jsx_runtime.jsx)("span", { children: lane }, lane))', 1],
  ['TrajectoryTimeline.module.css', 'room for five lanes', '._1p9O6q_selectionEdges[data-dragging=true]:before', '._1p9O6q_plot{height:78px}._1p9O6q_labels span:nth-child(4){top:49px}._1p9O6q_labels span:nth-child(5){top:63px}._1p9O6q_selectionEdges[data-dragging=true]:before', 1],
  ['TrajectoryView.js', 'the RTH view (same toolbar, overview, ledger and inspector; RTH data; kind filter chips and session strip)', '\t\t//#endregion\n\t\t//#region lib/types/client/index.js', '\t\t//#endregion\n__RTH_VIEW__\n\t\t//#region lib/types/client/index.js', 1],
  ['index.js', 'registration: its own conversation.view entry "rth-trajectory" labelled "RTH", its own right-sidebar tab kind "rth.trajectory" and the Bot header "Trajectory" button that opens it; no upstream Definitions, no uiSession hook, upstream Trajectory untouched', /\t\t\/\/#region lib\/types\/client\/index\.js[\s\S]*?\t\t\/\/#endregion\n\t\texports\.apply = apply;/, '__RTH_INDEX__\n\t\texports.apply = apply;', 1],
  ['all', 'own loader id, style-tag ids and plugin dataset (never the upstream ones)', /@deepseek-ai\/dsh-client-ui-trajectory/g, '@rth/dsh-rth-trajectory', 9],
  ['duration-store.js, string-wrapping-store.js', 'own persisted-preference names', /name: "dsh\.trajectory\./g, 'name: "rth.trajectory.', 2],
  ['TrajectoryTable.js', 'status: RTH records take their state from the validated RTH outcome, never the upstream "complete" fallback', 'function stateOf(record) {\n\t\t\tif (record.cell.isError) return "error";', 'function stateOf(record) {\n\t\t\tif (record.cell.rthKind !== void 0) return RTH.stateOf(record.cell);\n\t\t\tif (record.cell.isError) return "error";', 1],
  ['TrajectoryTable.js', 'status labels for the RTH-only states (pending, cancelled, not attributed, observed, unclassified)', 'function statusLabel(state, t) {\n\t\t\tif (state === "error")', 'function statusLabel(state, t) {\n\t\t\tif (typeof state === "string" && state.startsWith("rth-")) return t(`rth.status.${state.slice(4)}`);\n\t\t\tif (state === "error")', 1],
  ['TrajectoryTable.js', 'no upstream "Started / session timestamps" timing block for RTH records (their timing is the RTH Timing block)', 'function RecordTiming({ record, preview = false, t }) {\n', 'function RecordTiming({ record, preview = false, t }) {\n\t\t\tif (record.cell.rthKind !== void 0) return null;\n', 1],
  ['TrajectoryTimeline.js', 'tooltip detail: a measured duration only, never observed_at -> observed_at + duration', 'function timelineRecordDetail(cell) {\n', 'function timelineRecordDetail(cell) {\n\t\t\tif (cell.rthKind !== void 0) return RTH.timelineDetail(cell);\n', 1],
  ['timeline.js', 'timed ranges: an RTH record is an instant at its observation time, never observed_at + a producer duration', 'function cellRange(cell) {\n\t\t\tif (!finite(cell.startedAt)) return null;\n', 'function cellRange(cell) {\n\t\t\tif (!finite(cell.startedAt)) return null;\n\t\t\tif (cell.rthKind !== void 0) return {\n\t\t\t\tstart: cell.startedAt,\n\t\t\t\tend: cell.startedAt\n\t\t\t};\n', 1],
  ['TrajectoryToolbar.js', 'duration toggle hook (rthSequenceOnly): the toggle stays and maps Duration to the producer-clock overview; the time toggle stays hidden upstream', 'function TrajectoryToolbar({ actualDuration,', 'function TrajectoryToolbar({ rthSequenceOnly, actualDuration,', 1],
  ['TrajectoryToolbar.js', 'duration toggle hook (rthSequenceOnly): the toggle stays and maps Duration to the producer-clock overview; the time toggle stays hidden upstream', '"aria-pressed": actualDuration,', 'hidden: rthSequenceOnly === true,\n\t\t\t\t\t\t\t\t"aria-pressed": actualDuration,', 1],
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
for (const p of prefixes) src = src.split(p).join(`rtj${p.replace(/^_/, '')}`)

// The RTH runtime, bundled (React and DSH primitives come from the module loader's require).
const require = createRequire(join(HERE, '../package.json'))
const esbuild = require('esbuild')
const rt = await esbuild.build({ entryPoints: [join(HERE, 'src/runtime.mjs')], bundle: true, write: false, format: 'cjs', platform: 'browser', target: 'es2022', external: ['react', 'react/jsx-runtime', '@deepseek-ai/dsh-client-ui-primitives'], legalComments: 'inline', logLevel: 'warning', absWorkingDir: HERE })
const runtime = `function (require) {\n\t\t\tvar module = { exports: {} };\n\t\t\tvar exports = module.exports;\n${rt.outputFiles[0].text}\n\t\t\treturn module.exports;\n\t\t}`
src = src.replace('__RTH_RUNTIME__', () => runtime)
src = src.replace('__RTH_VIEW__', () => readFileSync(join(HERE, 'src/view.js'), 'utf8'))
src = src.replace('__RTH_INDEX__', () => readFileSync(join(HERE, 'src/index-region.js'), 'utf8'))

const block = (file) => readFileSync(join(HERE, file), 'utf8').trim().split('\n').map((l) => ` * ${l}`.trimEnd()).join('\n')
const lucide = JSON.parse(readFileSync(join(HERE, 'third_party/lucide-static-0.460.0/SOURCE.json'), 'utf8'))
const feather = JSON.parse(readFileSync(join(HERE, 'third_party/feather-icons-4.29.2/SOURCE.json'), 'utf8'))
const header = `/*!
 * @rth/dsh-rth-trajectory: a fork of @deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.2
 * (upstream lib/client.js sha256 ${sha(readFileSync(UPSTREAM, 'utf8'))}).
 * Modified by we unfold; the changed regions are listed in NOTICE.md. Upstream license:
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
 */
`
writeFileSync(OUT, header + src)
console.log(`${OUT}: ${(header + src).length} bytes, sha256 ${sha(header + src)}; ${PATCHES.length} patches, ${prefixes.size} CSS prefixes renamed`)
