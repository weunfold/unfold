// Cosmic points: a read-only view over an append-only JSONL ledger (schema rth.cp-ledger/v1). The
// plugin never writes the ledger. Records are judged in ONE pass in append order, so each act is
// judged against the history before it:
//   policy_approved        binds the full versioned POLICY (its sha256, rules included), the exact
//                          bootstrap records (ids and the sha256 of their canonical content) and a
//                          pinned decision receipt by the configured approver; nothing counts before it;
//   activation_earned      +10 for one activated build: a pinned, typed manifest whose computed
//                          build_id (artifact content) the record names; one award per activation_id
//                          and one per build_id, so a reinstall of the same artifacts earns nothing;
//   award_rejected         an appended correction that stops a named earlier award from that point;
//   activation_rolled_back recorded only: a rollback never takes points back;
//   evolution              the explicit move to the next stage, judged at its own position.
// A record counts only once its line is newline-terminated. record_id is unique: an exact replay is
// idempotent, and a conflicting reuse makes the whole view unavailable. A later line may never erase
// an earlier one, so nothing rejected earlier can be revived by a later conflict.
// This is local, auditable storage, not tamper-proof permanence.
import { createHash } from 'node:crypto'
import { readFileSync, statSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const SCHEMA = 'rth.cp-ledger/v1'
export const DEFAULT_LEDGER = join(dirname(fileURLToPath(import.meta.url)), '../../../cp/ledger/cp-ledger.jsonl')
/** Who may approve the policy, unless the plugin is configured with another id. */
export const DEFAULT_APPROVER = 'owner'
export const MAX_BYTES = 1_000_000
export const MAX_RECORDS = 5_000
const DIGEST = /^[0-9a-f]{64}$/
const DECISION = /^cp\/decisions\/[\w.-]+\.md$/

/** The full policy an approval binds: the arithmetic AND the rules. Any change is a new version. */
export const POLICY = Object.freeze({
  policy_id: 'rth-cosmic-points',
  policy_version: 4,
  start_cp: 100,
  cp_per_activation: 10,
  grains_per_cp: 100,
  level: '1 + floor((cp - 100) / 10)',
  colour_every_levels: 10,
  palette: Object.freeze(['cyan', 'magenta', 'violet', 'amber', 'rose', 'mint', 'ice-blue', 'coral', 'lime', 'white-gold']),
  stages: Object.freeze([['Seed', 100], ['Mote', 200], ['Wisp', 400], ['Sprite', 700], ['Genie', 1000]].map((x) => Object.freeze(x))),
  rules: Object.freeze({
    commit: 'a record counts only when its line ends with a newline',
    approval: 'policy_approved binds this policy sha256, the exact bootstrap records (ids and canonical sha256, appearing before it) and a pinned cp/decisions/*.md decision receipt by the configured approver; records before it outside the bootstrap never count',
    eligibility: 'activation_earned: cp_delta 10, a pinned typed manifest (files[{path,sha256}] all ok) whose computed build_id the record names',
    dedup: 'one award per activation_id (conflicting claims on one activation void it from that point) and per build_id; record_id unique: an exact replay is idempotent, a conflicting reuse makes the whole ledger view unavailable (contradictory history), never a recount',
    correction: 'award_rejected passes the same base validation, names an earlier counted award and stops it from that point; a correction that leaves the chosen stage below its threshold is disclosed and the stage stays; activation_rolled_back never reduces CP',
    evolution: 'explicit, next stage only, judged against the CP and stage at its own position; never revived by later records',
  }),
})

/** Canonical JSON: keys sorted at every depth (nested evidence included). */
export function canonical(v) {
  if (Array.isArray(v)) return `[${v.map(canonical).join(',')}]`
  if (v && typeof v === 'object') return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonical(v[k])}`).join(',')}}`
  return JSON.stringify(v)
}
const sha256 = (s) => createHash('sha256').update(s).digest('hex')
export const POLICY_SHA256 = sha256(canonical(POLICY))
export const PALETTE = POLICY.palette
export const STAGES = POLICY.stages

