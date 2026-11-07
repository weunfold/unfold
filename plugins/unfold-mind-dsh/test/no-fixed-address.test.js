// Mind opens no connection of its own: agent events reach it through unfold-events' window, and the
// host journal path through UNFOLD_HOST_JOURNAL. So no shipped file names a port, an address or a
// network module, and the only environment it reads is that one variable (and DSH_HOME).
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { test } from 'node:test'

const root = new URL('../', import.meta.url)
const walk = (dir) => readdirSync(new URL(dir, root), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(`${dir}${e.name}/`) : [`${dir}${e.name}`]))
const shipped = () => ['index.js', 'package.json', ...walk('src/'), ...(existsSync(new URL('client.js', root)) ? ['client.js'] : [])]
const read = (file) => readFileSync(new URL(file, root), 'utf8')

test('no shipped file names a fixed port or a default address', () => {
  // The two fixed ports the first Unfold plugins used (agentd observer, host owner), spelled so
  // that a scan for them finds only shipped code.
  for (const file of shipped()) {
    const src = read(file)
    for (const port of [77_00, 77_81]) assert.doesNotMatch(src, new RegExp(`\\b${port}\\b`), file)
    assert.doesNotMatch(src, /127\.0\.0\.1|localhost|\[::1\]|0\.0\.0\.0/, file)
    assert.doesNotMatch(src, /node:net|node:http\b|node:dgram|listen\(|createServer\(/, file)
  }
})

test('the server half reads only UNFOLD_HOST_JOURNAL and DSH_HOME from the environment', () => {
  const names = new Set()
  for (const file of ['index.js', ...walk('src/').filter((f) => !f.startsWith('src/client/'))]) {
    for (const m of read(file).matchAll(/process\.env\.([A-Z_]+)|env\.([A-Z_]+)|\b(UNFOLD_[A-Z_]+)\b/g)) names.add(m[1] ?? m[2] ?? m[3])
  }
  assert.deepEqual([...names].sort(), ['DSH_HOME', 'UNFOLD_HOST_JOURNAL'])
})
