import { createElement as h, useEffect, useRef, useState } from 'react'
import { hiddenIn, hiddenWarning } from './hidden.js'
import { when } from './i18n.js'
import { offerShown } from './install.js'
import { adviceFor, agentOf, changesOf, describeChange, describeSubscription, draftFor, noticesOf, pendingFor, rowSubscription, statusText, suggestion, watchedAgent } from './view.js'

// The slot views. Each one is a function of the bridge's snapshot (`snap`, null while the view
// stream is down), the copy (`t`) and the taps (`actions`), so the tests and the renders draw
// them from a fixture. None of them shows a draft's token; only Send and Cancel carry it.

const KNOWN_ERRORS = new Set(['bad_token', 'not_found', 'send_failed', 'send_unknown', 'agentd_unavailable', 'unauthorized'])
const errorText = (code, t) => (KNOWN_ERRORS.has(code) ? t(`error.${code}`) : t('error.other', { code }))

// One tap at a time per control: busy while it runs, a refusal shown where it was made.
function useTap() {
  const [state, setState] = useState({ busy: false, error: null })
  const run = action => {
    if (state.busy) return
    setState({ busy: true, error: null })
    Promise.resolve().then(action).then(
      () => setState({ busy: false, error: null }),
      error => setState({ busy: false, error: error?.code ?? 'other' }),
    )
  }
  return [run, state]
}

const TapError = ({ code, t }) => (code ? h('p', { className: 'ub-error', role: 'alert' }, errorText(code, t)) : null)

const dotKind = status => (status === 'running' ? 'running' : status === 'error' ? 'error' : 'idle')
const Dot = ({ status, watched }) => h('span', { className: `ub-dot ub-dot-${dotKind(status)}${watched ? ' ub-dot-watched' : ''}`, 'aria-hidden': 'true' })

const Eye = ({ on }) => h('svg', { className: 'ub-eye', viewBox: '0 0 16 16', width: 14, height: 14, 'aria-hidden': 'true' },
  h('path', { d: 'M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8s-2.4 4.5-6.5 4.5S1.5 8 1.5 8z', fill: 'none', stroke: 'currentColor', strokeWidth: 1.3 }),
  h('circle', { cx: 8, cy: 8, r: 2, fill: on ? 'currentColor' : 'none', stroke: 'currentColor', strokeWidth: 1.3 }))

// With the view stream down nothing is known about Unfold, so it reads as not connected.
export function LinkState({ snap, t }) {
  if (!snap) return h('div', { className: 'ub-link ub-link-off', 'data-unfold-link': 'down' }, h('p', null, t('link.off')), h('p', { className: 'ub-hint' }, t('link.waiting')))
  if (snap.connected) return h('p', { className: 'ub-link ub-link-on', 'data-unfold-link': 'on' }, t('link.on'))
  return h('div', { className: 'ub-link ub-link-off', 'data-unfold-link': 'off' }, h('p', null, t('link.off')), h('p', { className: 'ub-hint' }, t('link.offHint')))
}

const Hidden = ({ points, t }) => (points.length > 0 ? h('p', { className: 'ub-marked', role: 'alert', 'data-unfold-hidden': String(points.length) }, hiddenWarning(points, t)) : null)

// Unfold's read of a pending approval. It answers nothing: the person answers on DSH's card.
export function Advice({ advice, t }) {
  return h('section', { className: 'ub-advice', 'data-unfold-advice': advice.call_id },
    h('p', { className: 'ub-label' }, t('dock.adviceLabel')),
    advice.marked ? h('p', { className: 'ub-marked', 'data-unfold-marked': 'true' }, t('dock.marked')) : null,
    h(Hidden, { points: hiddenIn(advice.note), t }),
    h('p', { className: 'ub-note' }, advice.note),
    h('p', { className: 'ub-hint' }, t('dock.adviceHint')))
}

// sidebar.workspaces.session.row.action: the watch toggle, with the Session's status dot. On
// adds a subscription for this Session; off removes the row's own subscription and no other.
export function RowAction({ snap, t, actions, sessionId }) {
  const [run, tap] = useTap()
  if (!snap) return null
  const agent = agentOf(snap, sessionId)
  const own = rowSubscription(snap, sessionId)
  const watched = own !== null || (agent !== null && watchedAgent(snap, agent))
  const label = [own ? t('row.unwatch') : t('row.watch'), agent ? statusText(agent.status, t) : null].filter(Boolean).join(' · ')
  const title = [label, !own && watched ? t('row.watchedByRule') : null, tap.error ? errorText(tap.error, t) : null].filter(Boolean).join(' · ')
  return h('button', {
    type: 'button',
    className: `ub-row${own ? ' ub-row-on' : ''}`,
    'data-unfold-row': sessionId,
    'aria-pressed': own ? 'true' : 'false',
    'aria-label': label,
    title,
    disabled: tap.busy,
    onClick: () => run(() => (own ? actions.drop(own.id) : actions.watch(sessionId))),
  }, h(Eye, { on: own !== null }), h(Dot, { status: agent?.status, watched }))
}

