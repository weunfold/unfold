// The cosmic points ledger (src/cp.mjs): a committed-line ledger read in one ordered pass; an approval
// bound to the full policy, the exact bootstrap and the approver's decision receipt; record_id identity;
// awards tied to a pinned typed manifest and its build identity; corrections validated like any record;
// evolution judged at its own position.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { DEFAULT_APPROVER, DEFAULT_LEDGER, POLICY, POLICY_SHA256, SCHEMA, buildIdOf, canonical, growth, readGrowth } from '../src/cp.mjs'

const sha = (s) => createHash('sha256').update(s).digest('hex')
const FILES = new Map()
const put = (path, text) => { FILES.set(path, { text, sha256: sha(text) }); return { path, sha256: sha(text) } }
const file = (p) => FILES.get(p) ?? null
const EV = [put('cp/evidence/x.md', 'evidence x')]
const EVY = [put('cp/evidence/y.md', 'evidence y')]
const DECISION = put('cp/decisions/cp-approval.md', 'The owner approves the CP policy and the bootstrap.')
const AT = '2026-10-22T00:00:00Z'
const POL = { id: POLICY.policy_id, version: POLICY.policy_version }

const manifestFor = (tag, extra = {}) => ({ ok: true, files: [{ path: 'plugins/dsh-events/lib/client.js', sha256: sha(`events ${tag}`), ok: true }, { path: 'plugins/dsh-mind/lib/client.js', sha256: sha(`mind ${tag}`), ok: true }], link: null, ...extra })
let n = 0
/** A valid award for a distinct build (or the build `tag`), its manifest pinned at its own path. */
const award = (o = {}, { tag, path } = {}) => {
  n++
  const m = manifestFor(tag ?? `build-${n}`)
  const pin = put(path ?? `cp/evidence/build-${n}/post.json`, JSON.stringify(m))
  return { schema: SCHEMA, record_id: `a${n}`, event: 'activation_earned', rth_id: 'rth-1', policy: POL, activation_id: `act-${n}`, build_id: buildIdOf(m), manifest: pin, cp_delta: 10, at: AT, evidence: EV, ...o }
}
const approve = (boot = [], o = {}) => ({ schema: SCHEMA, record_id: 'p1', event: 'policy_approved', rth_id: 'rth-1', policy: { ...POL, sha256: POLICY_SHA256 }, approved_by: 'owner', at: AT, decision: DECISION, bootstrap: { record_ids: boot.map((r) => r.record_id), sha256: sha(canonical(boot)) }, ...o })
const evo = (to, o = {}) => ({ schema: SCHEMA, record_id: `e-${to}-${++n}`, event: 'evolution', rth_id: 'rth-1', policy: POL, to_stage: to, at: AT, evidence: EV, ...o })
const reject = (target, o = {}) => ({ schema: SCHEMA, record_id: `r-${++n}`, event: 'award_rejected', rth_id: 'rth-1', policy: POL, target_record_id: target.record_id, at: AT, evidence: EV, ...o })
const L = (...recs) => recs.map((r) => JSON.stringify(r)).join('\n') + '\n'
const g = (...recs) => growth(L(...recs), { file })

test('unapproved: awards with no approval count nothing; a blank approver is a draft; an absent ledger serves "unapproved"', () => {
  assert.equal(g(award(), award()).state, 'unapproved')
  assert.equal(g(approve([], { approved_by: '' }), award()).state, 'unapproved', 'approved_by blank is a draft')
  const absent = readGrowth(join(mkdtempSync(join(tmpdir(), 'cp-absent-')), 'cp-ledger.jsonl'))
  assert.equal(absent.state, 'unapproved', 'no counted ledger: unapproved')
  assert.equal(absent.cp, undefined, 'no CP number is served while unapproved')
})

