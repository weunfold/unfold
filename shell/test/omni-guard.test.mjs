// The Bot's home and seed guard (the omni-ui insert in shell/dsh-home-bot/profiles/web/cordis.patch.yml):
// OMNI_HOME must be an absolute, canonical path under the admitted RTH state root's instance
// directory (<RTH_DATA_DIR or ~/.local/share/rth>/instances/<id>/...); the seed must be there too,
// or the packaged read-only seed of this tree (RTH_DSH_ROOT); ~/.omni and any live Omni team stay
// refused; the dev layout (under this tree's shell/) needs the explicit RTH_OMNI_DEV_LAYOUT=1.
// The profile's own !!js expressions are evaluated exactly as the Cordis loader does
// (`with (ctx) eval(expr)`), against a controlled environment. Temporary directories only.
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import yaml from 'js-yaml'

const SHELL = join(dirname(fileURLToPath(import.meta.url)), '..')
const ROOT = join(SHELL, '..')
const PROFILE = join(SHELL, 'dsh-home-bot/profiles/web/cordis.patch.yml')
const JS = new yaml.Type('tag:yaml.org,2002:js', { kind: 'scalar', construct: (src) => ({ js: src }) })
const omni = yaml.load(readFileSync(PROFILE, 'utf8'), { schema: yaml.DEFAULT_SCHEMA.extend([JS]) }).flatMap((i) => i.insert ?? [i]).find((e) => e.id === 'omni-ui').config
const evaluate = new Function('ctx', 'expr', 'with (ctx) { return eval(expr) }')
/** Evaluate one of the profile's expressions with exactly this environment. */
function run(key, env) {
  const saved = process.env
  process.env = { PATH: saved.PATH, HOME: env.HOME ?? saved.HOME, ...env }
  try { return { value: evaluate({}, omni[key].js) } } catch (e) { return { error: e.message } } finally { process.env = saved }
}
const T = realpathSync(mkdtempSync(join(tmpdir(), 'rth-omni-guard-')))
test.after(() => rmSync(T, { recursive: true, force: true }))
const STATE = join(T, 'state')
const FAKE_HOME = join(T, 'home')
mkdirSync(join(STATE, 'instances/inst-1'), { recursive: true })
mkdirSync(join(FAKE_HOME, '.omni/ui'), { recursive: true })
const base = { RTH_DATA_DIR: STATE, HOME: FAKE_HOME, RTH_DSH_ROOT: ROOT }
const home = (OMNI_HOME, extra = {}) => run('home', { ...base, OMNI_HOME, ...extra })
const seed = (RTH_OMNI_TEAM_SEED, extra = {}) => run('importHome', { ...base, RTH_OMNI_TEAM_SEED, ...extra })

test('admitted: OMNI_HOME under <state root>/instances/<id>/ gives its ui dir', () => {
  assert.deepEqual(home(join(STATE, 'instances/inst-1/omni-home')), { value: join(STATE, 'instances/inst-1/omni-home/ui') })
})

test('admitted: the default state root ~/.local/share/rth when RTH_DATA_DIR is unset', () => {
  // os.homedir() reads the process's real HOME, so this case uses it (the path need not exist; nothing is written).
  const want = join(homedir(), '.local/share/rth/instances/rth-omni-guard-test/omni-home')
  const got = run('home', { RTH_DSH_ROOT: ROOT, OMNI_HOME: want })
  assert.ok(got.value?.endsWith('/.local/share/rth/instances/rth-omni-guard-test/omni-home/ui'), JSON.stringify(got))
})

test('refused: empty, relative, traversal, outside an instance dir, and a path that merely contains /shell/', () => {
  for (const bad of ['', 'omni-home', join(STATE, 'instances/inst-1/../../../evil'), join(STATE, 'omni-home'), join(STATE, 'instances'), join(STATE, 'instances/inst-1'), join(T, 'evil/shell/omni-home'), '/tmp/x/shell/y']) {
    assert.match(home(bad).error ?? '', /rth refuses to start the Bot/, JSON.stringify(bad))
  }
})

