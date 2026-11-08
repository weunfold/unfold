// The DSH client, bundled once and run outside a browser with a minimal React stand-in: a
// component's elements are plain nodes, and `expand` calls function components down to the
// host elements, which `html` and `text` read. Hooks hold their first value and effects never
// run, `fetch` is whatever the test puts in `client.fetch`, and every console call lands in
// `client.logs`.
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { runInNewContext } from 'node:vm'
import { build } from 'esbuild'

const PLUGIN = dirname(dirname(fileURLToPath(import.meta.url)))
const Fragment = Symbol('Fragment')

const react = {
  createElement: (type, props, ...children) => ({ type, props: { ...props, children: children.flat(Infinity) } }),
  Fragment,
  useState: initial => [initial, () => {}],
  useSyncExternalStore: (_subscribe, get) => get(),
  useEffect() {},
  useRef: current => ({ current }),
}

export async function loadClient() {
  const { outputFiles } = await build({
    stdin: {
      contents: [
        "export * from './src/client/api.js'",
        "export * from './src/client/i18n.js'",
        "export * from './src/client/install.js'",
        "export * from './src/client/hidden.js'",
        "export * from './src/client/slots.js'",
        "export * from './src/client/view.js'",
        "export { STYLES } from './src/client/styles.js'",
        "export { SLOTS } from './src/client/index.js'",
      ].join('\n'),
      resolveDir: PLUGIN,
    },
    bundle: true, format: 'cjs', platform: 'browser', write: false, logLevel: 'silent',
    external: ['react', 'react-dom'],
  })
  const client = { fetch: () => Promise.reject(new Error('no fetch in this test')), logs: [] }
  const log = (...args) => { client.logs.push(args) }
  const module = { exports: {} }
  runInNewContext(outputFiles[0].text, {
    module, exports: module.exports, URL, encodeURIComponent, Promise, Intl, Date,
    console: { log, info: log, warn: log, error: log, debug: log },
    setTimeout, clearTimeout,
    fetch: (...args) => client.fetch(...args),
    require: name => (name === 'react' ? react : {}),
  })
  return Object.assign(client, module.exports)
}

// Function components called down to host elements; null, false and undefined drop out.
export function expand(node) {
  if (node === null || node === undefined || node === false || node === true) return null
  if (Array.isArray(node)) return node.map(expand).filter(child => child !== null)
  if (typeof node !== 'object') return node
  if (typeof node.type === 'function') return expand(node.type(node.props))
  const children = expand(node.props.children ?? [])
  return { ...node, props: { ...node.props, children } }
}

const nodes = tree => (Array.isArray(tree) ? tree : tree === null ? [] : [tree])
export function findAll(tree, match) {
  const found = []
  const walk = node => {
    if (node === null || typeof node !== 'object') return
    if (Array.isArray(node)) return node.forEach(walk)
    if (match(node)) found.push(node)
    walk(node.props.children)
  }
  nodes(tree).forEach(walk)
  return found
}
export const find = (tree, match) => findAll(tree, match)[0] ?? null
export const byData = (name, value) => node => node.props?.[name] !== undefined && (value === undefined || node.props[name] === value)

export function text(tree) {
  if (tree === null || tree === undefined) return ''
  if (Array.isArray(tree)) return tree.map(text).join('')
  if (typeof tree !== 'object') return String(tree)
  return text(tree.props.children)
}

const escape = value => String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
const ATTRIBUTES = { className: 'class', strokeWidth: 'stroke-width', viewBox: 'viewBox', tabIndex: 'tabindex', dateTime: 'datetime' }
export function html(tree) {
  if (tree === null || tree === undefined) return ''
  if (Array.isArray(tree)) return tree.map(html).join('')
  if (typeof tree !== 'object') return escape(tree)
  if (tree.type === Fragment) return html(tree.props.children)
  const attributes = Object.entries(tree.props)
    .filter(([name, value]) => name !== 'children' && name !== 'key' && name !== 'ref' && typeof value !== 'function' && value !== null && value !== undefined && value !== false)
    .map(([name, value]) => (value === true ? ` ${ATTRIBUTES[name] ?? name}` : ` ${ATTRIBUTES[name] ?? name}="${escape(value)}"`))
    .join('')
  return `<${tree.type}${attributes}>${html(tree.props.children)}</${tree.type}>`
}