export const levelOf = (cp) => 1 + Math.floor((cp - POLICY.start_cp) / POLICY.cp_per_activation)
export const coloursOf = (level) => Math.min(PALETTE.length, 1 + Math.floor((level - 1) / POLICY.colour_every_levels))
export const availableStageOf = (cp) => STAGES.filter(([, t]) => cp >= t).at(-1)[0]
const stageIndex = (name) => STAGES.findIndex(([n]) => n === name)

/** The stable build identity: the artifact content of a typed manifest, apart from activation metadata. */
export function buildIdOf(manifest) {
  if (!manifest || manifest.ok !== true || !Array.isArray(manifest.files) || !manifest.files.length) return null
  const files = []
  for (const f of manifest.files) {
    if (!f || typeof f.path !== 'string' || !DIGEST.test(f.sha256 ?? '') || f.ok !== true) return null
    files.push({ path: f.path, sha256: f.sha256 })
  }
  files.sort((a, b) => (a.path < b.path ? -1 : 1))
  return sha256(canonical({ files, link: manifest.link ? { path: manifest.link.path, target: manifest.link.target } : null }))
}

const isStr = (x) => typeof x === 'string' && x.length > 0
const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/
const isTime = (x) => isStr(x) && ISO.test(x) && Number.isFinite(Date.parse(x))
const pinOk = (p, file) => {
  if (!p || !isStr(p.path) || !DIGEST.test(p.sha256 ?? '')) return 'a pin must be {path, sha256 (64 hex)}'
  const f = file(p.path)
  if (!f) return `missing: ${p.path}`
  if (f.sha256 !== p.sha256) return `changed: ${p.path}`
  return null
}

/**
 * The growth view.
 * @param {string} text the ledger
 * @param {{file?: (path: string) => {sha256: string, text: string}|null, source?: string, approver?: string}} opts
 */
