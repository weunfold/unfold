// The event catalog (catalog/events.v3.json) is the only place a wire kind gets its plain
// name. Readers never hard-code names: an unknown kind is passed through and flagged, so the
// pane survives the protocol moving under it.
import { readFileSync } from 'node:fs'

export const DEFAULT_CATALOG = new URL('../../../catalog/events.v3.json', import.meta.url)

const ENVELOPE_KEYS = new Set(['v', 'id', 'producer', 'seq', 't_ns', 'source', 'cause', 'received', 'observed', 'kind'])
const SUMMARY_MAX = 160

export function loadCatalog(path = DEFAULT_CATALOG) {
  return new Catalog(JSON.parse(readFileSync(path, 'utf8')))
}

export class Catalog {
  constructor(doc) {
    if (doc.catalog_format !== 'rth.event-catalog/v1') throw new Error(`unknown catalog format ${doc.catalog_format}`)
    this.version = doc.catalog_version
    this.phases = Object.keys(doc.phases)
    this.byWire = new Map()
    for (const entry of doc.events) {
      if (this.byWire.has(entry.wire)) throw new Error(`catalog lists wire kind ${entry.wire} twice`)
      if (!this.phases.includes(entry.phase)) throw new Error(`${entry.wire}: unknown phase ${entry.phase}`)
      this.byWire.set(entry.wire, entry)
    }
  }

  entry(kind) {
    return this.byWire.get(kind) ?? null
  }

  /** One envelope as a row a person reads. Never throws on an unknown kind. */
  describe(env) {
    const entry = this.entry(env.kind)
    const fields = eventFields(env)
    return {
      v: env.v ?? null,
      id: env.id,
      producer: env.producer,
      seq: env.seq,
      t_ns: env.t_ns,
      source: env.source,
      cause: env.cause ?? null,
      received: env.received ?? null, // the producer side's admission stamp, if the wire carried one
      observed: env.observed ?? null, // our own receipt, kept apart from `received`
      wire: env.kind,
      name: entry?.name ?? env.kind,
      phase: entry?.phase ?? 'agent',
      known: entry !== null,
      media: Boolean(entry?.media),
      coalesce: entry?.coalesce ? fields[entry.coalesce] ?? null : null,
      summary: entry ? render(entry, fields) : clip(JSON.stringify(fields)),
      fields,
    }
  }
}

export function eventFields(env) {
  const out = {}
  for (const [k, v] of Object.entries(env)) if (!ENVELOPE_KEYS.has(k)) out[k] = v
  return out
}

function render(entry, fields) {
  return clip(entry.summary.replace(/\{(\w+)\}/g, (_, key) => {
    const raw = fields[key]
    const mapped = entry.values?.[key]?.[raw]
    if (mapped !== undefined) return mapped
    if (raw === undefined || raw === null) return '?'
    if (Array.isArray(raw)) return raw.join(' / ')
    return typeof raw === 'object' ? JSON.stringify(raw) : String(raw)
  }))
}

const clip = (s) => (s.length > SUMMARY_MAX ? `${s.slice(0, SUMMARY_MAX - 1)}…` : s)