test('the approver is configurable: an approval by anyone else is not valid', () => {
  assert.equal(DEFAULT_APPROVER, 'owner')
  assert.equal(g(approve()).approved_by, 'owner')
  const other = g(approve([], { approved_by: 'someone-else' }))
  assert.equal(other.state, 'unapproved')
  assert.ok(other.notes.some((x) => /not approved by owner/.test(x.why)))
  const v = growth(L(approve([], { approved_by: 'maintainer' })), { file, approver: 'maintainer' })
  assert.deepEqual([v.state, v.approved_by], ['approved', 'maintainer'])
  assert.equal(DEFAULT_LEDGER.endsWith('cp/ledger/cp-ledger.jsonl'), true)
})
test('approved and empty: the 100 CP start, level 1, one colour, Seed', () => {
  const v = g(approve())
  assert.deepEqual([v.state, v.cp, v.level, v.colours, v.stage, v.availableStage, v.grains], ['approved', 100, 1, 1, 'Seed', null, 10_000])
})

test('the approval binds the full policy (rules included), the decision receipt and the exact bootstrap', () => {
  const changedRules = { ...POLICY, rules: { ...POLICY.rules, dedup: 'anything' } }
  assert.notEqual(sha(canonical(changedRules)), POLICY_SHA256, 'the hashed policy covers the eligibility/dedup/correction/evolution rules')
  assert.equal(g(approve([], { policy: { ...POL, sha256: 'f'.repeat(64) } })).state, 'unapproved', 'other policy terms')
  assert.equal(g(approve([], { decision: undefined })).state, 'unapproved', 'no decision receipt')
  assert.equal(g(approve([], { decision: { ...EV[0] } })).state, 'unapproved', 'the receipt must be a cp/decisions/*.md file')
  assert.equal(g(approve([], { decision: { ...DECISION, sha256: '0'.repeat(64) } })).state, 'unapproved', 'a changed receipt')
  assert.equal(g(approve([], { bootstrap: undefined })).state, 'unapproved', 'no bootstrap binding')
  const a1 = award()
  const a2 = award()
  assert.equal(g(a1, a2, approve([a1, a2])).cp, 120, 'the approved bootstrap counts')
  assert.equal(g(a1, a2, approve([a1])).cp, 110, 'a record before the approval outside the bootstrap never counts')
  const altered = { ...a1, evidence: EVY }
  const v = g(altered, a2, approve([a1, a2]))
  assert.equal(v.state, 'unapproved', 'an altered bootstrap award voids the approval')
  assert.ok(v.notes.some((x) => /bootstrap content differs/.test(x.why)))
  assert.equal(g(approve([a1]), a1).state, 'unapproved', 'bootstrap records must precede the approval')
})

// ---- committed lines, record identity, manifests ----

test('a fully valid award without its final newline is not committed: 100 CP until the delimiter arrives', () => {
  const a = award()
  const text = L(approve()) + JSON.stringify(a)
  const v = growth(text, { file })
  assert.deepEqual([v.state, v.cp, v.incompleteTail], ['approved', 100, true])
  assert.equal(growth(text + '\n', { file }).cp, 110, 'committed once the newline lands')
  assert.equal(growth(JSON.stringify(approve()), { file }).state, 'unapproved', 'an unterminated approval does not approve')
  const ten = Array.from({ length: 10 }, () => award())
  assert.equal(growth(L(approve(), ...ten) + JSON.stringify(evo('Mote')), { file }).stage, 'Seed', 'an unterminated evolution does not evolve')
})

test('the same record_id with changed nested evidence (both resolvable) is a conflict: fail closed', () => {
  const a = award()
  const v = g(approve(), a, { ...a, evidence: EVY })
  assert.equal(v.state, 'unavailable', 'contradictory history: no CP at all')
  assert.equal(v.cp, undefined)
  assert.match(v.why, /record_id a\d+ \(line 2\) is reused with different content at line 3/)
  assert.notEqual(canonical({ e: [{ path: 'x', sha256: 'a' }] }), canonical({ e: [{ path: 'y', sha256: 'b' }] }), 'nested keys are compared')
  assert.equal(canonical({ b: { d: 1, c: [{ y: 1, x: 2 }] }, a: 0 }), '{"a":0,"b":{"c":[{"x":2,"y":1}],"d":1}}')
  assert.equal(g(approve(), a, a, a).cp, 110, 'an exact replay is idempotent')
})