// sidebar.session.row.leading: an at-rest mark on a row Unfold watches (by any subscription),
// so watching shows without hovering. It only shows; the toggle is the row action.
export function WatchedMark({ snap, t, sessionId }) {
  const agent = agentOf(snap, sessionId)
  if (!snap || !(rowSubscription(snap, sessionId) !== null || (agent !== null && watchedAgent(snap, agent)))) return null
  return h('span', { className: 'ub-watched', role: 'img', 'aria-label': t('row.watchedMark'), title: t('row.watchedMark'), 'data-unfold-watched': sessionId }, h(Eye, { on: true }))
}

function Suggestion({ snap, t, actions }) {
  const [run, tap] = useTap()
  const agent = suggestion(snap)
  if (!agent) return null
  return h('section', { className: 'ub-suggest', 'data-unfold-suggest': agent.session_id },
    h('p', { className: 'ub-label' }, t('suggest.title', { name: agent.name, workspace: agent.workspace })),
    h('p', { className: 'ub-hint' }, t('suggest.body')),
    h('button', { type: 'button', className: 'ub-button', disabled: tap.busy, onClick: () => run(() => actions.watch(agent.session_id)) }, t('suggest.watch')),
    h(TapError, { code: tap.error, t }))
}

// The board: agents and their status, approvals waiting with Unfold's read, the drafts held
// (each one is read and sent in its own Session's composer), and Unfold's own changes.
export function BoardPanel({ snap, t, actions, onClose }) {
  const panel = useRef(null)
  // Opening moves focus into the board, and Escape closes it.
  useEffect(() => { panel.current?.focus() }, [])
  const agents = snap?.agents ?? []
  const notices = noticesOf(snap)
  const drafts = snap?.drafts ?? []
  return h('div', {
    ref: panel,
    className: 'ub-board',
    role: 'dialog',
    tabIndex: -1,
    'aria-label': t('board.title'),
    'data-unfold-board': '',
    onKeyDown: event => { if (event.key === 'Escape' && onClose) onClose() },
  },
    h('header', { className: 'ub-board-head' },
      h('h2', null, t('board.title')),
      onClose ? h('button', { type: 'button', className: 'ub-button ub-quiet', onClick: onClose }, t('board.close')) : null),
    h(LinkState, { snap, t }),
    snap ? h(Suggestion, { snap, t, actions }) : null,
    snap ? h('h3', null, t('board.agents')) : null,
    !snap ? null : agents.length === 0
      ? h('p', { className: 'ub-hint' }, t('board.noAgents'))
      : h('ul', { className: 'ub-agents' }, agents.map(agent => {
        const watched = watchedAgent(snap, agent)
        const pending = pendingFor(snap, agent.session_id)
        return h('li', { key: agent.session_id, className: 'ub-agent', 'data-unfold-agent': agent.session_id },
          h('div', { className: 'ub-agent-line' },
            h(Dot, { status: agent.status, watched }),
            h('span', { className: 'ub-agent-name' }, `${agent.name} · ${agent.workspace}`),
            h('span', { className: 'ub-hint' }, statusText(agent.status, t)),
            watched ? h('span', { className: 'ub-badge' }, t('board.watched')) : null),
          pending.map(approval => {
            const advice = adviceFor(snap, approval)
            return h('div', { key: approval.call_id, className: 'ub-approval', 'data-unfold-approval': approval.call_id },
              h('p', { className: 'ub-hint' }, t('board.approval')),
              advice ? h(Advice, { advice, t }) : null)
          }))
      })),
    drafts.length > 0 ? h('h3', null, t('board.drafts')) : null,
    drafts.length > 0 ? h('ul', { className: 'ub-drafts' }, drafts.map(draft =>
      h('li', { key: draft.effect_id, 'data-unfold-draft-for': draft.session_id }, t('board.draftWaiting', { name: draft.name, workspace: draft.workspace })))) : null,
    notices.length > 0 ? h('h3', null, t('board.notices')) : null,
    notices.length > 0 ? h('ul', { className: 'ub-notices' }, notices.map(entry => h(ChangeRow, { key: entry.change_id, entry, snap, t, actions }))) : null)
}

