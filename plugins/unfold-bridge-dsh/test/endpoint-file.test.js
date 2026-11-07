import { test } from 'node:test'
import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { endpointFile, removeEndpoint, takeStateDir, writeEndpoint } from '../src/host/endpoint-file.js'

const KEY = 'test-key-not-a-secret-0123456789abcdef'
const temp = () => mkdtempSync(join(tmpdir(), 'unfold-bridge-'))
const mode = path => statSync(path).mode & 0o777

test('the file is unfold-bridge/endpoint.json in XDG_STATE_HOME, else ~/.local/state', () => {
  assert.equal(endpointFile({ XDG_STATE_HOME: '/s' }, '/home/x'), join('/s', 'unfold-bridge', 'endpoint.json'))
  assert.equal(endpointFile({}, '/home/x'), join('/home/x', '.local', 'state', 'unfold-bridge', 'endpoint.json'))
  // a relative XDG_STATE_HOME is invalid by the spec and ignored
  assert.equal(endpointFile({ XDG_STATE_HOME: 'rel' }, '/home/x'), join('/home/x', '.local', 'state', 'unfold-bridge', 'endpoint.json'))
})

test('Unfold names the folder in UNFOLD_BRIDGE_STATE_DIR: the file is <dir>/endpoint.json', () => {
  assert.equal(endpointFile({ XDG_STATE_HOME: '/s' }, '/home/x', '/u/instances/one/dsh/bridge'), join('/u/instances/one/dsh/bridge', 'endpoint.json'))
  assert.equal(endpointFile({}, '/home/x', null), join('/home/x', '.local', 'state', 'unfold-bridge', 'endpoint.json'), 'no folder named: the old rule')
})

test('UNFOLD_BRIDGE_STATE_DIR is read once and leaves the env; only an absolute one is used', () => {
  const env = { UNFOLD_BRIDGE_STATE_DIR: '/u/instances/one/dsh/bridge', XDG_STATE_HOME: '/s' }
  assert.equal(takeStateDir(env), '/u/instances/one/dsh/bridge')
  assert.ok(!Object.hasOwn(env, 'UNFOLD_BRIDGE_STATE_DIR'), 'gone from the env after the read')
  assert.equal(env.XDG_STATE_HOME, '/s', 'nothing else is touched')
  assert.equal(takeStateDir(env), undefined, 'a second read finds nothing')
  // a relative or empty one is ignored, and still taken out of the env
  for (const value of ['rel/dir', '', './here']) {
    const named = { UNFOLD_BRIDGE_STATE_DIR: value }
    assert.equal(takeStateDir(named), null, JSON.stringify(value))
    assert.deepEqual(named, {}, JSON.stringify(value))
  }
  assert.equal(takeStateDir({}), undefined, 'unset')
})

test('it is written 0600 in a 0700 folder with the port, the key, the pid and the start time', () => {
  const dir = temp()
  try {
    const file = join(dir, 'unfold-bridge', 'endpoint.json')
    writeEndpoint(file, { port: 43123, key: KEY, pid: 4242, started: 1700000000000 })
    assert.equal(mode(join(dir, 'unfold-bridge')), 0o700)
    assert.equal(mode(file), 0o600)
    assert.deepEqual(JSON.parse(readFileSync(file, 'utf8')), { v: 1, port: 43123, key: KEY, pid: 4242, started: 1700000000000 })
    assert.deepEqual(readdirSync(join(dir, 'unfold-bridge')), ['endpoint.json'], 'no temporary file left behind')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('a folder that is already there is tightened to 0700, and an older file is replaced', () => {
  const dir = temp()
  try {
    const folder = join(dir, 'unfold-bridge')
    mkdirSync(folder, { mode: 0o755 })
    chmodSync(folder, 0o755)
    const file = join(folder, 'endpoint.json')
    writeFileSync(file, '{"v":1,"port":1,"key":"old"}', { mode: 0o644 })
    writeEndpoint(file, { port: 2, key: KEY, pid: 1, started: 1 })
    assert.equal(mode(folder), 0o700)
    assert.equal(mode(file), 0o600)
    assert.equal(JSON.parse(readFileSync(file, 'utf8')).key, KEY)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('a folder that is a symlink is refused, and its target is left as it was', () => {
  const dir = temp()
  try {
    const target = join(dir, 'elsewhere')
    mkdirSync(target, { mode: 0o755 })
    chmodSync(target, 0o755)
    symlinkSync(target, join(dir, 'bridge'))
    assert.throws(() => writeEndpoint(join(dir, 'bridge', 'endpoint.json'), { port: 2, key: KEY, pid: 1, started: 1 }), { code: 'symlinked_state_dir' })
    assert.equal(mode(target), 0o755, 'the target keeps its mode')
    assert.deepEqual(readdirSync(target), [], 'and gets no file')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('it is removed at stop, but only while it is still this start\'s', () => {
  const dir = temp()
  try {
    const file = join(dir, 'unfold-bridge', 'endpoint.json')
    writeEndpoint(file, { port: 2, key: KEY, pid: 1, started: 1 })
    removeEndpoint(file, KEY)
    assert.throws(() => statSync(file), { code: 'ENOENT' })
    removeEndpoint(file, KEY)  // already gone: nothing to do
    writeEndpoint(file, { port: 3, key: 'a-newer-start-0123456789abcdef', pid: 2, started: 2 })
    removeEndpoint(file, KEY)
    assert.equal(JSON.parse(readFileSync(file, 'utf8')).port, 3, 'a newer start keeps its file')
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