test('an invalid correction (wrong schema, policy version 999, no record_id) has no effect and is noted', () => {
  const a = award()
  for (const bad of [{ schema: 'rth.cp-ledger/v0' }, { policy: { ...POL, version: 999 } }, { record_id: undefined }, { at: 'yesterday' }, { evidence: [] }, { rth_id: 'rth-other' }]) {
    const v = g(approve(), a, reject(a, bad))
    assert.equal(v.cp, 110, JSON.stringify(bad))
    assert.ok(v.notes.length, 'the rejection reason is recorded')
  }
  assert.equal(g(approve(), a, reject(a)).cp, 100, 'a valid correction stops the award')
  assert.equal(g(approve(), reject(a), a).cp, 110, 'a correction names an earlier award; it does not pre-empt a later one')
})

test('two distinct activations sharing one record_id earn nothing (the view fails closed)', () => {
  const v = g(approve(), award({ record_id: 'same' }), award({ record_id: 'same' }))
  assert.deepEqual([v.state, v.cp], ['unavailable', undefined])
})

test('the manifest is a pinned typed resource whose computed build_id the award names', () => {
  const a = award()
  assert.equal(g(approve(), { ...a, build_id: 'b'.repeat(64) }).cp, 100, 'a changed build claim')
  assert.equal(g(approve(), { ...a, manifest: { ...a.manifest, sha256: '0'.repeat(64) } }).cp, 100, 'a manifest that changed since it was pinned')
  assert.equal(g(approve(), { ...a, manifest: 'post.json' }).cp, 100, 'an unchecked string is not a manifest')
  const untyped = put('cp/evidence/untyped/post.json', JSON.stringify({ ok: true, files: [{ path: 'x', sha256: 'aa', ok: true }] }))
  assert.equal(g(approve(), award({ manifest: untyped, build_id: null })).cp, 100, 'a manifest without 64-hex file digests is not typed')
  const failed = manifestFor('failed', { ok: false })
  assert.equal(g(approve(), award({ manifest: put('cp/evidence/failed/post.json', JSON.stringify(failed)), build_id: sha('x') })).cp, 100, 'a failed activation earns nothing')
  assert.equal(buildIdOf(failed), null)
})

test('a reinstall of identical artifacts with different activation metadata earns nothing; a new build does', () => {
  const first = award({}, { tag: 'same-artifacts' })
  const again = award({ at: '2026-11-01T00:00:00Z' }, { tag: 'same-artifacts', path: 'cp/evidence/reinstall/post.json' })
  assert.notEqual(first.activation_id, again.activation_id)
  assert.notEqual(first.manifest.path, again.manifest.path)
  assert.equal(first.build_id, again.build_id, 'the build identity is the artifact content, not the activation')
  const v = g(approve(), first, again)
  assert.equal(v.cp, 110)
  assert.ok(v.notes.some((x) => /reinstall earns nothing/.test(x.why)))
  assert.equal(g(approve(), first, award()).cp, 120)
})

test('conflicting claims on one activation earn nothing from that point; shapes are validated', () => {
  const a = award()
  assert.equal(g(approve(), a, award({ activation_id: a.activation_id })).cp, 100)
  for (const o of [{ at: 'not a time' }, { at: 'Oct 22 2026' }, { at: '2026-10-22' }, { at: '2026-13-45T99:00:00Z' }, { evidence: [{ path: 'cp/evidence/x.md', sha256: 'aa' }] }, { manifest: { path: 'bench/x', sha256: 'abc' } }, { cp_delta: 50 }, { rth_id: 'rth-other' }, { policy: { ...POL, version: 1 } }, { schema: 'rth.cp-ledger/v0' }, { evidence: [] }, { evidence: [{ path: 'cp/evidence/nope.md', sha256: '0'.repeat(64) }] }]) {
    assert.equal(g(approve(), award(o)).cp, 100, JSON.stringify(o))
  }
})

test('ten distinct eligible activations earn ten awards; ten copies of one earn one', () => {
  const v = g(approve(), ...Array.from({ length: 10 }, () => award()))
  assert.deepEqual([v.cp, v.level, v.colours, v.palette.at(-1), v.stage, v.availableStage], [200, 11, 2, 'magenta', 'Seed', 'Mote'])
  const one = award()
  assert.equal(g(approve(), ...Array.from({ length: 10 }, () => one)).cp, 110)
})

