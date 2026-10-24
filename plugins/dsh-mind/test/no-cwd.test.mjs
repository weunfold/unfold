// No cwd fallback: Mind's data (its suggestion log) is config.dataDir, else
// $DSH_HOME/rth-mind; with neither, the plugin refuses with a clear error before anything is made.
import assert from 'node:assert/strict'
import { mkdtempSync, readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { apply, dataDirFor } from '../index.mjs'

test('config.dataDir wins, then $DSH_HOME/rth-mind', () => {
  assert.equal(dataDirFor({ dataDir: '/x/m' }, { DSH_HOME: '/h' }), '/x/m')
  assert.equal(dataDirFor({}, { DSH_HOME: '/h' }), '/h/rth-mind')
})
test('neither: refused with a clear error, never the working directory', () => {
  for (const env of [{}, { DSH_HOME: '' }]) assert.throws(() => dataDirFor({}, env), /DSH_HOME is not set.*dataDir/)
})
test('the plugin itself refuses at apply, with nothing written to the working directory', (t) => {
  const cwd = mkdtempSync(join(tmpdir(), 'rth-mind-cwd-'))
  const was = process.cwd()
  const saved = process.env.DSH_HOME
  t.after(() => { process.chdir(was); if (saved === undefined) delete process.env.DSH_HOME; else process.env.DSH_HOME = saved; rmSync(cwd, { recursive: true, force: true }) })
  process.chdir(cwd)
  delete process.env.DSH_HOME
  assert.throws(() => apply({}, {}), /DSH_HOME is not set/)
  assert.deepEqual(readdirSync(cwd), [])
})
