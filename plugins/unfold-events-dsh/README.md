# Unfold events (`@automagik/unfold-events-dsh`)

What Unfold's agent is doing, inside the [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH) of Unfold's own instance. It adds two right-sidebar tabs:

- **Unfold events**: every event the agent sends, newest first, each named in plain words from the event catalog (`catalog/events.v3.json`). A kind the catalog doesn't name yet is shown as it came and flagged, never hidden.
- **Unfold**: the presence, one creature that says what the agent is doing right now (idle, pending, replying, speaking, asleep or unknown), with a production queue of what is running. Every pose cites its source; a missing or stale source shows as unknown, never as a guess.

It only watches. It attaches to the agent as a read-only **observer**, so the agent refuses anything it would send, and it drops the link if it is granted any other role.

## Where the agent is

Nothing here names an address or a port. In Unfold's instance the app publishes the agent's address in a file (one line, a literal loopback address with a port) only while its own chat holds the agent, and removes the file before it lets go. Unfold's launcher passes that file's path as `UNFOLD_AGENTD_OBSERVER`; the launcher's preload reads the variable once, deletes it from the environment before any plugin loads, and hands the path to in-process plugins as `globalThis[Symbol.for('unfold.channel')].observerFile`. This plugin reads the path from there only, never from the environment.

The file is read again at every connect. While it is absent the plugin waits, with backoff, and dials nothing; a file that holds anything but a literal loopback address with a port is refused in one line. Outside an Unfold instance there is no channel, and the tabs say so.

What the source can't show is stated in each snapshot (`source`): the agent never broadcasts the host's own input to observers, and a reconnect resumes live without filling the gap.

## Routes

Behind DSH's `/api` channel (`connection.fetch`: its Host/Origin fence and browser auth):

| Route | What it serves |
|---|---|
| `GET /api/unfold-events` | the window: `{rev, rows, aggregates, status, link, source, dropped, unknown}` |
| `GET /api/unfold-events/stream` | that snapshot, then every change, as server-sent events |
| `GET /api/unfold-presence` | the presence: `{rev, view}` |
| `GET /api/unfold-presence/stream` | that view, then each changed view, and a heartbeat |

The voice controls on the presence tab read and request through the Unfold session plugin's routes (`api/unfold-session/call`, `api/unfold-session/mute`); without that plugin voice stays unknown. The microphone indicator reads the host's status through its `unfoldSession` service.

## Settings

| Setting | Default | |
|---|---|---|
| `limit` | 2000 | rows kept in the window |
| `agentd.enabled` | true | attach through the observer file; there is no address to set |
| `presence.host` | true | read the host's status for the microphone indicator |
| `presence.heartbeatMs` | 10000 | presence stream heartbeat |
| `stream.*` | 1 MiB, 10000, 5000 ms | per-client bounds of the server-sent events hub |

## Development

```sh
bun install && bun run harness
bun run test
bun run lint
node scripts/build-client.mjs   # after a change in src/client; commit client.js
```

Tested with DSH 0.2.1-rc.4.

`src/sse.js` is a copy of [`@automagik/unfold-sse-dsh`](../unfold-sse-dsh/README.md), held byte for byte by `test/sse-copy.test.js`. `src/client/redact.js` and the patterns in `src/window.js` are the credential masking the Unfold plugins share; a test holds the copies to one answer.

## Licence

MIT (see LICENSE).
