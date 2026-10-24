// Preloaded into DSH's own process by shell/app.mjs (`node --import <this> <dsh bin>`): a prompt exit
// on SIGTERM/SIGINT. DSH's shutdown disposes every fs watcher it opened (chokidar on the skill roots,
// ~/.agents/skills at depth 1, the profile and credentials files, the Mind journal tail: about 140
// with 49 skills). On macOS each fs.watch close goes through libuv's FSEvents teardown, which blocks
// the event loop until the stream is rescheduled: measured 1.6 s for 6 watchers and 61-83 s for 140
// (node 24.13.1, macOS 26). The loop never ran again, so DSH's own 5 s forced exit never fired, and
// app.mjs's 10 s SIGKILL ended it every time.
//
// Scope, so nothing but DSH's terminal shutdown changes:
// - only in DSH's own process (argv[1] is @deepseek-ai/dsh/lib/bin.js); a no-op anywhere else;
// - not inherited: removed from process.execArgv at load, so fork()ed children and workers start
//   without it (app.mjs passes it as an argument, never through NODE_OPTIONS);
// - only after a quit signal that DSH itself handles. In DSH that signal is the terminal shutdown:
//   lib/profile-boot-*.js installs SIGTERM/SIGINT -> interrupt(), and createProcessShutdown's
//   interrupt() always ends in forceExitOnce() (after dispose, or at its 5 s timer). Then a watcher
//   close only unrefs and reports 'close': the process is about to exit and the OS releases it.
// A signal that arrives before DSH has a handler of its own keeps its default action.
import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'

const SELF = import.meta.url
for (let i = process.execArgv.length - 1; i >= 0; i--) {
  const a = process.execArgv[i]
  if ((a === '--import' || a === '--experimental-loader' || a === '--loader') && process.execArgv[i + 1] === SELF) process.execArgv.splice(i, 2)
  else if (a === `--import=${SELF}`) process.execArgv.splice(i, 1)
}

let isDsh = false
try { isDsh = /\/@deepseek-ai\/dsh\/lib\/bin\.js$/.test(fs.realpathSync(process.argv[1] ?? '')) } catch {}

if (isDsh) {
  let exiting = false
  for (const sig of ['SIGTERM', 'SIGINT']) {
    const onQuit = () => {
      if (process.listenerCount(sig) > 1) {
        exiting = true
        return
      }
      process.off(sig, onQuit)
      process.kill(process.pid, sig)
    }
    process.prependListener(sig, onQuit)
  }
  // FSWatcher is not exported: patch its prototype from the first watcher fs.watch returns.
  let patched = false
  const watch = fs.watch
  fs.watch = function (...args) {
    const w = watch.apply(this, args)
    if (!patched) {
      patched = true
      const proto = Object.getPrototypeOf(w)
      const close = proto.close
      proto.close = function (...a) {
        if (!exiting) return close.apply(this, a)
        if (this.rthClosed) return
        this.rthClosed = true
        this.unref()
        process.nextTick(() => this.emit('close'))
      }
    }
    return w
  }
  // `import { watch } from 'node:fs'` sees the wrapper too.
  syncBuiltinESMExports()
}
