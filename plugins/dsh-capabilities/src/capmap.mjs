// The capability map: a pure function of capabilities/registry.json and the RTH-Bench runs
// (scorecard.json joined to its manifest.json). Nothing here judges a capability; it only
// reads what the bench scored, and only compares scores measured under the same contract.
//
// A run is verified when its manifest names the same suite, scorer and candidate as its
// scorecard and hashes every scored task. Best and trend come only from verified, eligible
// runs inside the registry's declared comparison cohort (suite id/sha/epoch, scorer id/version,
// selection split, exact task@version) whose candidate meets the cell's prerequisites. Everything else is listed with why it does not count. The newest such
// run is the latest measured candidate: a measurement, never proof of what is deployed.
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('../../../', import.meta.url))
export const DEFAULT_REGISTRY = join(ROOT, 'capabilities/registry.json')
export const DEFAULT_RESULTS = join(ROOT, 'bench/results')
export const STATUSES = ['unsupported', 'unverified', 'failing', 'proxy', 'supported']
const TASK_REF = /^[a-z0-9_.-]+@\d+$/
const SHA256 = /^[0-9a-f]{64}$/

export function loadRegistry(path = DEFAULT_REGISTRY) {
  const text = readFileSync(path, 'utf8')
  const doc = JSON.parse(text)
  if (doc.registry_format !== 'rth.capabilities/v2') throw new Error(`unknown registry format ${doc.registry_format}`)
  const ids = new Set()
  for (const cell of doc.cells) {
    if (ids.has(cell.id)) throw new Error(`registry lists cell ${cell.id} twice`)
    if (!doc.groups[cell.group]) throw new Error(`${cell.id}: unknown group ${cell.group}`)
    for (const ref of cell.evidence) if (!TASK_REF.test(ref)) throw new Error(`${cell.id}: evidence ${ref} is not task@version`)
    ids.add(cell.id)
  }
  return { ...doc, sha256: createHash('sha256').update(text).digest('hex') }
}

/** Every run with a scorecard, oldest first, each joined to its manifest and checked. */
export function loadRuns(dir = DEFAULT_RESULTS) {
  if (!existsSync(dir)) return []
  const read = (run, file) => (existsSync(join(dir, run, file)) ? JSON.parse(readFileSync(join(dir, run, file), 'utf8')) : null)
  return readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isDirectory() && existsSync(join(dir, d.name, 'scorecard.json')))
    .map((d) => joinRun(read(d.name, 'scorecard.json'), read(d.name, 'manifest.json')))
    .filter((r) => r.scorecard.scorecard_format === 'rth-bench.scorecard/v1')
    .sort((a, b) => a.started_utc.localeCompare(b.started_utc))
}

/** One run: its scores by exact task@version, and the provenance its manifest establishes. */
export function joinRun(scorecard, manifest) {
  const scores = taskScores(scorecard)
  const problems = []
  const m = manifest
  if (!m) problems.push('no manifest.json')
  else {
    if (m.manifest_format !== 'rth-bench.run-manifest/v1') problems.push(`manifest format ${m.manifest_format}`)
    if (m.run_id !== scorecard.run_id) problems.push(`manifest is for run ${m.run_id}`)
    if (m.suite?.id !== scorecard.suite_id || m.suite?.sha256 !== scorecard.suite_sha256 || m.suite?.epoch !== scorecard.suite_epoch) problems.push('suite differs between manifest and scorecard')
    if (!SHA256.test(m.suite?.sha256 ?? '')) problems.push('suite has no sha256')
    if (m.scorer?.id !== scorecard.scorer?.id || m.scorer?.version !== scorecard.scorer?.version) problems.push('scorer differs between manifest and scorecard')
    if (!SHA256.test(m.candidate?.agentd_sha256 ?? '')) problems.push('candidate has no agentd_sha256')
    else if (m.candidate.agentd_sha256_in_guest && m.candidate.agentd_sha256_in_guest !== m.candidate.agentd_sha256) problems.push('guest ran a different agentd')
    if (m.candidate?.backend !== scorecard.candidate?.backend) problems.push('backend differs between manifest and scorecard')
    const hashed = new Map((m.tasks ?? []).map((t) => [t.task, t.sha256]))
    const unhashed = [...scores.keys()].filter((ref) => !SHA256.test(hashed.get(ref) ?? ''))
    if (unhashed.length) problems.push(`manifest does not hash ${unhashed.join(', ')}`)
  }
  return {
    run_id: scorecard.run_id,
    started_utc: scorecard.started_utc,
    scorecard,
    scores,
    verified: problems.length === 0,
    problems,
    provenance: {
      suite: { id: scorecard.suite_id ?? null, sha256: scorecard.suite_sha256 ?? null, epoch: scorecard.suite_epoch ?? null },
      scorer: { id: scorecard.scorer?.id ?? null, version: scorecard.scorer?.version ?? null },
      selection_split: scorecard.selection_split ?? null,
      candidate: { backend: scorecard.candidate?.backend ?? null, agentd_sha256: m?.candidate?.agentd_sha256 ?? null },
      task_sha256: Object.fromEntries((m?.tasks ?? []).map((t) => [t.task, t.sha256])),
    },
  }
}

