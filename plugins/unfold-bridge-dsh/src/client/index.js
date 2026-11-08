import { createElement as h, useSyncExternalStore } from 'react'
import { cancelDraft, dropSubscription, openView, sendDraft, undoChange, watchSession } from './api.js'
import { languageOf, translator } from './i18n.js'
import { createInstall } from './install.js'
import { BoardButton, Dock, InstallAction, RowAction, SectionIcon, SettingsSection, WatchedMark } from './slots.js'
import { installStyles } from './styles.js'

// The DSH client half of the Unfold bridge: it renders what the bridge's view stream carries
// and turns the person's taps into the bridge's admitted client routes. It holds no key, and a
// draft's token only to send or cancel that draft. Unfold's read of an approval goes in the
// requesting Session's composer dock: DSH's approval card, with its command preview, keeps its
// own detail slot, so nothing here registers in `conversation.approval.detail`.
export const SLOTS = [
  ['sidebar.workspaces.session.row.action', { id: 'unfold-watch', order: 300 }, RowAction],
  // DSH mounts this seat only on an idle row, in place of the row's own state dot.
  ['sidebar.session.row.leading', { id: 'unfold-watched', order: 100 }, WatchedMark],
  ['sidebar.footer.action', { id: 'unfold-board', order: 100 }, BoardButton],
  ['conversation.input.dock', { id: 'unfold-dock', order: 50 }, Dock],
  // The nav label is "Unfold" in every language, so it never needs registering again on a
  // locale change.
  ['settings.section', { id: 'unfold-bridge', order: 900, label: 'Unfold' }, SettingsSection],
  // DSH 0.2.2 renders a nav icon for a section whose id matches this seat's registration.
  ['settings.section.icon', { id: 'unfold-bridge', order: 900 }, SectionIcon],
]

// DSH's page as an app (src/client/install.js), beside the board button.
export const INSTALL_SLOT = ['sidebar.footer.action', { id: 'unfold-bridge.install', order: 110 }]

export const inject = ['slots']

// Made when DSH loads this bundle, before apply, so a beforeinstallprompt the browser fires
// early is still held.
const install = createInstall(globalThis.window)
install.watch()

// The latest snapshot, for every slot at once.
function createStore() {
  let snap = null
  const listeners = new Set()
  return {
    get: () => snap,
    set(next) {
      snap = next
      for (const fn of Array.from(listeners)) fn()
    },
    subscribe(fn) {
      listeners.add(fn)
      return () => listeners.delete(fn)
    },
  }
}

export function apply(ctx) {
  const store = createStore()
  const locale = ctx.get?.('locale')
  const language = () => languageOf(locale?.getLocale?.().active)
  const onLocale = fn => locale?.subscribe?.(fn) ?? (() => {})
  const actions = { send: sendDraft, cancel: cancelDraft, watch: watchSession, drop: dropSubscription, undo: undoChange }
  const bind = View => function UnfoldSlot(props) {
    const snap = useSyncExternalStore(store.subscribe, store.get)
    const t = translator(useSyncExternalStore(onLocale, language))
    return h(View, { ...props, snap, t, actions })
  }
  ctx.effect(installStyles, 'unfold-bridge: client styles')
  ctx.effect(() => openView(snap => store.set(snap)), 'unfold-bridge: the view stream')
  ctx.effect(() => install.watch(), 'unfold-bridge: the install offer')
  for (const [name, options, View] of SLOTS) ctx.slots.inject(name, () => ctx.slots.register({ name, ...options }, bind(View)))
  const [slot, slotOptions] = INSTALL_SLOT
  ctx.slots.inject(slot, () => ctx.slots.register({ name: slot, ...slotOptions }, function UnfoldInstall(props) {
    const offer = useSyncExternalStore(install.subscribe, install.get)
    const t = translator(useSyncExternalStore(onLocale, language))
    return h(InstallAction, { ...props, offer, t, onInstall: install.prompt })
  }))
}
