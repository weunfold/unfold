// Canonical JSON (RFC 8785 for the values this module produces: strings, integers, booleans,
// null, arrays and objects, with keys sorted by UTF-16 code units) and its SHA-256.
import { createHash } from 'node:crypto'

export function canonical(v) {
  if (v === null || typeof v !== 'object') {
    if (typeof v === 'number' && !Number.isInteger(v)) throw new Error('canonical: only integers are used here')
    return JSON.stringify(v)
  }
  if (Array.isArray(v)) return `[${v.map(canonical).join(',')}]`
  const keys = Object.keys(v).filter((k) => v[k] !== undefined).sort()
  return `{${keys.map((k) => `${JSON.stringify(k)}:${canonical(v[k])}`).join(',')}}`
}

export const sha256 = (s) => createHash('sha256').update(s).digest('hex')
export const canonicalSha256 = (v) => sha256(canonical(v))
