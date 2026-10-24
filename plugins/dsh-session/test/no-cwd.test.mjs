// No cwd fallback: the root Session's workspace is config.workspaceDir, else
// $DSH_HOME/rth-workspace; with neither, it is refused with a clear error (nothing is written into
// the working directory, which a Finder launch makes `/`).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { workspaceDirFor } from '../index.mjs'

test('config.workspaceDir wins, then $DSH_HOME/rth-workspace', () => {
  assert.equal(workspaceDirFor({ workspaceDir: '/x/ws' }, { DSH_HOME: '/h' }), '/x/ws')
  assert.equal(workspaceDirFor({}, { DSH_HOME: '/h' }), '/h/rth-workspace')
})
test('neither: refused with a clear error, never the working directory', () => {
  for (const env of [{}, { DSH_HOME: '' }]) assert.throws(() => workspaceDirFor({}, env), /DSH_HOME is not set.*workspaceDir/)
})