// sidebar.footer.action: the button that opens the board, with a count of what waits on the
// person (approvals and drafts).
export function BoardButton({ snap, t, actions, wide }) {
  const [open, setOpen] = useState(false)
  const waiting = (snap?.approvals?.length ?? 0) + (snap?.drafts?.length ?? 0)
  return h('div', { className: 'ub-footer' },
    h('button', {
      type: 'button',
      className: 'ub-footer-button',
      'aria-expanded': open ? 'true' : 'false',
      'aria-label': waiting > 0 ? t('board.openWaiting', { count: waiting }) : t('board.open'),
      'data-unfold-board-button': '',
      onClick: () => setOpen(!open),
    }, h(Eye, { on: Boolean(snap?.connected) }), wide ? h('span', null, t('board.open')) : null, waiting > 0 ? h('span', { className: 'ub-count', 'aria-hidden': 'true' }, String(waiting)) : null),
    open ? h(BoardPanel, { snap, t, actions, onClose: () => setOpen(false) }) : null)
}

// The nav glyph DSH's Settings shows for the Unfold page (its settings.section.icon seat):
// two arcs unfolding from the same line, in the shell's 16 px stroke style.
export const SectionIcon = ({ size = 16 }) => h('svg', { className: 'ub-icon', viewBox: '0 0 16 16', width: size, height: size, 'aria-hidden': 'true' },
  h('path', { d: 'M2 9.5c2.2-3.4 4-3.4 6 0s3.8 3.4 6 0', fill: 'none', stroke: 'currentColor', strokeWidth: 1.3, strokeLinecap: 'round' }),
  h('path', { d: 'M2 5.5c2.2-3.4 4-3.4 6 0s3.8 3.4 6 0', fill: 'none', stroke: 'currentColor', strokeWidth: 1.3, strokeLinecap: 'round', opacity: 0.55 }))

const AppIcon = () => h('svg', { className: 'ub-icon', viewBox: '0 0 16 16', width: 14, height: 14, 'aria-hidden': 'true' },
  h('rect', { x: 2, y: 2.5, width: 12, height: 11, rx: 2, fill: 'none', stroke: 'currentColor', strokeWidth: 1.3 }),
  h('path', { d: 'M8 5v5M5.8 7.8 8 10l2.2-2.2', fill: 'none', stroke: 'currentColor', strokeWidth: 1.3 }))

// The install offer, opened: Install when the browser can prompt, else Safari's own menu line.
function InstallPanel({ offer, t, onInstall, onClose }) {
  const panel = useRef(null)
  // Opening moves focus into the offer, and Escape closes it.
  useEffect(() => { panel.current?.focus() }, [])
  return h('div', {
    ref: panel,
    className: 'ub-board',
    role: 'dialog',
    tabIndex: -1,
    'aria-label': t('install.title'),
    'data-unfold-install-panel': '',
    onKeyDown: event => { if (event.key === 'Escape') onClose() },
  },
    h('header', { className: 'ub-board-head' },
      h('h2', null, t('install.title')),
      h('button', { type: 'button', className: 'ub-button ub-quiet', onClick: onClose }, t('board.close'))),
    offer.canPrompt
      ? [
        h('p', { key: 'hint', className: 'ub-hint' }, t('install.hint')),
        h('button', { key: 'install', type: 'button', className: 'ub-button ub-primary', 'data-unfold-install': '', onClick: onInstall }, t('install.button')),
      ]
      : h('p', { className: 'ub-hint', 'data-unfold-install-safari': '' }, t('install.safari')))
}

// sidebar.footer.action: Install as app, beside the board button. While the browser can prompt,
// the button prompts at once; opened from #install, the offer shows in full and takes the focus.
// Never in an installed app.
export function InstallAction({ offer, t, onInstall, wide }) {
  const [open, setOpen] = useState(offer.opened)
  if (!offerShown(offer)) return null
  return h('div', { className: 'ub-footer' },
    h('button', {
      type: 'button',
      className: 'ub-footer-button',
      'aria-expanded': open ? 'true' : 'false',
      'aria-label': t('install.open'),
      'data-unfold-install-button': '',
      onClick: () => (offer.canPrompt ? onInstall() : setOpen(!open)),
    }, h(AppIcon), wide ? h('span', null, t('install.open')) : null),
    open ? h(InstallPanel, { offer, t, onInstall, onClose: () => setOpen(false) }) : null)
}