// ---- evolution is judged at its append position ----

test('an evolution appended at 100 CP stays rejected when ten awards follow: Seed with Mote available', () => {
  const ten = Array.from({ length: 10 }, () => award())
  const early = evo('Mote')
  const v = g(approve(), early, ...ten)
  assert.deepEqual([v.cp, v.stage, v.availableStage], [200, 'Seed', 'Mote'])
  assert.ok(v.notes.some((x) => /not available at this point \(100 CP of 200\)/.test(x.why)))
  const later = evo('Mote')
  const w = g(approve(), early, ...ten, later)
  assert.deepEqual([w.stage, w.availableStage, w.evolutions.length, w.evolutions[0].record_id], ['Mote', null, 1, later.record_id], 'a new explicit record after 200 CP chooses Mote')
  assert.equal(g(approve(), early, ...ten, early).stage, 'Seed', 'an exact replay of the early record is the same rejected act')
  assert.equal(g(approve(), ...ten, evo('Wisp')).stage, 'Seed', 'only the next stage')
})

test('reload is deterministic from disk; a correction after evolving is disclosed and nothing decays; a rollback changes nothing', () => {
  const ten = Array.from({ length: 10 }, () => award())
  const mote = evo('Mote')
  const rb = { schema: SCHEMA, record_id: `rb-${++n}`, event: 'activation_rolled_back', rth_id: 'rth-1', policy: POL, activation_id: ten[0].activation_id, at: AT, evidence: EV }
  const text = L(approve(), evo('Mote'), ...ten, mote, reject(ten[9]), rb)
  const dir = mkdtempSync(join(tmpdir(), 'cp-'))
  for (const [p, f] of FILES) { mkdirSync(dirname(join(dir, p)), { recursive: true }); writeFileSync(join(dir, p), f.text) }
  writeFileSync(join(dir, 'ledger.jsonl'), text)
  const a = readGrowth(join(dir, 'ledger.jsonl'), { repo: dir })
  const b = readGrowth(join(dir, 'ledger.jsonl'), { repo: dir })
  assert.deepEqual(a, b)
  assert.deepEqual([a.state, a.cp, a.stage, a.evolutions.length], ['approved', 190, 'Mote', 1])
  assert.ok(a.notes.some((x) => /leaves Mote below its threshold \(190 CP\).*nothing decays/.test(x.why)), 'the correction is disclosed')
  assert.ok(a.notes.some((x) => /rollback recorded: CP does not change/.test(x.why)))
})

test('malformed input and read errors are "unavailable", never a fresh start', () => {
  assert.equal(growth(L(approve()) + '{corrupt\n' + L(award()), { file }).state, 'unavailable')
  const dir = mkdtempSync(join(tmpdir(), 'cp-'))
  assert.equal(readGrowth(dir).state, 'unavailable', 'a directory is not an empty ledger')
  assert.equal(readGrowth(join(dir, 'absent.jsonl')).state, 'unapproved')
  const f = join(dir, 'ledger.jsonl')
  writeFileSync(f, 'x'.repeat(1_000_001))
  assert.equal(readGrowth(f).state, 'unavailable')
})

// ---- a later line never erases an earlier one ----

const tenAwards = () => Array.from({ length: 10 }, () => award())
const fourSteps = () => {
  const ten = tenAwards()
  const corr = reject(ten[8], { record_id: 'correction' })
  const early = evo('Mote', { record_id: 'early-evo' })
  const conflict = reject(ten[7], { record_id: 'correction' })
  return { ten, corr, early, conflict }
}

test("a reused correction id never revives the evolution rejected at 190 CP", () => {
  const { ten, corr, early, conflict } = fourSteps()
  const before = g(approve(), ...ten, corr, early)
  assert.deepEqual([before.state, before.cp, before.stage, before.evolutions.length], ['approved', 190, 'Seed', 0])
  assert.ok(before.notes.some((x) => /not available at this point \(190 CP of 200\)/.test(x.why)))
  const after = g(approve(), ...ten, corr, early, conflict)
  assert.equal(after.state, 'unavailable', 'contradictory history fails closed')
  assert.equal(after.stage, undefined, 'never Mote')
  assert.equal(after.evolutions, undefined)
  assert.match(after.why, /record_id correction/)
})

