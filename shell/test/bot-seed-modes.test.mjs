// The Bot's team seed copy (the vendored omni-ui-dsh importTeam): a packaged seed is read-only (a
// signed bundle, a read-only image), and cpSync keeps the source modes, so the copied home was
// read-only too (EACCES on the team's lock and inflight files). The copy must be owner-writable,
// files and directories, while the seed itself is left as it was. Temporary directories only.
import assert from 'node:assert/strict'
import { chmodSync, cpSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { apply } from '../vendor/omni-ui-dsh/index.js'
import { fakeContext } from '../vendor/omni-ui-dsh/test/fake-ctx.js'

const SHELL = join(new URL('.', import.meta.url).pathname, '..')
const walk = (d) => [d, ...readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]))]
const chmodTree = (d, mode, dirMode) => { for (const p of walk(d).reverse()) chmodSync(p, statSync(p).isDirectory() ? dirMode : mode) }

test('a read-only seed is copied into an owner-writable home, and the seed stays read-only', async (t) => {
  const root = mkdtempSync(join(tmpdir(), 'rth-bot-seed-'))
  const seed = join(root, 'seed')
  const home = join(root, 'home/ui')
  cpSync(join(SHELL, 'dsh-home-bot/omni-team-seed'), seed, { recursive: true, filter: (s) => !s.includes('/logs') })
  chmodTree(seed, 0o444, 0o555)
  t.after(() => { chmodTree(seed, 0o644, 0o755); rmSync(root, { recursive: true, force: true }) })
  const ctx = fakeContext()
  t.after(async () => { await ctx.dispose?.() })
  apply(ctx, { home, importHome: seed, updateCheck: false })
  const copied = walk(home)
  assert.ok(copied.some((p) => p.endsWith('state.json')), 'the team was imported')
  for (const p of copied) assert.ok(statSync(p).mode & 0o200, `owner-writable: ${p} (${(statSync(p).mode & 0o777).toString(8)})`)
  writeFileSync(join(home, 'inflight.json'), '{"turns":{},"queued":{}}') // the Bot rewrites its own files
  for (const p of walk(seed)) assert.equal(statSync(p).mode & 0o222, 0, `the seed is untouched: ${p}`)
})