// The held draft, in full: no clamp, no trim. Keyed by its effect id where it is used, so a
// draft that replaces it brings its own Send. A draft or recipient label holding characters
// the person can't see is still shown as held, with a warning, and Send stays off: a tap must
// send only what was seen. Cancel stays on.
export function DraftCard({ draft, t, actions }) {
  const [run, tap] = useTap()
  const to = t('dock.draftTo', { name: draft.name, workspace: draft.workspace })
  const hidden = [...hiddenIn(to), ...hiddenIn(draft.text)]
  return h('section', { className: 'ub-draft', 'data-unfold-draft': draft.effect_id },
    h('p', { className: 'ub-label' }, t('dock.draftBy')),
    h('p', { className: 'ub-to', 'data-unfold-draft-to': '' }, to),
    h(Hidden, { points: hidden, t }),
    h('div', { className: 'ub-draft-text', 'data-unfold-draft-text': '' }, draft.text),
    h('p', { className: 'ub-hint' }, hidden.length > 0 ? t('hidden.hold') : t('dock.draftHint')),
    h('div', { className: 'ub-actions' },
      h('button', { type: 'button', className: 'ub-button ub-primary', 'data-unfold-send': draft.effect_id, disabled: tap.busy || hidden.length > 0, onClick: () => { if (hidden.length === 0) run(() => actions.send(draft)) } }, tap.busy ? t('dock.sending') : t('dock.send')),
      h('button', { type: 'button', className: 'ub-button ub-quiet', 'data-unfold-cancel': draft.effect_id, disabled: tap.busy, onClick: () => run(() => actions.cancel(draft)) }, t('dock.cancel'))),
    h(TapError, { code: tap.error, t }))
}

// conversation.input.dock, per Session: Unfold's read of this Session's pending approval (DSH's
// own card stays as it is), and the draft held for this Session only.
export function Dock({ snap, t, actions, sessionId }) {
  const advice = pendingFor(snap, sessionId).map(approval => adviceFor(snap, approval)).filter(Boolean)
  const draft = draftFor(snap, sessionId)
  if (advice.length === 0 && !draft) return null
  return h('div', { className: 'ub-dock', 'data-unfold-dock': sessionId },
    advice.map(note => h(Advice, { key: note.call_id, advice: note, t })),
    draft ? h(DraftCard, { key: draft.effect_id, draft, t, actions }) : null)
}

function SubscriptionRow({ sub, snap, t, actions }) {
  const [run, tap] = useTap()
  const badge = sub.added_by === 'unfold' ? t('settings.byUnfold') : sub.added_by === 'person' ? t('settings.byYou') : null
  return h('li', { className: 'ub-sub', 'data-unfold-sub': sub.id },
    h('span', null, describeSubscription(sub, snap, t)),
    badge ? h('span', { className: 'ub-badge', 'data-unfold-by': sub.added_by }, badge) : null,
    h('button', { type: 'button', className: 'ub-button ub-quiet', disabled: tap.busy, onClick: () => run(() => actions.drop(sub.id)) }, t('settings.remove')),
    h(TapError, { code: tap.error, t }))
}

function ChangeRow({ entry, snap, t, actions }) {
  const [run, tap] = useTap()
  const at = when(entry.at, t)
  return h('li', { className: 'ub-change', 'data-unfold-change': entry.change_id },
    at ? h('time', { className: 'ub-hint', dateTime: new Date(entry.at).toISOString() }, at) : null,
    h('span', null, describeChange(entry, snap, t)),
    h('button', { type: 'button', className: 'ub-button ub-quiet', disabled: tap.busy, onClick: () => run(() => actions.undo(entry.change_id)) }, t('settings.undo')),
    h(TapError, { code: tap.error, t }))
}

// settings.section: the link state, every subscription with who added it, and the change log
// with Undo on each change. Until Unfold's view has arrived (or while the stream is down) the
// lists are unknown, not empty, so only the link state shows.
export function SettingsSection({ snap, t, actions }) {
  const head = [
    h('h2', { key: 'title' }, t('settings.title')),
    h('p', { key: 'intro', className: 'ub-hint' }, t('settings.intro')),
    h('h3', { key: 'link' }, t('settings.link')),
    h(LinkState, { key: 'state', snap, t }),
  ]
  if (!snap?.view) return h('div', { className: 'ub-settings', 'data-unfold-settings': '' }, head)
  const subs = snap.view.subscriptions ?? []
  const changes = changesOf(snap)
  return h('div', { className: 'ub-settings', 'data-unfold-settings': '' },
    head,
    h('h3', null, t('settings.subscriptions')),
    subs.length === 0
      ? h('p', { className: 'ub-hint' }, t('settings.noSubscriptions'))
      : h('ul', { className: 'ub-subs' }, subs.map(sub => h(SubscriptionRow, { key: sub.id, sub, snap, t, actions }))),
    h('h3', null, t('settings.log')),
    changes.length === 0
      ? h('p', { className: 'ub-hint' }, t('settings.noLog'))
      : h('ul', { className: 'ub-changes' }, changes.map(entry => h(ChangeRow, { key: entry.change_id, entry, snap, t, actions }))))
}