test('refused: ~/.omni and a live team reached through a symlink (canonical paths, not text)', () => {
  assert.match(home(join(FAKE_HOME, '.omni')).error ?? '', /rth refuses/)
  symlinkSync(join(FAKE_HOME, '.omni'), join(STATE, 'instances/inst-1/linked-home'))
  assert.match(home(join(STATE, 'instances/inst-1/linked-home')).error ?? '', /rth refuses/, 'a link into ~/.omni is refused')
})

test('the dev layout needs the explicit allowance', () => {
  const dev = join(SHELL, 'dsh-home-bot/omni-home')
  assert.match(home(dev).error ?? '', /rth refuses/)
  assert.deepEqual(home(dev, { RTH_OMNI_DEV_LAYOUT: '1' }), { value: join(realpathSync(SHELL), 'dsh-home-bot/omni-home/ui') })
  assert.match(home(join(T, 'elsewhere/omni-home'), { RTH_OMNI_DEV_LAYOUT: '1' }).error ?? '', /rth refuses/, 'the allowance is this tree only')
})

test('seed: the packaged read-only seed of this tree, or one under the instance dir; never ~/.omni or a lookalike', () => {
  const packaged = join(ROOT, 'shell/dsh-home-bot/omni-team-seed')
  assert.deepEqual(seed(packaged), { value: realpathSync(packaged) })
  // An instance seed belongs to the admitted home's instance, so the home is given alongside.
  assert.deepEqual(seed(join(STATE, 'instances/inst-1/seed'), { OMNI_HOME: join(STATE, 'instances/inst-1/omni-home') }), { value: join(STATE, 'instances/inst-1/seed') })
  for (const bad of ['', join(FAKE_HOME, '.omni/ui'), join(T, 'evil/shell/dsh-home-bot/omni-team-seed'), 'shell/dsh-home-bot/omni-team-seed']) {
    assert.match(seed(bad).error ?? '', /rth refuses to seed the Bot team/, JSON.stringify(bad))
  }
})

// ---- The FINAL Bot home, /ui included, is what is validated; dangling links, unsupported
// components and inspection errors are refusals, never missing tails.
import { lstatSync, readdirSync, statSync, writeFileSync } from 'node:fs'
/** A foreign fixture's names, kinds and modes: unchanged after a refusal (nothing created, copied or chmodded). */
const snapshot = (dir) => { const out = []; const walk = (p) => { for (const n of readdirSync(p).sort()) { const q = join(p, n); const st = lstatSync(q); out.push(`${q.slice(dir.length)} ${st.mode.toString(8)}`); if (st.isDirectory()) walk(q) } }; walk(dir); return out.join('\n') }
const OUTSIDE = join(T, 'outside')
mkdirSync(join(OUTSIDE, 'ui'), { recursive: true })
writeFileSync(join(FAKE_HOME, '.omni/ui/state.json'), '{"FICTIONAL":"live team"}')

test('refused: a /ui inside an admitted home that links to ~/.omni/ui', () => {
  const h = join(STATE, 'instances/r1a/omni-home')
  mkdirSync(h, { recursive: true })
  symlinkSync(join(FAKE_HOME, '.omni/ui'), join(h, 'ui'))
  const before = snapshot(join(FAKE_HOME, '.omni'))
  assert.match(home(h).error ?? '', /rth refuses to start the Bot/, 'the final home is ~/.omni/ui')
  assert.equal(snapshot(join(FAKE_HOME, '.omni')), before)
})

test('refused: a /ui that links outside the state root', () => {
  const h = join(STATE, 'instances/r1b/omni-home')
  mkdirSync(h, { recursive: true })
  symlinkSync(join(OUTSIDE, 'ui'), join(h, 'ui'))
  const before = snapshot(OUTSIDE)
  assert.match(home(h).error ?? '', /rth refuses to start the Bot/)
  assert.equal(snapshot(OUTSIDE), before)
})

