// What Unfold answers with, read from the app only. The app is the one source of truth: its
// control endpoint lists the options that answer now (`GET /answer-with`) and takes a pick
// (`POST /answer-with`); nothing here invents an option or a level. The Unfold Bot's model menu
// in DSH (@automagik/unfold-bot-dsh, through the Bot team's `botModels` hook) is drawn from
// `menu()`, which only reads what was last read, so drawing never reaches the app.
//
// The app's facts (`GET /answer-with`, 200):
//   { rev, chosen, answering, why,
//     effort: { chosen, levels: [id] },
//     options: [{ id, name, where: 'mac'|'direct'|'automagik', provider, memories, confirm, efforts }] }
// `chosen` is the option picked; `answering` the one the running agent answers with (null while
// it restarts); `why` why they differ, else null. A pick answers `{rev, after_reply}`:
// `after_reply` true means the app switches once the reply in progress ends.
//
// Read again: at start, when the agent link attaches again (every model change restarts the
// agent), when the menu opens, after a pick, and when a typed turn ends.

import { refusedText } from './host-client.js'

const LEVEL_NAMES = { auto: 'Auto', off: 'Off', low: 'Low', medium: 'Medium', high: 'High', max: 'Max' }
// Unfold's own instance runs on the person's Mac: the device the menu names.
const DEVICE = 'Mac'
const VIA = 'Unfold Automagik'
// A switch waiting for a reply ends within the app's own bound (120 s); past this, the menu stops
// saying it waits and shows what the app says.
const PENDING_MS = 150_000

// A refusal code as the person reads it.
const sentence = (code) => {
  const text = refusedText(code)
  return text.charAt(0).toUpperCase() + text.slice(1)
}
const str = (value) => (typeof value === 'string' && value !== '' ? value : null)

/** The app's facts, kept to their documented fields; null when they are not facts. */
export function cleanFacts(body) {
  if (body === null || typeof body !== 'object' || !Array.isArray(body.options)) return null
  const options = body.options.flatMap((o) => {
    const id = str(o?.id)
    if (id === null || !['mac', 'direct', 'automagik'].includes(o.where)) return []
    return [{ id, name: str(o.name) ?? id, where: o.where, provider: str(o.provider), memories: o.memories === true, confirm: o.confirm === true, efforts: o.efforts === true }]
  })
  const levels = Array.isArray(body.effort?.levels) ? body.effort.levels.filter((id) => str(id) !== null) : []
  return {
    rev: Number.isInteger(body.rev) ? body.rev : null,
    chosen: str(body.chosen),
    answering: str(body.answering),
    why: str(body.why),
    effort: { chosen: str(body.effort?.chosen), levels },
    options,
  }
}

const privacyOf = (o) =>
  o.where === 'mac' ? { kind: 'local', device: DEVICE }
    : o.where === 'direct' ? { kind: 'direct', provider: o.provider ?? o.name, ...(o.memories ? { memories: true } : {}) }
      : { kind: 'relay', provider: o.provider ?? o.name, via: VIA, device: DEVICE }

/** The Bot team's menu (bots-dsh host-models.js) for the app's facts. */
export function menuOf(facts, { pending = null, error = null } = {}) {
  if (!facts) return error ? { entries: [], error } : undefined
  const levels = facts.effort.levels.map((id) => ({ id, name: LEVEL_NAMES[id] ?? id }))
  const entries = facts.options.map((o) => ({
    id: o.id,
    label: o.name,
    group: o.where === 'automagik' ? VIA : 'Unfold',
    ...(o.efforts && levels.length > 0 ? { levels } : {}),
    privacy: privacyOf(o),
    ...(o.confirm ? { confirm: true } : {}),
    ...(o.id === facts.chosen ? { current: true } : {}),
    ...(o.id === facts.answering ? { answering: true } : {}),
  }))
  const answeringName = facts.options.find((o) => o.id === facts.answering)?.name
  const status = pending && pending.choice === facts.chosen && facts.answering !== facts.chosen ? { kind: 'after-reply' }
    : facts.answering === facts.chosen || facts.chosen === null ? undefined
      : facts.why && answeringName ? { kind: 'fallback', answering: answeringName }
        : { kind: 'switching' }
  return {
    entries,
    ...(facts.effort.chosen ? { level: facts.effort.chosen } : {}),
    ...(status ? { status } : {}),
    ...(error ? { error } : {}),
  }
}

export class AnswerWith {
  constructor({ host, log = () => {}, refused = sentence, now = Date.now } = {}) {
    this.host = host
    this.log = log
    this.refused = refused
    this.now = now
    this.facts = null
    this.error = null
    this.pending = null
    this.reading = null
    this.stats = { reads: 0, picks: 0, failed: 0 }
  }

  /** The menu as last read (sync). */
  menu() {
    if (this.pending && this.now() - this.pending.at > PENDING_MS) this.pending = null
    return menuOf(this.facts, { pending: this.pending, error: this.error })
  }

  /** Read the app's facts again; reads asked for meanwhile share one request. */
  read() {
    this.reading ??= (async () => {
      try {
        this.stats.reads++
        const r = await this.host.answerWith()
        const facts = r.status === 200 ? cleanFacts(r.body) : null
        if (facts) {
          this.facts = facts
          this.error = null
          // The pick took over, or another one replaced it (Unfold's own settings).
          if (this.pending && (facts.answering === this.pending.choice || facts.chosen !== this.pending.choice)) this.pending = null
        } else {
          this.stats.failed++
          // One fixed sentence each, so a language pack can word it; the code goes to the log.
          if (r.status !== 0) this.log(`unfold-session: answer-with read refused (${r.body?.refused ?? r.body?.error ?? r.status})`)
          this.error = r.status === 0 ? 'Unfold is not reachable from here right now' : "Unfold's model list could not be read right now"
        }
      } finally {
        this.reading = null
      }
      return this.facts
    })()
    return this.reading
  }

  /** The person's pick from the menu: `{id?, effort?, ack?}`. Rejects with words for the person. */
  async pick({ id, effort, ack } = {}) {
    this.stats.picks++
    const r = await this.host.chooseAnswer({ choice: id, effort, ack })
    if (r.status !== 200) {
      const code = r.body?.refused ?? r.body?.error ?? String(r.status)
      this.log(`unfold-session: answer-with pick refused (${code})`)
      await this.read().catch(() => {})
      throw new Error(r.status === 0 && r.body?.sent === 'unknown' ? 'The pick may not have reached Unfold; open the menu again to see' : this.refused(code))
    }
    if (typeof id === 'string' && r.body?.after_reply === true) this.pending = { choice: id, at: this.now() }
    await this.read()
    return { afterReply: r.body?.after_reply === true }
  }
}
