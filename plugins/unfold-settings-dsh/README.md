# Unfold settings (`@automagik/unfold-settings-dsh`)

"Unfold settings" for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH), read-only: how Unfold is set up right now. Backend and model, the prompt and config revision, voice and device state and the model context, each with its source and how fresh it is. It has no write route and opens no connection of its own. Tested with DSH 0.2.1-rc.4.

## Where the values come from

It reads three services other plugins provide; a missing one makes its values unknown, never a default:

| Service | Plugin | What is read |
|---|---|---|
| `unfoldSession` | `@automagik/unfold-session-dsh` | `status().owner`, the host owner's status. Where the session plugin offers no `status()` (Unfold's own control endpoint carries only the person's submit), every host value is unknown and says why. An owner it reports unavailable is read once, and again only when unfold-session reloads. |
| `unfoldEvents` | `@automagik/unfold-events-dsh` | The observer's event window: the Hello, the `gemini ready` log line and `context_restore`. No second observer attaches to the agent. |
| `omniTeam` | the Bot team | `config.memoryReview` only. |

The effective prompt is never on a wire. The panel reads the policy text from the agent's `gemini.rs` (a constant tagged `[<name>-voice-policy <id>]`) and shows it only when a replica of the agent's setup message over that text and the ready line's knobs reproduces the line's `setup_id`; the context bounds come from the agent's `context.rs`. Both paths come from config; with none, the prompt is never shown and the bounds are unknown.

## Freshness

Every value is `live`, `stale`, `unknown` or `source` (a constant read from the agent's source). Host values expire at their lease; event-derived values need the stream's confirmation (views and heartbeats); a value seen before an observer gap is history until it is seen again. The browser expires what it shows on its own clock, so a silent or lost stream never keeps a value looking live. GET and the stream share one snapshot identity (the serving instance and a rising revision), so a late Refresh never undoes a newer view. Everything served passes through `redact`.

## Config

| Key | Default | What it is |
|---|---|---|
| `route` | `/api/unfold-settings` | The routes' base: `GET <route>` (one host read, then the view) and `GET <route>/stream` (the view, then each change, with heartbeats). |
| `leaseMs` | `15000` | A host status read older than this is stale. |
| `hostReadTimeoutMs` | `5000` | A host read that takes longer fails. |
| `heartbeatMs` | `5000` | Stream heartbeat while a settings stream is open; the browser treats three missed beats as a lost stream. |
| `sources.gemini` | none | Path to the agent's `gemini.rs` (the policy text). |
| `sources.context` | none | Path to the agent's `context.rs` (the fold bounds). |
| `transport` | `connection` | `connection`: DSH's `/api` channel (its Host/Origin fence and browser auth). `webServer`: a bare route, for hosts without Connection. |

The service is `unfoldSettings`. The browser half is a main panel with a sidebar entry (`unfold.settings`) and the same panel as a right-sidebar tab, for surfaces that hide the left panel list.

## Tests

```sh
bun run test
bun run lint   # oxlint from dev/harness (bun run harness), or DSH_HARNESS
node scripts/build-client.mjs --check
```

`src/sse.js` is `@automagik/unfold-sse-dsh`'s `index.js`, carried byte for byte; a test holds it to that file's hash. The test fixtures stand in for the agent's source files and a host status; they hold no real values.

## Licence

MIT (see LICENSE).
