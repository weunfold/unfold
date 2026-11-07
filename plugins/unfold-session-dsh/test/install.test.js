import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { bundlePatchPaths, loadOverlayPatches, composeEntries } from '@deepseek-ai/dsh-app-boot'

const root = join(import.meta.dirname, '..')
const manifest = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))

test('DSH can install the shipped bundle and compose its stable host plugin row', () => {
  assert.ok(manifest.dsh?.bundle, 'installBundle requires a dsh.bundle declaration')
  const files = bundlePatchPaths(root, manifest.dsh.bundle)
  assert.equal(files.length, 1)
  for (const file of files) assert.ok(manifest.files.includes(file.slice(root.length + 1)), 'the install patch is shipped')
  const patches = files.flatMap((file) => loadOverlayPatches('dsh', file))
  const existing = { id: 'existing-bot', name: '@example/companion', config: { kept: true } }
  const entries = composeEntries([[{ insert: [existing] }], patches])
  assert.deepEqual(entries.find((e) => e.id === existing.id), existing, 'installation preserves existing plugins')
  const ours = entries.filter((e) => e.name === manifest.name)
  assert.equal(ours.length, 1)
  assert.equal(ours[0].id, 'unfold-session')
})
