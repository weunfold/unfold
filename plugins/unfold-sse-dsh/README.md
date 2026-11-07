# Unfold server-sent events (`@automagik/unfold-sse-dsh`)

The server-sent events hub the Unfold plugins for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH) stream with. A stream is a finite baseline (a snapshot) followed by increments. It is a library: it registers nothing in DSH, opens no port and reads no environment.

## Interface

| Export | What it does |
|---|---|
| `sseHub(options?)` | A hub of streams. `open(req, res, event, data, id)` starts one stream with its baseline; `send(event, data, id)` writes one event to every stream, serialized once; `close(reason = 'unloaded')` ends every stream (plugin unload). `clients` is the live set and `drops` counts the clients dropped, by reason. |
| `fetchStream(hub, request, event, data, id, { highWaterMark }?)` | The same stream as a Fetch `Response`, for DSH's `connection.fetch` routes (the `/api` channel behind its Host/Origin fence and browser auth). An abort of `request.signal`, a consumer cancel or a hub drop each settle the body and remove the client. |
| `frame(event, data, id?)` | One event as SSE text: an `id:` line only when an id is given, then `event:` and `data:` (JSON). |
| `SSE_HEADERS` | The response headers of a stream. |
| `DEFAULTS` | `{ maxPendingBytes: 1 MiB, maxPendingEvents: 10000, drainTimeoutMs: 5000 }`. |

## Backpressure and drops

`res.write` returning `false` is ordinary backpressure, not a dead reader: later events queue until `'drain'`. A client is dropped only when a bound is exceeded: more than `maxPendingBytes` or `maxPendingEvents` queued behind it, or no `'drain'` within `drainTimeoutMs`. A client still waiting for drain is cut; any other gets a last `resnapshot` event with the reason. A reader that reconnects starts from a fresh snapshot. Other clients, and whatever feeds the hub, never wait.

## How a plugin carries it

Each Unfold plugin installs on its own, so a plugin that streams carries a copy of `index.js` as its own file, and a test in that plugin holds the copy byte for byte to this package's `index.js`. Change the hub here, then copy it.

## Tests

```sh
bun run test
bun run lint   # oxlint from dev/harness (bun run harness), or DSH_HARNESS
```

Tested with DSH 0.2.1-rc.4.

## Licence

MIT (see LICENSE).
