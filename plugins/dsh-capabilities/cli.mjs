#!/usr/bin/env node
// Print the capability map: `node cli.mjs` for a table, `--json` for the full map.
import { computeMap, loadRegistry, loadRuns } from './src/capmap.mjs'

const map = computeMap(loadRegistry(), loadRuns())
if (process.argv.includes('--json')) {
  process.stdout.write(`${JSON.stringify(map, null, 2)}\n`)
} else {
  const lmc = map.latest_measured_candidate
  console.log(`capability map from ${map.runs} bench runs (${map.unverified_runs.length} unverified), registry v${map.registry.version} ${map.registry.sha256.slice(0, 12)}`)
  console.log(`latest measured candidate (not an activation receipt): ${lmc ? `${lmc.backend} agentd ${lmc.agentd_sha256.slice(0, 12)} in ${lmc.run}` : 'none'}`)
  for (const [group, counts] of Object.entries(map.summary)) {
    console.log(`\n${group}: ${Object.entries(counts).filter(([, n]) => n).map(([s, n]) => `${n} ${s}`).join(', ')}`)
    for (const c of map.cells.filter((x) => x.group === group)) {
      const score = c.score === null ? '' : ` score ${c.score.toFixed(2)}${c.best === null ? '' : ` (best ${c.best.toFixed(2)} over ${c.trend.length})`}`
      const metrics = c.metrics.map((m) => ` ${m.metric} ${m.stat} ${m.value}${m.unit}/${m.max}`).join('')
      const tml = (c.reference?.records ?? []).filter((r) => r.system === 'TML-Interaction-Small')
      const ref = tml.length ? ` | TML ${tml.map((r) => `${r.metric}${r.condition === 'default' ? '' : ` (${r.condition})`} ${r.value}${r.unit === 's' ? ' s' : ''}${r.verified ? '' : '?'}`).join(', ')}` : ''
      console.log(`  ${c.status.padEnd(11)} ${c.id.padEnd(26)} ${c.title}${score}${metrics}${ref} [${c.reason_code ?? 'ok'}]`)
    }
  }
}
