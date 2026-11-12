// Check the static module table shipped by the real DSH web Host, rather than a
// primitives stub. DSH_HARNESS selects an installed Host for compatibility runs.
import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const harness = process.env.DSH_HARNESS || join(root, 'plugins/unfold-trajectory-dsh/dev/harness')
const hostRequire = createRequire(join(harness, 'package.json'))
const { parse } = hostRequire('acorn')
const frontendManifest = hostRequire.resolve('@deepseek-ai/dsh-web-frontend/package.json')
const frontend = JSON.parse(readFileSync(frontendManifest, 'utf8'))
const assets = join(dirname(frontendManifest), 'dist/assets')
const primitives = '@deepseek-ai/dsh-client-ui-primitives'
const escape = value => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

function hostExports() {
  for (const file of readdirSync(assets).filter(name => name.endsWith('.js'))) {
    const source = readFileSync(join(assets, file), 'utf8')
    const binding = source.match(new RegExp(`"${escape(primitives)}":([\\w$]+)`))?.[1]
    if (!binding) continue
    const table = source.match(new RegExp(`${escape(binding)}=Object\\.freeze\\(Object\\.defineProperty\\(\\{__proto__:null,([^{}]+)\\},Symbol\\.toStringTag`))?.[1]
    assert.ok(table, `${file}: cannot read the Host primitives table; inspect the new bundle format`)
    const entries = table.split(',')
    assert.ok(entries.every(entry => /^[\w$]+:[\w$]+$/.test(entry)), 'the export table must contain explicit named bindings')
    return new Set(entries.map(entry => entry.split(':')[0]))
  }
  assert.fail('the installed DSH frontend has no primitives module table')
}

function walk(node, visit) {
  if (!node || typeof node !== 'object') return
  if (node.type) visit(node)
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => walk(child, visit))
    else if (value && typeof value === 'object') walk(value, visit)
  }
}

function clientReads(source) {
  const ast = parse(source, { ecmaVersion: 'latest', sourceType: 'script' })
  const aliases = new Set()
  const isImport = node => node?.type === 'CallExpression' && node.callee.name === 'require' && node.arguments[0]?.value === primitives
  walk(ast, node => {
    if (node.type !== 'VariableDeclarator' || !(isImport(node.init) || isImport(node.init?.arguments?.[0]))) return
    assert.equal(node.id.type, 'Identifier', 'destructured primitives imports need explicit coverage')
    aliases.add(node.id.name)
  })
  if (!aliases.size) return new Set()
  // The trajectory runtime receives this namespace as kindTag's primitives argument.
  aliases.add('primitives')
  const names = new Set()
  walk(ast, node => {
    if (node.type !== 'MemberExpression' || node.object.type !== 'Identifier' || !aliases.has(node.object.name)) return
    assert.ok(!node.computed || typeof node.property.value === 'string', 'dynamic primitives reads need explicit coverage')
    names.add(node.computed ? node.property.value : node.property.name)
  })
  return names
}

function requiredReads(names, source, file) {
  const required = new Set(names)
  // The only optional capability: shell/vendor/omni-ui-dsh/src/client/user-message.js:16;
  // client.js:14029 checks its type, and :15093 gates the user/steering renderers.
  if (file === 'shell/vendor/omni-ui-dsh/client.js' && names.has('projectUserText')) {
    assert.match(source, /var canDrawUserMessages = \(\) => typeof ui\.projectUserText === "function";/)
    assert.match(source, /\.\.\.canDrawUserMessages\(\) \? \["user", "steering"\]\.map\(/)
    required.delete('projectUserText')
  }
  return required
}

function checkReads(names, table, file) {
  const missing = [...names].filter(name => !table.has(name)).sort()
  assert.deepEqual(missing, [], `${file}: exports missing from DSH ${frontend.version}`)
}

const table = hostExports()
const bundles = readdirSync(join(root, 'plugins')).filter(name => /^unfold-.*-dsh$/.test(name))
  .map(name => `plugins/${name}/client.js`).filter(file => existsSync(join(root, file)))
bundles.push('shell/vendor/omni-ui-dsh/client.js')

test('all client primitives reads exist in the real installed Host export table', () => {
  assert.equal(frontend.devDependencies[primitives], frontend.version, 'check the primitives build served by this Host version')
  const consumers = new Set()
  for (const file of bundles) {
    const source = readFileSync(join(root, file), 'utf8')
    const names = clientReads(source)
    if (!names.size) continue
    consumers.add(file)
    checkReads(requiredReads(names, source, file), table, file)
    console.log(`${frontend.version} ${file}: ${[...names].sort().join(', ')}`)
  }
  assert.ok(consumers.has('plugins/unfold-trajectory-dsh/client.js'), 'trajectory must be checked')
  assert.ok(consumers.has('shell/vendor/omni-ui-dsh/client.js'), 'the vendored Omni client must be checked')
})

test('the check rejects a removed real export and an old icon name', () => {
  const names = clientReads('const ui = require("@deepseek-ai/dsh-client-ui-primitives"); ui.MarkdownText; ui.IconSettingsOutline16;')
  assert.deepEqual([...names].sort(), ['IconSettingsOutline16', 'MarkdownText'])
  assert.ok(table.has('MarkdownText'))
  const missing = new Set(table)
  missing.delete('MarkdownText')
  assert.throws(() => checkReads(new Set(['MarkdownText']), missing, 'removed export'), /MarkdownText/)
  assert.throws(() => checkReads(names, table, 'old icon'), /IconSettingsOutline16/)
})

test('the optional capability is allowed only while its detection and rendering guards remain', () => {
  const file = 'shell/vendor/omni-ui-dsh/client.js'
  const source = readFileSync(join(root, file), 'utf8')
  const names = clientReads(source)
  assert.ok(names.has('projectUserText'))
  assert.throws(() => requiredReads(names, source.replace('typeof ui.projectUserText', 'typeof undefined'), file))
  assert.throws(() => requiredReads(names, source.replace('...canDrawUserMessages() ?', '...true ?'), file))
  assert.ok(requiredReads(names, source, 'another-client.js').has('projectUserText'))
})
