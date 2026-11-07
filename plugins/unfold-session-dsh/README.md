# Unfold session (`@automagik/unfold-session-dsh`)

Unfold's rolling session as one root Session of the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH) of Unfold's own instance. DSH is a view and a composer here, never a second brain:

- **Typed turns.** When you send a message in the `unfold-root` Session's composer, the plugin holds DSH's turn, sends your text once to Unfold's control endpoint, and Unfold's app sends it to the agent on the Unfold Session as a typed command. The reply streams back from the agent as a chat bubble. DSH's loop then ends the turn without a model step: no model call, no provider.
- **Spoken turns** are shown, never sent: a reply the agent's voice provider opened is followed by its own ids and drawn as a user bubble (the words heard for it) and a reply bubble.
- **Live listening**: a "listening…" bubble above the composer from the moment speech starts until its words land in a chat bubble.
- **Unfold status**: a right-sidebar tab with the link to the agent and the latest measured candidate.

The takeover is enabled only when `AUTOMAGIK_HOST=unfold` or `AUTOMAGIK_PRELOAD=unfold`, matching the Bot team and welcome. In that instance every other root Session is refused: DSH runs no root loop anywhere. On ordinary hosts this plugin leaves Sessions, agent hooks and the browser UI untouched; its stable configuration endpoint reports `enabled: false`.

## What is sent, and how

Only your own composer message is ever sent. Nothing an event says (a transcript, a reply, a note, a turn another composer sent) reaches the endpoint, and no DSH route of this plugin leads to it.

The endpoint is a Unix socket in the instance's own 0700 socket dir, with a bearer token made at each start. Unfold's launcher passes both as `UNFOLD_CONTROL_SOCKET` and `UNFOLD_CONTROL_TOKEN_FILE`; the launcher's preload reads them once, deletes them from the environment and unlinks the token file before any plugin loads, and hands the socket path and the token to in-process plugins as `globalThis[Symbol.for('unfold.channel')]`. This plugin reads them from there only. It opens no TCP connection for control, names no port, and never reads the token file. A tool or agent DSH spawns sees neither variable.

The endpoint has three verbs. `POST /submit {session, text}` sends a typed turn:

| Answer | What the chat shows |
|---|---|
| 200 `{accepted: true, turn_id}` | the reply, as it streams; its bubble names `turn_id`, the command id agentd admitted |
| 400 `command` | a message that starts with `/` is an owner command, which only Unfold's own keyboard sends; the plugin refuses it before it connects |
| 401, 403, 400 `body`, 503, a refused peer, no endpoint | not sent, and why |
| the connection failed after the message was written | outcome unknown: it may have been sent |

A message is sent at most once: no answer is ever retried, and a message the composer delivers again is never sent again. There is no verb to cancel a reply, so the stop button ends the turn here and says the outcome in Unfold is unknown unless the agent confirms it. After an observer gap a reply keeps the text it saw and is marked possibly incomplete; there is no recorded result to take its whole text from.

`GET /answer-with` and `POST /answer-with {choice?, effort?, ack?}` are what Unfold answers with, the same setting as Unfold's own settings: the Unfold Bot's model menu in DSH (drawn by the Bot team, registered by `@automagik/unfold-bot-dsh`) lists only the options the app returns, with where each message goes, the one picked, the one answering now and, for an option with levels, the app's levels. A pick answers `{rev, after_reply}`; while the app waits for the reply in progress the menu says it switches after this reply. 400 `body`, 409 `not_offered` and 409 `needs_ack` (an option the person has to confirm first) come back as the pick's refusal. The plugin reads the facts at start, when the agent link attaches again (a model change restarts the agent), when the menu opens, after a pick and when a typed turn ends; it never polls. The service carries it as `answerWith`.

Without a channel (a DSH that Unfold's launcher did not start), or before the agent names its Unfold Session, nothing is sent and the chat says so.

## Where the agent is

agentd's address is never configured. The app publishes it in the observer file only while its own chat holds the agent; the plugin reads that file at every connect, attaches as a read-only **observer**, and drops the link on any other grant. While the file is absent it waits, with backoff, and dials nothing. An address that is not a literal loopback address with a port is never dialled.

## Routes

Behind DSH's `/api` channel (`connection.fetch`: its Host/Origin fence and browser auth):

| Route | What it serves |
|---|---|
| `GET /api/unfold-session/status` | the link, the counters and the latest measured candidate |
| `GET /api/unfold-session/live` | the listening view, then each change, as server-sent events |
| `GET /api/unfold-session/call` | `{unavailable}`: in Unfold's instance calls belong to Unfold's own app |
| `POST /api/unfold-session/call`, `POST /api/unfold-session/mute` | 503: the endpoint has no call or mute verb |

The service `unfoldSession` (`bridge`, `turns`, `live`, `status()`) is what the Unfold events and settings plugins read. In Unfold's instance the host's own status is not readable through the control endpoint, so `status().owner` is always `{unavailable: <reason>}`, and those plugins show the call, the microphone and the voice as unknown.

`src/call-view.js` is a copy of the call view in [`@automagik/unfold-events-dsh`](../unfold-events-dsh/README.md), held byte for byte by `test/sse-copy.test.js`.

## Settings

| Setting | Default | |
|---|---|---|
| `sessionId` | `unfold-root` | the one DSH root Session that shows Unfold |
| `title` | `Unfold` | its workspace title |
| `workspaceDir` | `<DSH_HOME>/unfold-workspace` | its workspace directory |
| `blockOtherSessions` | true | refuse turns in every other root Session |
| `route` | `/api/unfold-session` | the routes' base |

## Development

```sh
bun install && bun run harness
bun run test
bun run lint
node scripts/build-client.mjs   # after a change in src/client; commit client.js
```

The tests run against a fake control endpoint (`test/fake-control.js`) on a Unix socket under `/tmp/ufd-<uid>/`, removed by its path, and against fake agentd observers on kernel-picked ports; every other TCP connect is refused before it is made.

`src/sse.js` is a copy of [`@automagik/unfold-sse-dsh`](../unfold-sse-dsh/README.md), held byte for byte by `test/sse-copy.test.js`.

## Licence

MIT (see LICENSE).