test('reload from disk gives the same fail-closed view; a resolved history needs a new explicit evolution', () => {
  const { ten, corr, early, conflict } = fourSteps()
  const dir = mkdtempSync(join(tmpdir(), 'cp-'))
  for (const [p, f] of FILES) { mkdirSync(dirname(join(dir, p)), { recursive: true }); writeFileSync(join(dir, p), f.text) }
  writeFileSync(join(dir, 'ledger.jsonl'), L(approve(), ...ten, corr, early, conflict))
  const a = readGrowth(join(dir, 'ledger.jsonl'), { repo: dir })
  assert.deepEqual(a, readGrowth(join(dir, 'ledger.jsonl'), { repo: dir }))
  assert.equal(a.state, 'unavailable')
  // resolved: the conflicting line is not in the ledger; the early evolution stays rejected; a later explicit one after a new award counts
  const more = award()
  const later = evo('Mote', { record_id: 'later-evo' })
  const v = g(approve(), ...ten, corr, early, more, later)
  assert.deepEqual([v.state, v.cp, v.stage, v.evolutions.map((x) => x.record_id)], ['approved', 200, 'Mote', ['later-evo']])
  assert.equal(g(approve(), ...ten, corr, early, more).stage, 'Seed', 'reaching 200 again does not revive early-evo')
})

test('conflicting ids for awards, corrections and evolutions all fail closed; exact replays stay idempotent', () => {
  const ten = tenAwards()
  const e1 = evo('Mote', { record_id: 'evo-x' })
  const cases = {
    award: [approve(), ...ten, award({ record_id: ten[0].record_id })],
    correction: [approve(), ...ten, reject(ten[0], { record_id: 'c' }), reject(ten[1], { record_id: 'c' })],
    evolution: [approve(), ...ten, e1, evo('Mote', { record_id: 'evo-x', at: '2026-10-23T00:00:00Z' })],
    'evolution id reused by an award': [approve(), ...ten, e1, award({ record_id: 'evo-x' })],
    'approval id reused': [approve(), ...ten, award({ record_id: 'p1' })],
  }
  for (const [k, recs] of Object.entries(cases)) assert.equal(g(...recs).state, 'unavailable', k)
  const r = reject(ten[0])
  const v = g(approve(), ...ten, r, r, e1, e1)
  assert.deepEqual([v.state, v.cp, v.stage], ['approved', 190, 'Seed'], 'exact replays: one correction; Mote unearned at 190 stays rejected')
  const ok = g(approve(), ...ten, e1, e1)
  assert.deepEqual([ok.cp, ok.stage, ok.evolutions.length], [200, 'Mote', 1])
  const rb = { schema: SCHEMA, record_id: 'rb-1', event: 'activation_rolled_back', rth_id: 'rth-1', policy: POL, activation_id: ten[0].activation_id, at: AT, evidence: EV }
  assert.equal(g(approve(), ...ten, rb, rb).cp, 200, 'a rollback (replayed) never takes points back')
})

test('guards: five known counterexamples stay repaired', () => {
  const a = award()
  assert.equal(growth(L(approve()) + JSON.stringify(award()), { file }).cp, 100, 'unterminated valid award')
  assert.equal(g(approve(), a, { ...a, evidence: EVY }).state, 'unavailable', 'changed nested evidence under one record_id')
  assert.equal(g(approve(), a, reject(a, { schema: 'bad', policy: { ...POL, version: 999 }, record_id: undefined })).cp, 110, 'invalid correction')
  assert.equal(g(approve(), award({ record_id: 'same' }), award({ record_id: 'same' })).state, 'unavailable', 'shared record_id')
  const v = g(approve(), evo('Mote'), ...tenAwards())
  assert.deepEqual([v.stage, v.cp, v.availableStage], ['Seed', 200, 'Mote'], 'early evolution')
})