/** Exact task@version -> { score, status } on the scorecard's selection split. */
export function taskScores(scorecard) {
  const out = new Map()
  for (const t of scorecard.tasks ?? []) {
    const split = t.splits?.[scorecard.selection_split]
    if (split) out.set(t.task, { score: split.score, status: split.status })
  }
  for (const p of scorecard.probes ?? []) out.set(p.task, { score: p.score, status: p.status })
  return out
}

/** Why `run` is outside `cohort` for evidence `refs`, or null when it is comparable. */
export function outsideCohort(run, cohort, refs) {
  const p = run.provenance
  if (!cohort) return 'the registry declares no comparison cohort'
  if (p.suite.id !== cohort.suite_id || p.suite.sha256 !== cohort.suite_sha256 || p.suite.epoch !== cohort.suite_epoch) return `suite ${p.suite.id}@${p.suite.epoch} (${String(p.suite.sha256).slice(0, 12)}) is not the cohort's`
  if (p.scorer.id !== cohort.scorer_id || p.scorer.version !== cohort.scorer_version) return `scorer ${p.scorer.id}@${p.scorer.version} is not the cohort's`
  if (p.selection_split !== cohort.selection_split) return `selection split ${p.selection_split} is not the cohort's (${cohort.selection_split})`
  const missing = refs.filter((ref) => !run.scores.has(ref))
  if (missing.length) return `does not score ${missing.join(', ')}`
  return null
}

export function computeMap(registry, runs) {
  const verified = runs.filter((r) => r.verified)
  const latest = verified.at(-1) ?? null
  const cells = registry.cells.map((cell) => mapCell(cell, runs, cell.comparison ?? registry.comparison))
  const summary = {}
  for (const c of cells) {
    summary[c.group] ??= Object.fromEntries(STATUSES.map((s) => [s, 0]))
    summary[c.group][c.status]++
  }
  return {
    registry: { version: registry.registry_version, sha256: registry.sha256 ?? null },
    comparison: registry.comparison ?? null,
    latest_run: runs.at(-1)?.run_id ?? null,
    runs: runs.length,
    latest_measured_candidate: latest && measured(latest),
    unverified_runs: runs.filter((r) => !r.verified).map((r) => ({ run: r.run_id, why: r.problems })),
    summary,
    cells,
  }
}

