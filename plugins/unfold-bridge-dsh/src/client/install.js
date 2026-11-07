// The install offer: DSH's page as an app of its own. Chrome and Edge fire beforeinstallprompt
// when the page can be installed; the event is held, and its prompt() runs only on the person's
// click, once per event. Safari has no such event, so there the offer is one line for its own
// menu. An installed app never shows the offer. Unfold opens DSH's address with #install to open
// the offer at load; a browser that has the event opens it when the event fires, and shows
// nothing before (it may not be installable, or be installed already from another window).
export const INSTALL_HASH = '#install'

const DISPLAY_MODES = ['(display-mode: standalone)', '(display-mode: fullscreen)']
const isInstalled = win => win?.navigator?.standalone === true || DISPLAY_MODES.some(query => win?.matchMedia?.(query)?.matches === true)

// The offer's state for the slot: whether the browser can prompt now, whether it has the event at
// all, whether it is installed, and whether the address asked for it. `win` is the page's window
// (undefined outside a browser).
export function createInstall(win) {
  let held = null
  let snap = { canPrompt: false, installed: isInstalled(win), opened: win?.location?.hash === INSTALL_HASH, prompts: win !== undefined && win !== null && 'onbeforeinstallprompt' in win }
  const listeners = new Set()
  const set = next => {
    snap = { ...snap, ...next }
    for (const fn of Array.from(listeners)) fn()
  }
  const onPrompt = event => {
    event.preventDefault?.()
    held = event
    set({ canPrompt: true })
  }
  const onInstalled = () => {
    held = null
    set({ canPrompt: false, installed: true })
  }
  let watching = false
  return {
    get: () => snap,
    subscribe(fn) {
      listeners.add(fn)
      return () => listeners.delete(fn)
    },
    // Listens until the returned stop; watching again while watching listens once, and any stop
    // stops all (the bundle's own watch at load and apply's effect share the listeners).
    watch() {
      if (!watching && win?.addEventListener) {
        win.addEventListener('beforeinstallprompt', onPrompt)
        win.addEventListener('appinstalled', onInstalled)
        watching = true
      }
      return () => {
        if (!watching) return
        win.removeEventListener?.('beforeinstallprompt', onPrompt)
        win.removeEventListener?.('appinstalled', onInstalled)
        watching = false
      }
    },
    // The person's click. The event is spent at once, so a second click prompts nothing.
    prompt() {
      const event = held
      if (!event) return undefined
      held = null
      set({ canPrompt: false })
      // A browser that refuses the prompt leaves nothing to show.
      try { return Promise.resolve(event.prompt()).catch(() => undefined) } catch { return undefined }
    },
  }
}

// Shown while the browser can prompt, or while the address asked for it in a browser without the
// event (Safari's line), until the app is installed.
export const offerShown = offer => !offer.installed && (offer.canPrompt || (offer.opened && !offer.prompts))