export function growth(text, { file = () => null, source = 'cp-ledger.jsonl', approver = DEFAULT_APPROVER } = {}) {
  const raw = String(text)
  const lines = raw.split('\n')
  const uncommitted = lines.at(-1) !== '' // the last line has no newline: never counts (any content)
  const committed = lines.slice(0, -1) // every line a newline terminated; the unterminated rest is dropped
  if (committed.length > MAX_RECORDS) return { state: 'unavailable', source, why: `more than ${MAX_RECORDS} records` }
  const notes = []
  const note = (line, why) => notes.push({ line, why })
  if (uncommitted && lines.at(-1).trim()) note(lines.length, 'the last line is not committed (no newline): ignored')
  const records = []
  for (let i = 0; i < committed.length; i++) {
    if (!committed[i].trim()) continue
    let e
    try { e = JSON.parse(committed[i]) } catch { return { state: 'unavailable', source, why: `corrupt record at line ${i + 1}` } }
    if (!e || typeof e !== 'object' || Array.isArray(e)) return { state: 'unavailable', source, why: `not a record at line ${i + 1}` }
    records.push({ line: i + 1, e })
  }
  // record_id identity: an exact replay collapses onto its first position; a conflicting reuse is a
  // contradictory history: the view fails closed rather than drop lines and recount (which could
  // revive an act rejected earlier).
  const firstById = new Map()
  const live = []
  for (const r of records) {
    const id = r.e.record_id
    if (!isStr(id)) { note(r.line, 'no record_id'); continue }
    const prior = firstById.get(id)
    if (!prior) { firstById.set(id, r); live.push(r); continue }
    if (canonical(prior.e) === canonical(r.e)) continue
    return { state: 'unavailable', source, why: `contradictory history: record_id ${id} (line ${prior.line}) is reused with different content at line ${r.line}; a later line may not rewrite an earlier one, so no growth is shown until the ledger is repaired` }
  }
  const base = { source, schema: SCHEMA, policy: { id: POLICY.policy_id, version: POLICY.policy_version, sha256: POLICY_SHA256 }, incompleteTail: uncommitted && Boolean(lines.at(-1).trim()), notes }

  // 1. the approval: the first valid one, binding policy, bootstrap and a decision receipt
  let approval = null
  for (let k = 0; k < live.length && !approval; k++) {
    const { line, e } = live[k]
    if (e.event !== 'policy_approved') continue
    const why = e.schema !== SCHEMA ? 'unsupported schema'
      : e.policy?.id !== POLICY.policy_id || e.policy?.version !== POLICY.policy_version || e.policy?.sha256 !== POLICY_SHA256 ? 'approves other policy terms'
      : !isStr(e.rth_id) ? 'no rth_id'
      : e.approved_by !== approver ? `not approved by ${approver}`
      : !isTime(e.at) ? 'no valid time'
      : !DECISION.test(e.decision?.path ?? '') ? 'the decision receipt must be a cp/decisions/*.md file'
      : pinOk(e.decision, file)
      ?? (!Array.isArray(e.bootstrap?.record_ids) || !DIGEST.test(e.bootstrap?.sha256 ?? '') ? 'the approval must name the bootstrap record ids and their sha256' : null)
    if (why) { note(line, `approval not valid: ${why}`); continue }
    const before = new Map(live.slice(0, k).map((r) => [r.e.record_id, r.e]))
    const boot = e.bootstrap.record_ids.map((id) => before.get(id))
    if (boot.some((x) => !x)) { note(line, 'approval names bootstrap records that do not precede it'); continue }
    if (sha256(canonical(boot)) !== e.bootstrap.sha256) { note(line, 'the bootstrap content differs from what was approved'); continue }
    approval = { index: k, line, e, bootIds: new Set(e.bootstrap.record_ids) }
  }
  if (!approval) return { ...base, state: 'unapproved', drafts: live.length }

  // 2. one pass in append order: the bootstrap (at the approval's position), then what follows
  const rth = approval.e.rth_id
  const baseCheck = (e) => (e.schema !== SCHEMA ? 'unsupported schema'
    : e.rth_id !== rth ? 'another RTH identity'
    : e.policy?.id !== POLICY.policy_id || e.policy?.version !== POLICY.policy_version ? 'another policy'
    : !isTime(e.at) ? 'no valid time'
    : !Array.isArray(e.evidence) || !e.evidence.length ? 'no evidence'
    : e.evidence.map((p) => pinOk(p, file)).find(Boolean) ?? null)
  const order = [...live.slice(0, approval.index).filter((r) => approval.bootIds.has(r.e.record_id)), ...live.slice(approval.index + 1)]
  for (const r of live.slice(0, approval.index)) if (!approval.bootIds.has(r.e.record_id)) note(r.line, 'before the approval and not in the approved bootstrap: never counts')
  const awards = new Map() // record_id -> award (counted)
  const byActivation = new Map()
  const byBuild = new Map()
  let cp = POLICY.start_cp
  let stage = STAGES[0][0]
  const evolutions = []
  for (const { line, e } of order) {
    if (e.event === 'policy_approved') { note(line, 'a later approval changes nothing: the first valid approval binds'); continue }
    const bad = baseCheck(e)
    if (bad) { note(line, `${e.event ?? 'record'} not counted: ${bad}`); continue }
    if (e.event === 'activation_earned') {
      if (e.cp_delta !== POLICY.cp_per_activation || !isStr(e.activation_id)) { note(line, 'an award is exactly +10 for one activation_id'); continue }
      const mBad = pinOk(e.manifest, file)
      if (mBad) { note(line, `manifest ${mBad}`); continue }
      let manifest = null
      try { manifest = JSON.parse(file(e.manifest.path).text) } catch {}
      const build = buildIdOf(manifest)
      if (!build || build !== e.build_id) { note(line, 'the manifest is not a valid typed manifest, or its build_id does not match the record'); continue }
      const prior = byActivation.get(e.activation_id)
      if (prior) {
        // a second, different claim on one activation: fail closed from here (nothing earlier is revived or revised)
        if (!prior.void && !prior.rejected) cp -= POLICY.cp_per_activation
        prior.void = true
        note(line, `conflicting claims on activation ${e.activation_id}: it earns nothing from this point`)
        continue
      }
      if (byBuild.has(build)) { note(line, 'the same build (artifacts) already earned: a reinstall earns nothing'); continue }
      const a = { e, line, void: false }
      byActivation.set(e.activation_id, a)
      byBuild.set(build, a)
      awards.set(e.record_id, a)
      cp += POLICY.cp_per_activation
    } else if (e.event === 'award_rejected') {
      const a = awards.get(e.target_record_id)
      if (!a || a.void || a.rejected) { note(line, 'the rejection names no counted earlier award'); continue }
      a.rejected = true
      cp -= POLICY.cp_per_activation
      if (stageIndex(stage) > 0 && cp < STAGES[stageIndex(stage)][1]) note(line, `this correction leaves ${stage} below its threshold (${cp} CP); the stage chosen earlier stays (nothing decays)`)
    } else if (e.event === 'activation_rolled_back') {
      note(line, 'rollback recorded: CP does not change (a rollback never takes points back)')
    } else if (e.event === 'evolution') {
      const to = stageIndex(e.to_stage)
      if (to !== stageIndex(stage) + 1) { note(line, `evolution must go to the next stage after ${stage}`); continue }
      if (cp < STAGES[to][1]) { note(line, `${e.to_stage} was not available at this point (${cp} CP of ${STAGES[to][1]}): rejected, and not revived later`); continue }
      stage = e.to_stage
      evolutions.push({ at: e.at, to_stage: e.to_stage, record_id: e.record_id, line })
    } else note(line, `unknown event ${e.event}`)
  }
  const counted = [...awards.values()].filter((a) => !a.void && !a.rejected)
  const level = levelOf(cp)
  const colours = coloursOf(level)
  const available = availableStageOf(cp)
  const next = STAGES.find(([, t]) => t > cp) ?? null
  return {
    ...base,
    state: 'approved',
    rth_id: rth,
    approved_by: approval.e.approved_by,
    cp,
    level,
    grains: cp * POLICY.grains_per_cp,
    colours,
    palette: PALETTE.slice(0, colours),
    stage,
    availableStage: stageIndex(available) > stageIndex(stage) ? available : null,
    nextLevelIn: POLICY.cp_per_activation - ((cp - POLICY.start_cp) % POLICY.cp_per_activation),
    nextColourAtLevel: colours < PALETTE.length ? colours * POLICY.colour_every_levels + 1 : null,
    nextStage: next ? { name: next[0], cp: next[1] } : null,
    entries: counted.map((a) => ({ at: a.e.at, build: a.e.build ?? a.e.activation_id, activation_id: a.e.activation_id, cp: a.e.cp_delta, evidence: a.e.evidence.map((x) => x.path) })),
    evolutions,
  }
}