function mapCell(cell, runs, cohort) {
  const base = {
    id: cell.id, group: cell.group, title: cell.title, verb: cell.verb ?? null, modality: cell.modality ?? null,
    evidence: cell.evidence, reference: cell.reference ?? null, benchmark_url: cell.benchmark_url ?? null, note: cell.note ?? null,
  }
  const empty = { score: null, best: null, trend: [], run: null, measured_candidate: null, metrics: [], evidence_scores: [], ineligible_runs: [], incomparable_runs: [], unverified_runs: [], prerequisite_missing_runs: [] }
  const result = (status, reason_code, reason, extra = {}) => ({ ...base, ...empty, status, reason_code, reason, ...extra })
  if (!cell.evidence.length) return result('unsupported', 'not_evaluated', 'no bench evidence')

  const lists = { ineligible_runs: [], incomparable_runs: [], unverified_runs: [], prerequisite_missing_runs: [] }
  const covering = []
  for (const run of runs) {
    const covers = cell.evidence.every((ref) => run.scores.has(ref))
    if (!run.verified) {
      if (covers) lists.unverified_runs.push({ run: run.run_id, why: run.problems })
      continue
    }
    const outside = outsideCohort(run, cohort, cell.evidence)
    if (outside) lists.incomparable_runs.push({ run: run.run_id, why: outside })
    else covering.push(run)
  }
  // Task content must match too: the same task@version re-hashed is a different task.
  const pin = covering.at(-1)?.provenance.task_sha256
  const cohortRuns = covering.filter((run) => {
    const drift = cell.evidence.filter((ref) => run.provenance.task_sha256[ref] !== pin[ref])
    if (drift.length) lists.incomparable_runs.push({ run: run.run_id, why: `task content differs for ${drift.join(', ')}` })
    return !drift.length
  })
  for (const run of cohortRuns) if (!run.scorecard.eligible) lists.ineligible_runs.push({ run: run.run_id, score: cellScore(cell, run), why: failedRules(run.scorecard) })

  const last = cohortRuns.at(-1)
  if (!last) {
    if (lists.unverified_runs.length) return result('unverified', 'unverified_provenance', 'only runs without verifiable provenance scored this', lists)
    return result('unsupported', 'not_evaluated', 'evidence never scored in the comparison cohort', lists)
  }
  // Prerequisites hold for every history member, not only the newest: a fake run is never a real-model best.
  const qualified = cohortRuns.filter((run) => {
    const lacking = missingRequirements(cell.requires, run.scorecard.candidate)
    if (lacking.length) lists.prerequisite_missing_runs.push({ run: run.run_id, why: `candidate ${run.provenance.candidate.backend} lacks ${lacking.join(', ')}` })
    return !lacking.length
  })
  const history = qualified.filter((run) => run.scorecard.eligible).map((run) => ({ run: run.run_id, score: cellScore(cell, run) }))
  const common = {
    ...lists,
    run: last.run_id,
    measured_candidate: measured(last),
    evidence_scores: cell.evidence.map((ref) => ({ task: ref, ...last.scores.get(ref), sha256: last.provenance.task_sha256[ref] })),
    best: history.length ? Math.max(...history.map((t) => t.score)) : null,
    trend: history,
  }
  const lacking = missingRequirements(cell.requires, last.scorecard.candidate)
  if (lacking.length) return result('unsupported', 'prerequisite_missing', `scored candidate ${last.provenance.candidate.backend} lacks ${lacking.join(', ')}`, common)

  const eligible = last.scorecard.eligible
  const passed = eligible && cell.evidence.every((ref) => last.scores.get(ref).status === 'pass')
  return {
    ...base,
    ...empty,
    ...common,
    status: !passed ? 'failing' : cell.proxy ? 'proxy' : 'supported',
    proxy_kind: passed && cell.proxy ? cell.proxy_kind ?? 'text' : null,
    reason_code: passed ? null : eligible ? 'task_failed' : 'scorecard_ineligible',
    reason: passed ? null : eligible ? 'an evidence task did not pass' : 'scorecard not eligible',
    score: cellScore(cell, last),
    metrics: (last.scorecard.envelopes ?? []).filter((e) => cell.metrics?.includes(e.metric)).map(({ metric, stat, value, max, unit, n, pass }) => ({ metric, stat, value, max, unit, n, pass })),
  }
}

const measured = (run) => ({ run: run.run_id, backend: run.provenance.candidate.backend, agentd_sha256: run.provenance.candidate.agentd_sha256, label: 'latest measured candidate (not an activation receipt)' })
const cellScore = (cell, run) => mean(cell.evidence.map((ref) => run.scores.get(ref).score))
const failedRules = (scorecard) => {
  const rules = (scorecard.eligibility ?? []).filter((r) => !r.pass).map((r) => r.rule)
  return rules.length ? rules : ['scorecard marked ineligible']
}

function missingRequirements(requires = {}, candidate = {}) {
  const lacking = (requires.capabilities ?? []).filter((c) => candidate?.capabilities?.[c] !== true)
  if (requires.real_model && (!candidate?.backend || candidate.backend.startsWith('fake'))) lacking.push('a real model')
  return lacking
}

const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length
