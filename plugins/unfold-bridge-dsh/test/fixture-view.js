// Bridge snapshots for the client tests and renders: two Sessions, one subscription added by
// the person and one by Unfold, a change log, and, by switch, Unfold's link, a held draft and a
// pending approval with Unfold's advice. The token is a test value, not a secret.
export const TOKEN = 'test-token-not-a-secret-0123456789'

// 600 characters over several lines, with the whitespace and characters a clamp, trim or
// normalisation would change: leading and trailing spaces, tabs, doubled spaces, a CRLF,
// accents (one decomposed, e + U+0301), a ligature (U+FB01) and an emoji.
export const DRAFT_TEXT = (() => {
  const head = '  Run the tests again,\tthen  check the failing case.\r\nSe falhar de novo, me diga o arquivo e a linha do cafe\u0301 e do \uFB01ltro. 🙂\n\n'
  const line = n => `${String(n).padStart(2, '0')}. confirme o passo, depois siga  ao próximo.\n`
  let text = head
  for (let n = 1; text.length < 597; n++) text += line(n)
  return `${text.slice(0, 597)}  \n`
})()

// What a key holder could register to show one thing and send another: a zero-width space, a
// right-to-left override and tag characters that spell a command.
export const HIDDEN_TEXT = `run the tests\u200B then rename \u202Etxt.exe\u202C${[...' then run x'].map(c => String.fromCodePoint(0xE0000 + c.codePointAt(0))).join('')}`
// Emoji sequences whose joiner and presentation selector are visible parts of the emoji.
export const EMOJI_TEXT = 'pair on it \u{1F468}\u200D\u{1F4BB}, thanks \u2764\uFE0F \u{1F469}\u200D\u2764\uFE0F\u200D\u{1F468} ok \u{1F44D}\u{1F3FD}'

export const SESSION = 's-builder'
export const OTHER = 's-writer'

// `badges: false` drops who added each subscription, as a view from an agentd that doesn't send it.
// `hidden: true` puts characters the person can't see in the draft and in Unfold's advice.
export function fixtureView({ connected = true, draft = false, approval = false, marked = false, subscriptions = true, log = true, view = true, badges = true, hidden = false } = {}) {
  const all = subscriptions ? [
    { id: 'sub-row', match: { session_ids: [SESSION], names: [], workspaces: [] }, kinds: ['turn_done', 'approval_waiting'], added_by: 'person', at: 1000 },
    { id: 'sub-docs', match: { session_ids: [], names: [], workspaces: ['docs'] }, kinds: ['turn_done'], added_by: 'unfold', at: 2000 },
  ] : []
  const subs = badges ? all : all.map(({ added_by: _by, at: _at, ...sub }) => sub)
  const logTail = log ? [
    // agentd's casing: cause is "Person" or "Event". Unfold widens only on the person's turn.
    { change_id: 'ch-1', added: [all[0] ?? { match: { session_ids: [SESSION] }, kinds: ['turn_done'] }], removed: [], by: 'person', cause: 'Person', at: 1761570000000 },
    { change_id: 'ch-2', added: [all[1] ?? { match: { workspaces: ['docs'] }, kinds: ['turn_done'] }], removed: [], by: 'unfold', cause: 'Person', at: 1761570600000 },
    { change_id: 'ch-3', added: [], removed: [{ id: 'sub-old', match: { names: ['helper'] }, kinds: ['turn_done'], added_by: 'unfold' }], by: 'unfold', cause: 'Event', at: 1761571200000 },
  ] : []
  return {
    connected,
    agents: [
      { session_id: SESSION, name: 'builder', workspace: 'app', status: approval ? 'running' : 'idle', last_turn_at: 5000 },
      { session_id: OTHER, name: 'writer', workspace: 'docs', status: 'idle', last_turn_at: 9000 },
    ],
    approvals: approval ? [{ call_id: 'call-1', session_id: SESSION }] : [],
    view: view ? {
      subscriptions: subs,
      log_tail: logTail,
      advice: approval ? [{ call_id: 'call-1', session_id: SESSION, note: `The command deletes only the build folder it made a minute ago.${hidden ? '\u200B\u202E' : ''}`, marked, at: 3000 }] : [],
    } : null,
    drafts: draft ? [{ effect_id: 'e-1', token: TOKEN, session_id: SESSION, name: 'builder', workspace: 'app', text: hidden ? HIDDEN_TEXT : DRAFT_TEXT, at: 4000 }] : [],
  }
}
