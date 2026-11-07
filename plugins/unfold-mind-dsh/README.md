# Unfold Mind (`@automagik/unfold-mind-dsh`)

The Mind stream for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH): every event Unfold's agent and host emit, as far as its sources carry them, becomes one plain sentence, grouped into episodes through explicit ids only. Each row opens an audit record (ids, cause chain, source, integrity, catalog definition, redacted envelope), and gap markers show wherever the evidence is incomplete. It shows and never commands: the only write is a suggestion, appended to a local log that goes nowhere. Tested with DSH 0.2.1-rc.4.

## Sources

All read-only. Mind opens no connection of its own.

| Source | Where it comes from |
|---|---|
| Agent events | The `unfoldEvents` service of `@automagik/unfold-events-dsh`: its event window and link state, the one observer attachment to agentd. |
| Host journal | `journals.host`, or, with `journals.hostFromInstance`, the path in `UNFOLD_HOST_JOURNAL`. Off by default. |
| Agent journal | `journals.agentd`: the instance's agentd journal, for file offsets and history from before Mind loaded. Off by default. |

### The host journal binding

`UNFOLD_HOST_JOURNAL` is part of the Unfold channel contract: the Unfold app will set it on its instance child to `<data>/app/<label>/host.jsonl`, under the app's own dir, and the launcher preload reads and deletes it and hands it to plugins as `hostJournal` in its registry (`globalThis[Symbol.for('unfold.channel')]`). The name is reserved and unset until the app writes that journal, so the host lane stays off until then (unless `journals.host` names a file). Mind takes the value once, at its first load: from the registry's `hostJournal` key when the registry exists (and only that key), from the environment only when there is no registry. It deletes the variable either way, so tools DSH spawns later never inherit it. A later load keeps that first value.

The value is parsed, never opened by the binding: a path that is not exactly `<data>/app/<label>/host.jsonl` (an absolute path, a label of 1-64 letters, digits, `.`, `_` or `-`) is refused, and so is any path with a `..` segment; the host journal stays off with the reason in the snapshot's `sources`. Mind never falls back to another log.

Every journal is read only where it really is: a file that is a symbolic link is refused, and for the bound host journal so is a link at `<data>/app` or `<data>/app/<label>`. The source then reads `refused`, with the reason.

## Routes

On DSH's `connection.fetch` only (its Host/Origin fence and browser auth):

| Route | What it serves |
|---|---|
| `GET /api/unfold-mind` | The snapshot: rows, episodes, link, sources. |
| `GET /api/unfold-mind/stream` | Server-sent events: the snapshot, then `row`, `row-updated`, `episode`, `merged`, `evicted`, `link`, heartbeats. |
| `GET /api/unfold-mind/event?id=<id>` | The audit record of one row. |
| `GET`, `POST /api/unfold-mind/suggestions` | The suggestion log; `POST` appends one `{key, actor, scope: {episode, event, title}, text}`. |

Every payload is redacted at the boundary. Suggestions are stored in `suggestions.jsonl` in the plugin's data dir (`dataDir`, default `<DSH_HOME>/unfold-mind`).

## Browser half

`client.js` adds the Mind tab to DSH's right sidebar. It is built from `src/client` with esbuild and committed:

```sh
bun run build                          # writes client.js
node scripts/build-client.mjs --check  # fails when client.js is stale
```

## What it carries

- `src/sse.js` is `@automagik/unfold-sse-dsh`'s `index.js`, byte for byte; `test/sse-copy.test.js` holds the copy to it.
- `catalog/events.v3.json` is the event catalog `@automagik/unfold-events-dsh` carries.
- `test/support/` holds that package's event window and catalog reader, so the tests run Mind on exactly what it emits.

## Tests

```sh
bun run harness   # the DSH packages the tests and the linter run against, in dev/harness
bun run test
bun run lint
```

## Licence

MIT (see LICENSE).