test('refused: dangling links and unsupported components are not missing tails', () => {
  mkdirSync(join(STATE, 'instances/r1c'), { recursive: true })
  symlinkSync(join(T, 'nowhere/omni-home'), join(STATE, 'instances/r1c/omni-home')) // a dangling OMNI_HOME
  assert.match(home(join(STATE, 'instances/r1c/omni-home')).error ?? '', /rth refuses to start the Bot/, 'dangling OMNI_HOME')
  mkdirSync(join(STATE, 'instances/r1d/omni-home'), { recursive: true })
  symlinkSync(join(T, 'nowhere/ui'), join(STATE, 'instances/r1d/omni-home/ui')) // a dangling /ui
  assert.match(home(join(STATE, 'instances/r1d/omni-home')).error ?? '', /rth refuses to start the Bot/, 'dangling /ui')
  writeFileSync(join(STATE, 'instances/r1e'), 'a file, not a directory')
  assert.match(home(join(STATE, 'instances/r1e/omni-home')).error ?? '', /rth refuses to start the Bot/, 'a file as a path component')
  assert.ok(!statSync(join(T, 'nowhere'), { throwIfNoEntry: false }), 'nothing created through a dangling link')
})

test('admitted: an absent home, and an existing canonical one with a real ui dir', () => {
  assert.deepEqual(home(join(STATE, 'instances/r1f/omni-home')), { value: join(STATE, 'instances/r1f/omni-home/ui') })
  mkdirSync(join(STATE, 'instances/r1g/omni-home/ui'), { recursive: true })
  assert.deepEqual(home(join(STATE, 'instances/r1g/omni-home')), { value: join(STATE, 'instances/r1g/omni-home/ui') })
})

// ---- An instance-scoped seed belongs to the SAME instance as the admitted home.
const both = (OMNI_HOME, RTH_OMNI_TEAM_SEED) => ({ home: home(OMNI_HOME), seed: seed(RTH_OMNI_TEAM_SEED, { OMNI_HOME }) })

test('refused: instance A\'s home with instance B\'s seed, directly or through a link', () => {
  const aHome = join(STATE, 'instances/a/omni-home')
  mkdirSync(join(STATE, 'instances/b/seed'), { recursive: true })
  writeFileSync(join(STATE, 'instances/b/seed/state.json'), '{"FICTIONAL":"team B"}')
  const before = snapshot(join(STATE, 'instances/b'))
  const direct = both(aHome, join(STATE, 'instances/b/seed'))
  assert.ok(direct.home.value, 'the home itself is admitted')
  assert.match(direct.seed.error ?? '', /rth refuses to seed the Bot team/, 'B\'s seed for A')
  mkdirSync(join(STATE, 'instances/a'), { recursive: true })
  symlinkSync(join(STATE, 'instances/b/seed'), join(STATE, 'instances/a/seed-link'))
  assert.match(seed(join(STATE, 'instances/a/seed-link'), { OMNI_HOME: aHome }).error ?? '', /rth refuses to seed the Bot team/, 'B\'s seed through a link in A')
  assert.equal(snapshot(join(STATE, 'instances/b')), before)
})

test('admitted: the same instance\'s seed, and the packaged seed, with the home evaluated alongside', () => {
  const aHome = join(STATE, 'instances/a2/omni-home')
  assert.deepEqual(both(aHome, join(STATE, 'instances/a2/seed')).seed, { value: join(STATE, 'instances/a2/seed') })
  const packaged = join(ROOT, 'shell/dsh-home-bot/omni-team-seed')
  assert.deepEqual(both(aHome, packaged).seed, { value: realpathSync(packaged) })
  assert.match(seed(join(STATE, 'instances/a2/seed')).error ?? '', /rth refuses to seed the Bot team/, 'an instance seed needs an admitted home to belong to')
})
