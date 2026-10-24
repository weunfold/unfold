// What an event kind means, for the audit panel: the event catalog entry (catalog/events.v3.json,
// read-only), its field notes, and the rth-protocol v2 doc comment above the variant
// (crates/rth-protocol/src/lib.rs, read-only, best effort). Host journal notes are not in either:
// they get this plugin's own glossary line, labelled as such. Nothing here is authoritative
// beyond the file it quotes; a missing file or variant is "unknown".
import { readFileSync } from 'node:fs'
import { HOST_GLOSSARY } from './sentences.mjs'

export const DEFAULT_CATALOG = new URL('../../../catalog/events.v3.json', import.meta.url)
export const DEFAULT_PROTOCOL = new URL('../../../crates/rth-protocol/src/lib.rs', import.meta.url)

export function loadDefinitions({ catalog = DEFAULT_CATALOG, protocol = DEFAULT_PROTOCOL } = {}) {
  let doc = null
  let catalogError = null
  try {
    doc = JSON.parse(readFileSync(catalog, 'utf8'))
    if (doc.catalog_format !== 'rth.event-catalog/v1') throw new Error(`unknown catalog format ${doc.catalog_format}`)
  } catch (e) {
    catalogError = e.message
    doc = null
  }
  let protocolDocs = new Map()
  let protocolError = null
  try {
    protocolDocs = protocolComments(readFileSync(protocol, 'utf8'))
  } catch (e) {
    protocolError = e.message
  }
  const byWire = new Map((doc?.events ?? []).map((e) => [e.wire, e]))
  const notes = new Map()
  for (const n of doc?.field_notes ?? []) notes.set(n.wire, [...(notes.get(n.wire) ?? []), n])
  return {
    catalogVersion: doc?.catalog_version ?? null,
    catalogError,
    protocolError,
    entry: (wire) => byWire.get(wire) ?? null,
    /** The audit panel's "what this means" block for one row. */
    define(lane, kind) {
      if (lane === 'host') {
        return { kind, lane, catalog: null, protocol: null, glossary: HOST_GLOSSARY[kind] ?? `rth-host note “${kind}”: unknown to Mind and not in the events catalog` }
      }
      if (lane === 'mind') return { kind, lane, catalog: null, protocol: null, glossary: 'A marker written by the Mind plugin itself (source UI), not an Unfold event.' }
      const e = byWire.get(kind)
      return {
        kind,
        lane,
        catalog: e
          ? { file: 'catalog/events.v3.json', version: doc.catalog_version, name: e.name, phase: e.phase, phase_means: doc.phases?.[e.phase] ?? 'unknown', summary_template: e.summary, note: e.note ?? null, values: e.values ?? null, field_notes: (notes.get(kind) ?? []).map((n) => ({ added: n.added, source: n.source, note: n.note })) }
          : { file: 'catalog/events.v3.json', missing: catalogError ?? `wire kind “${kind}” is not in catalog v${doc?.catalog_version ?? '?'}` },
        protocol: protocolDocs.has(kind)
          ? { file: 'crates/rth-protocol/src/lib.rs', version: 2, variant: protocolDocs.get(kind).variant, direction: protocolDocs.get(kind).direction, comment: protocolDocs.get(kind).comment || 'no doc comment on this variant' }
          : { file: 'crates/rth-protocol/src/lib.rs', missing: protocolError ?? 'variant not found' },
        glossary: null,
      }
    },
  }
}

/** `/// …` comments directly above each `Event` variant, keyed by its snake_case wire kind. */
export function protocolComments(src) {
  const start = src.indexOf('pub enum Event {')
  if (start < 0) throw new Error('pub enum Event not found')
  const end = src.indexOf('\n}\n', start)
  const lines = src.slice(start, end).split('\n')
  const out = new Map()
  let doc = []
  let direction = 'unknown'
  for (const line of lines) {
    const dir = /^\s{4}\/\/ ── (guest → host|host → guest) ──/.exec(line)
    if (dir) {
      direction = dir[1]
      doc = []
      continue
    }
    const c = /^\s{4}\/\/\/ ?(.*)$/.exec(line)
    if (c) {
      doc.push(c[1])
      continue
    }
    const v = /^\s{4}([A-Z][A-Za-z]+)\s*[{(,]/.exec(line) ?? /^\s{4}([A-Z][A-Za-z]+)\s*$/.exec(line)
    if (v) {
      const wire = v[1].replace(/(?<!^)([A-Z])/g, '_$1').toLowerCase()
      out.set(wire, { variant: v[1], direction, comment: doc.join(' ').trim() })
      doc = []
      continue
    }
    if (!/^\s{8}/.test(line) && !/^\s*#\[/.test(line)) doc = []
  }
  return out
}