/** Read the ledger: absent = unapproved; any other read failure = unavailable (never a fresh start). */
export function readGrowth(path = DEFAULT_LEDGER, { repo = resolve(dirname(path), '../..'), approver = DEFAULT_APPROVER } = {}) {
  const source = relative(repo, path) || path
  let text
  try {
    const st = statSync(path)
    if (!st.isFile()) return { state: 'unavailable', source, why: 'not a file' }
    if (st.size > MAX_BYTES) return { state: 'unavailable', source, why: `larger than ${MAX_BYTES} bytes` }
    text = readFileSync(path, 'utf8')
  } catch (e) {
    if (e.code === 'ENOENT') return { state: 'unapproved', source, drafts: 0, why: 'no approved ledger yet' }
    return { state: 'unavailable', source, why: `cannot be read (${e.code ?? e.message})` }
  }
  const cache = new Map()
  const file = (p) => {
    if (cache.has(p)) return cache.get(p)
    let v = null
    const abs = resolve(repo, p)
    if (!isAbsolute(p) && !relative(repo, abs).startsWith('..')) {
      try {
        const st = statSync(abs)
        if (st.isFile() && st.size <= 20_000_000) {
          const buf = readFileSync(abs)
          v = { sha256: createHash('sha256').update(buf).digest('hex'), text: buf.length <= 1_000_000 ? buf.toString('utf8') : '' }
        }
      } catch {}
    }
    cache.set(p, v)
    return v
  }
  return growth(text, { file, source, approver })
}
