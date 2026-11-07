// Ports for the tests. A port the code under test must bind itself can't be held for it: it is
// picked by the kernel and released first, and another process can take it in between, so
// `onFreePort` runs its attempt again on a new port when the bind fails with EADDRINUSE. A port a
// test needs taken is held by the test's own listener (`hold`) until the test closes it.
import { createServer } from 'node:http'

const pickPort = () => new Promise((resolve, reject) => {
  const probe = createServer()
  probe.once('error', reject)
  probe.listen(0, '127.0.0.1', () => {
    const { port } = probe.address()
    probe.close(() => resolve(port))
  })
})

export async function onFreePort(attempt, tries = 5) {
  for (let i = 1; ; i++) {
    const port = await pickPort()
    try {
      return await attempt(port)
    } catch (error) {
      if (error?.code !== 'EADDRINUSE' || i >= tries) throw error
    }
  }
}

// A listener on `port` (any free one when 0); it rejects when the port can't be held.
export const hold = (port = 0) => new Promise((resolve, reject) => {
  const holder = createServer()
  holder.once('error', reject)
  holder.listen(port, '127.0.0.1', () => {
    holder.off('error', reject)
    resolve({ port: holder.address().port, close: () => new Promise(done => holder.close(done)) })
  })
})
