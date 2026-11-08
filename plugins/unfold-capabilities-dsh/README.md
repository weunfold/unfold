# Unfold capabilities (`@automagik/unfold-capabilities-dsh`)

"What I can do" for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (DSH): how much Unfold can do now, and how well, per capability, modality and benchmark row. The map is computed only from bench scorecards joined to their manifests; this plugin judges nothing itself. It is read-only: it writes nothing and opens no connection of its own. Tested with DSH 0.2.1-rc.4.

## What the map says

- A cell's status is `supported`, `proxy` (a stand-in only), `failing`, `unverified` or `unsupported`, with a reason code. A cell without evidence is `unsupported` (`not_evaluated`), never assumed.
- A run counts only when it is verified (its manifest names the scorecard's suite, scorer and candidate and hashes every scored task), eligible, and inside the registry's comparison cohort (suite id, sha and epoch, scorer id and version, selection split, exact `task@version`, the same task hashes), and its candidate meets the cell's prerequisites. Every other run is listed apart, with why.
- The newest such run is the latest measured candidate: a measurement, never proof of what is deployed.

## Config

| Key | Default | What it is |
|---|---|---|
| `registry` | none | Path to the capability registry (`registry.json`, format `unfold.capabilities/v2`). Without it the map is empty and says `no capability registry is configured`. |
| `results` | none | Path to the bench results, one folder per run holding `scorecard.json` (`unfold-bench.scorecard/v1`) and `manifest.json` (`unfold-bench.run-manifest/v1`). |
| `watch` | `false` | Also recompute on `fs.watch` events. Best effort: on macOS it delivered no events in testing, so it is not the trigger. |
| `route` | `/api/unfold-capabilities` | The routes' base. |
| `transport` | `connection` | `connection`: DSH's `/api` channel (its Host/Origin fence and browser auth). `webServer`: a bare route, for hosts without Connection. |

## Routes

| Route | Method | What it does |
|---|---|---|
| `<route>` | GET | The map. |
| `<route>/stream` | GET | Server-sent events: the map, then each new map. |
| `<route>/refresh` | POST | Recomputes the map from disk and pushes it to every open stream; a bench run calls it when it lands. It writes nothing. |

The service is `unfoldCapabilities` (`map()`, `refresh()`, `onChange(listener)`). The browser half is a main panel with a sidebar entry, `unfold.capabilities`, that follows the stream.

## Tests

```sh
bun run test
bun run lint   # oxlint from dev/harness (bun run harness), or DSH_HARNESS
node scripts/build-client.mjs --check
```

`src/sse.js` is `@automagik/unfold-sse-dsh`'s `index.js`, carried byte for byte; a test holds it to that file's hash.

## Licence

MIT (see LICENSE).
