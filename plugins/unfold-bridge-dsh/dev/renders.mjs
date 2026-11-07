// Headless renders of the DSH client's slots, for review: every slot in English and Brazilian
// Portuguese across Unfold connected or not × a draft held or not × an approval waiting or not,
// plus the first-run board, subscriptions with no `added_by` (no badge), hidden characters in a
// draft and in advice, and Unfold's view not sent yet. Each render is an HTML file with the client's own styles.
//
//   node dev/renders.mjs --out <dir>
import { mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { parseArgs } from 'node:util'
import { OTHER, SESSION, fixtureView } from '../test/fixture-view.js'
import { expand, html, loadClient } from '../test/render.js'

const { values } = parseArgs({ options: { out: { type: 'string' } } })
if (!values.out) {
  console.error('usage: node dev/renders.mjs --out <dir>')
  process.exit(2)
}
const out = resolve(values.out)
mkdirSync(out, { recursive: true })

const client = await loadClient()
const actions = { send() {}, cancel() {}, watch() {}, drop() {}, undo() {} }
const draw = (View, props) => html(expand({ type: View, props: { actions, ...props } }))
// The page's own rules come after the client's, so the open board sits inside its frame.
const page = (title, body) => `<!doctype html>
<html><head><meta charset="utf-8"><title>${title}</title><style>${client.STYLES}</style><style>
body { font: 13px/1.45 system-ui, sans-serif; margin: 16px; max-width: 720px; }
.frame { border: 1px dashed #aaa; padding: 8px; margin: 8px 0; position: relative; }
.frame > .slot { font: 11px monospace; color: #777; }
.ub-board { position: static; }
</style></head><body><h1>${title}</h1>${body}</body></html>
`
const frame = (slot, body) => `<div class="frame"><div class="slot">${slot}</div>${body || '<em>(renders nothing)</em>'}</div>`

const SLOTS = {
  row: (snap, t) => frame(`sidebar.workspaces.session.row.action · ${SESSION}`, draw(client.RowAction, { snap, t, sessionId: SESSION }))
    + frame(`sidebar.workspaces.session.row.action · ${OTHER}`, draw(client.RowAction, { snap, t, sessionId: OTHER }))
    + frame(`sidebar.session.row.leading · ${SESSION}`, draw(client.WatchedMark, { snap, t, sessionId: SESSION }))
    + frame(`sidebar.session.row.leading · ${OTHER}`, draw(client.WatchedMark, { snap, t, sessionId: OTHER })),
  board: (snap, t) => frame('sidebar.footer.action (closed)', draw(client.BoardButton, { snap, t, wide: true }))
    + frame('sidebar.footer.action (board open)', draw(client.BoardPanel, { snap, t, onClose() {} })),
  dock: (snap, t) => frame(`conversation.input.dock · ${SESSION}`, draw(client.Dock, { snap, t, sessionId: SESSION }))
    + frame(`conversation.input.dock · ${OTHER}`, draw(client.Dock, { snap, t, sessionId: OTHER })),
  settings: (snap, t) => frame('settings.section', draw(client.SettingsSection, { snap, t })),
}

const written = []
const write = (name, title, body) => {
  writeFileSync(join(out, name), page(title, body))
  written.push(name)
}
for (const language of ['en', 'pt-BR']) {
  const t = client.translator(language)
  for (const connected of [true, false]) for (const draft of [true, false]) for (const approval of [true, false]) {
    const snap = fixtureView({ connected, draft, approval, marked: approval })
    const state = `${language}_${connected ? 'connected' : 'not-connected'}_${draft ? 'draft' : 'no-draft'}_${approval ? 'approval' : 'no-approval'}`
    for (const [slot, render] of Object.entries(SLOTS)) write(`${state}_${slot}.html`, `${state} · ${slot}`, render(snap, t))
  }
  write(`${language}_hidden-characters_dock.html`, `${language} · hidden characters in the draft and the advice · dock`, SLOTS.dock(fixtureView({ draft: true, approval: true, hidden: true }), t))
  write(`${language}_hidden-characters_board.html`, `${language} · hidden characters in the advice · board`, SLOTS.board(fixtureView({ draft: true, approval: true, hidden: true }), t))
  write(`${language}_no-view-yet_settings.html`, `${language} · Unfold's view not sent yet · settings`, SLOTS.settings(fixtureView({ connected: false, view: false }), t))
  write(`${language}_no-badges_settings.html`, `${language} · subscriptions without added_by · settings`, SLOTS.settings(fixtureView({ badges: false }), t))
  write(`${language}_first-run_board.html`, `${language} · first run · board`, SLOTS.board(fixtureView({ subscriptions: false, log: false }), t))
  write(`${language}_bridge-down_settings.html`, `${language} · view stream down · settings`, SLOTS.settings(null, t))
  // The install offer: the browser can prompt, or not (Safari), each closed and opened by #install.
  for (const canPrompt of [true, false]) for (const opened of [false, true]) {
    const offer = { canPrompt, opened, installed: false, prompts: canPrompt }
    const state = `${language}_install_${canPrompt ? 'can-prompt' : 'safari'}_${opened ? 'opened-by-hash' : 'closed'}`
    write(`${state}.html`, `${state} · sidebar.footer.action`, frame('sidebar.footer.action · unfold-bridge.install', draw(client.InstallAction, { offer, t, onInstall() {}, wide: true })))
  }
}
writeFileSync(join(out, 'index.html'), page('Unfold bridge client renders', `<ul>${written.map(name => `<li><a href="${name}">${name}</a></li>`).join('')}</ul>`))
console.log(`renders: ${written.length} files in ${out}`)
