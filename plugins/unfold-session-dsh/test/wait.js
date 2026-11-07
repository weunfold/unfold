// Every wait in the tests is bounded by a ref'd timer and fails with a named error, so a missing
// event fails its test instead of hanging the run, or ending it early on a runtime that exits
// when only unref'd handles are left.
export class TestWaitTimeout extends Error {
  name = 'TestWaitTimeout'
}

/** `promise`, or a TestWaitTimeout naming `what` after `ms`. */
export function within(promise, ms, what) {
  let timer
  const bound = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new TestWaitTimeout(`${what}: not done within ${ms} ms`)), ms)
  })
  return Promise.race([promise, bound]).finally(() => clearTimeout(timer))
}
