# Unfold plugins for DeepSeek Harness

These nine packages are optional. Neither the repository's shell profiles nor
the application installs them automatically. The Bot artwork and Bot team
runtime belong to Automagik and are not included here.

| Package | Purpose | Licence |
| --- | --- | --- |
| `@automagik/unfold-bot-dsh` | Unfold on an existing Bot team | Apache-2.0 |
| `@automagik/unfold-bridge-dsh` | An authenticated loopback model bridge | Apache-2.0 |
| `@automagik/unfold-capabilities-dsh` | A capability map from measured results | MIT |
| `@automagik/unfold-events-dsh` | Agent events and presence | MIT |
| `@automagik/unfold-mind-dsh` | Events grouped into episodes | MIT |
| `@automagik/unfold-session-dsh` | The app's conversation in DSH | MIT |
| `@automagik/unfold-settings-dsh` | Read-only effective settings | MIT |
| `@automagik/unfold-sse-dsh` | The shared server-sent events library | MIT |
| `@automagik/unfold-trajectory-dsh` | Conversation timing and event inspection | MIT |

Read each package's README for its DSH version range and required services.
The modern suite and the legacy shell use different DSH versions. The session
and event views require the channel supplied by an Unfold app instance; an
ordinary DSH instance cannot provide it by setting an agent address.

## Install

Choose a package tarball from an `unfold-dsh-vX.Y.Z` GitHub Release, verify its
checksum and explicitly install it through DSH's plugin manager. Git sources
must pin a release commit and select the package subdirectory. The repository
root is not an installable DSH package.

Installing the optional Bot makes its companion-plugin list available to its
host. The Bot team must already be installed separately. It does not copy the
Automagik pet into this repository.

## Develop

Use Node 24.9 or later. Each package contains its own pinned development harness:

```sh
cd plugins/unfold-events-dsh
bun run harness
bun run test
bun run lint
```

Alternatively set `DSH_HARNESS` to an existing compatible DSH installation.
Build scripts also require the package's declared developer dependencies.
The tests use synthetic local services and do not call a model. Tests with
additional CI-only cells report those cells as skipped in an ordinary run.

The plugin CI installs each package independently with Bun 1.4.4 and Node
24.13.1, installs its development harness, builds any browser client, and runs
its Node tests with four test workers. The Bot and SSE packages have no browser
client build. CI also verifies that rebuilding leaves committed bundles unchanged.

Run the shared copy and delivery checks from the repository root:

```sh
bun test scripts/unfold-copies.test.ts scripts/sse-copies.test.ts scripts/plugin-delivery.test.ts
```

The shared redaction performance helper lives in `scripts/redact-fuzz-guard.mjs`.
Changes to it run every plugin's CI row.

After changing browser source, run that package's build command and commit its
`client.js`. The trajectory package retains its upstream source and third-party
notices so its client can be rebuilt.

The retained root licence attribution refers to the former plugin layout; the corresponding notice is now in `unfold-trajectory-dsh/NOTICE.md`.
