// The report's identity-safe source: agentd's own journal ids as the observer stream carries them,
// never chat adjacency.
// Utterance parts by (producer, utterance_id, utterance_seq) with their `final` flag; replies by
// (producer, response_id) with their terminal status; typed commands by command_id. Items are kept
// in arrival order (a producer's seq order); no item ever records which reply answered which
// utterance. In memory only: what was said before this DSH process started is not here, and the
// snapshot says so (`omissions.before_observer_start`).
//
// Envelope identity: `(producer, seq)` names one journaled envelope, and a producer's seqs
// only grow within its lifetime (rth-protocol `Envelope`). Each identity mutates the journal at
// most once: an identical repeat is dropped; a repeat with other content (another text, target or
// kind) marks `conflicted` BOTH the item the identity first touched and the one the repeat names,
// also if that one only appears later (omitted from the report input and listed, never resolved by
// picking one); an older seq that is no longer remembered is dropped unverified. Every envelope
// with a producer and seq is checked, not only transcript kinds. Wire-only envelopes (seq 0) are
// not journaled, so they are not report input either.
import { canonicalSha256 } from './canon.mjs'

export const MAX_ITEMS = 400
export const MAX_SEEN = 4 * MAX_ITEMS
const CONTENT = ['kind', 'utterance_id', 'utterance_seq', 'text', 'final', 'response_id', 'status', 'command_id', 'duplicate']

export class TranscriptJournal {
  constructor({ maxItems = MAX_ITEMS } = {}) {
    this.maxItems = maxItems
    this.items = new Map() // key → item
    this.ord = 0
    this.dropped = 0
    this.startedAt = new Date().toISOString()
    this.seen = new Map() // `${producer}\n${seq}` → {digest, key}: the content and the item it first touched, bounded (MAX_SEEN)
    this.disputed = new Set() // item keys a conflicting identity named; an item created later under one is conflicted too
    this.last = new Map() // producer → highest seq seen
    this.repeats = { identical: 0, conflicting: 0, stale: 0, unjournaled: 0 }
  }

  /** The item an envelope would touch, for marking a conflict. */
  #keyOf(env) {
    const p = env.producer
    if (env.kind === 'input_transcript') return env.utterance_id ? `u:${p}/${env.utterance_id}` : null
    if (env.kind === 'response_started' || env.kind === 'agent_delta' || env.kind === 'agent_done') return env.response_id ? `r:${p}/${env.response_id}` : null
    if (env.kind === 'command_admitted') return env.command_id ? `c:${env.command_id}` : null
    return null
  }

  /** True when this envelope may mutate the journal (its first sighting). */
  #admit(env, key) {
    if (!Number.isInteger(env.seq) || env.seq <= 0) { this.repeats.unjournaled++; return false }
    const id = `${env.producer}\n${env.seq}`
    const digest = canonicalSha256(Object.fromEntries(CONTENT.filter((k) => env[k] !== undefined).map((k) => [k, env[k]])))
    const known = this.seen.get(id)
    if (known !== undefined) {
      if (known.digest === digest) { this.repeats.identical++; return false }
      this.repeats.conflicting++
      for (const k of [known.key, key]) {
        if (!k) continue
        this.disputed.add(k)
        const it = this.items.get(k)
        if (it) it.conflicted = true
      }
      return false
    }
    if (env.seq <= (this.last.get(env.producer) ?? 0)) { this.repeats.stale++; return false }
    this.last.set(env.producer, env.seq)
    this.seen.set(id, { digest, key })
    while (this.seen.size > MAX_SEEN) this.seen.delete(this.seen.keys().next().value)
    return true
  }

  #item(key, make) {
    let it = this.items.get(key)
    if (!it) {
      it = { ...make(), ord: ++this.ord }
      if (this.disputed.has(key)) it.conflicted = true
      this.items.set(key, it)
      while (this.items.size > this.maxItems) {
        this.items.delete(this.items.keys().next().value)
        this.dropped++
      }
    }
    return it
  }

  ingest(env) {
    const p = env?.producer
    if (!p) return
    const key = this.#keyOf(env)
    if (!this.#admit(env, key) || !key) return
    switch (env.kind) {
      case 'input_transcript': {
        if (!env.utterance_id) return
        // A new utterance from this producer closes the open ones before it (end not observed).
        const key = `u:${p}/${env.utterance_id}`
        if (!this.items.has(key)) for (const it of this.items.values()) if (it.kind === 'utterance' && it.producer === p && it.finality === 'open') it.finality = 'closed'
        const it = this.#item(key, () => ({ kind: 'utterance', producer: p, id: env.utterance_id, first_seq: env.seq, parts: new Map(), finality: 'open' }))
        const n = Number.isInteger(env.utterance_seq) ? env.utterance_seq : it.parts.size
        if (!it.parts.has(n)) it.parts.set(n, String(env.text ?? ''))
        if (env.final === true) it.finality = 'final'
        return
      }
      case 'response_started': {
        if (!env.response_id) return
        this.#item(`r:${p}/${env.response_id}`, () => ({ kind: 'reply', producer: p, id: env.response_id, first_seq: env.seq, deltas: [], status: 'open' }))
        return
      }
      case 'agent_delta': {
        const it = this.items.get(`r:${p}/${env.response_id}`)
        if (it && it.status === 'open') it.deltas.push(String(env.text ?? ''))
        return
      }
      case 'agent_done': {
        const it = this.items.get(`r:${p}/${env.response_id}`)
        if (it && it.status === 'open') it.status = env.status ?? 'unknown'
        return
      }
      case 'command_admitted': {
        if (!env.command_id || env.duplicate) return
        const it = this.#item(`c:${env.command_id}`, () => ({ kind: 'typed', producer: p, id: env.command_id, first_seq: env.seq, text: null }))
        it.admitted = true
        return
      }
    }
  }

  /** The plugin's own typed text for a command it submitted (agentd's admission carries no text). */
  typed(commandId, text) {
    const it = this.#item(`c:${commandId}`, () => ({ kind: 'typed', producer: 'dsh:composer', id: commandId, first_seq: null, text: null }))
    if (it.text == null) it.text = String(text ?? '')
  }

  /** A frozen, chronological view: plain objects, each with id, revision and finality. */
  freeze() {
    const out = []
    for (const it of [...this.items.values()].sort((a, b) => a.ord - b.ord)) {
      if (it.conflicted) {
        out.push({ kind: it.kind, producer: it.producer, id: it.id, revision: 0, finality: 'conflicted', text: '' })
      } else if (it.kind === 'utterance') {
        const parts = [...it.parts.entries()].sort((a, b) => a[0] - b[0]).map((e) => e[1])
        out.push({ kind: 'utterance', producer: it.producer, id: it.id, revision: parts.length, finality: it.finality, text: parts.join('').trim() })
      } else if (it.kind === 'reply') {
        out.push({ kind: 'reply', producer: it.producer, id: it.id, revision: it.deltas.length, finality: it.status, text: it.deltas.join('').trim() })
      } else {
        out.push({ kind: 'typed', producer: it.producer, id: it.id, revision: 1, finality: it.admitted ? 'admitted' : 'submitted', text: (it.text ?? '').trim() })
      }
    }
    return { items: out, watermark: this.ord, dropped: this.dropped, startedAt: this.startedAt, repeats: { ...this.repeats } }
  }
}
