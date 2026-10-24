# NOTICE: @rth/dsh-rth-trajectory

This plugin is a modified copy of **@deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.2**
(MIT License, Copyright (c) 2026 DeepSeek). The full license text is in `LICENSE` (identical to
`upstream/LICENSE`). The built `lib/client.js` repeats the notice in a preserved `/*! ... */` header.

## Upstream, as copied

| item | value |
| --- | --- |
| package | `@deepseek-ai/dsh-client-ui-trajectory` |
| version | `0.2.1-rc.2` |
| source | the published npm package, copied verbatim to `upstream/` |
| `upstream/lib/client.js` sha256 | `b11426f89d3b4302964b491d7826a29fe3de390be4bbfae325375f54fe6919ad` |
| license | MIT, Copyright (c) 2026 DeepSeek |

`upstream/` is never edited. `build.mjs` refuses to run if the bundle hash differs, and
`test/build.test.mjs` checks the full hash.

### Upstream license, verbatim (`LICENSE`)

```
MIT License

Copyright (c) 2026 DeepSeek

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## What RTH changed

`build.mjs` applies the patches below to a copy of the upstream bundle. Each patch must match
the stated number of times or the build fails. Region names are the upstream source files as
the bundle's `//#region` comments name them.

| region | change | matches |
| --- | --- | --- |
| top (module requires) | load the RTH runtime (taxonomy, adapter, shared stream, badges, styles) inside the module factory | 1 |
| locales.js | own dictionary namespace (never the upstream one) | 1 |
| locales.js | RTH keys added to both dictionaries | 2 (en, zh) |
| TrajectoryTable.js | turn header cards: the RTH turn card replaces "Turn N" when one exists; the between-turns label is the short "Between" | 1 |
| TrajectoryTable.js | ledger row: the coloured RTH kind badge for RTH records | 1 |
| TrajectoryTable.js | inspector header: the RTH kind badge | 1 |
| TrajectoryTable.js | inspector Summary: no token rows for RTH records (tokens are not attributed) | 1 |
| TrajectoryTable.js | inspector Summary: the RTH Timing block (named endpoints, clock, evidence ids, "not measured: reason") | 1 |
| TrajectoryTable.js | turn header card: the measured chips on the turn's first row | 1 |
| timeline.js | RTH records: their own kind, lane (five lanes) and span honesty (measured / instant / unknown) | 2 |
| TrajectoryTimeline.js | span honesty attribute (unknown = start marker + hatched, non-scaled "?" tail in every mode; instant = tick in timed modes) | 1 |
| TrajectoryTimeline.js | tooltip heading for RTH kinds | 1 |
| TrajectoryTimeline.js | five lane labels: Hear, Think, Speak, Act, Life | 1 |
| TrajectoryTimeline.module.css | room for five lanes | 1 |
| TrajectoryView.js | the RTH view (same toolbar, overview, ledger and inspector; RTH data; kind filter chips and session strip) | 1 (inserts `src/view.js`) |
| index.js | registration: its own conversation.view entry "rth-trajectory" labelled "RTH", its own right-sidebar tab kind "rth.trajectory" and the Bot header "Trajectory" button that opens it; no upstream Definitions, no uiSession hook, upstream Trajectory untouched | 1 (replaces the region with `src/index-region.js`) |
| all | own loader id, style-tag ids and plugin dataset (never the upstream ones) | 9 |
| duration-store.js, string-wrapping-store.js | own persisted-preference names | 2 |
| TrajectoryTable.js | status: RTH records take their state from the validated RTH outcome, never the upstream "complete" fallback | 1 |
| TrajectoryTable.js | status labels for the RTH-only states (pending, cancelled, not attributed, observed, unclassified) | 1 |
| TrajectoryTable.js | no upstream "Started / session timestamps" timing block for RTH records (their timing is the RTH Timing block) | 1 |
| TrajectoryTimeline.js | tooltip detail: a measured duration only, never observed_at -> observed_at + duration | 1 |
| timeline.js | timed ranges: an RTH record is an instant at its observation time, never observed_at + a producer duration | 1 |
| TrajectoryToolbar.js | duration toggle hook (rthSequenceOnly): the toggle stays and maps Duration to the producer-clock overview; the time toggle stays hidden upstream | 2 (signature, button) |

After the patches, `build.mjs` also:

- renames the four CSS-module class prefixes to `rtj…`, so upstream rules never reach this
  view's elements and this view's rules never reach upstream's, even with both plugins loaded;
- bundles `src/runtime.mjs` (which imports `src/adapter.mjs`, `src/taxonomy.mjs`, `src/status.mjs`, `src/clock-layout.mjs`,
  `src/icons.generated.mjs` and the
  `plugins/dsh-mind/src/{metrics,moments}.mjs` logic, and a copy of the Mind client's stream reducer for the one shared `/api/rth-mind/stream` subscription) with esbuild into the module factory;
- writes the license header.

The upstream Trajectory view itself is untouched. It is a separate package, loaded or not by
the profile as before.

## RTH's own code

`src/` (taxonomy, adapter, status, clock layout, runtime, view and index regions; not the generated icon data), `build.mjs`, `tools/`, `test/` and
`fixtures/` are RTH's own work.

## Third-party notices

### Lucide icons (lucide-static 0.460.0)

The badge icons' element data is copied from `lucide-static` 0.460.0
(`https://registry.npmjs.org/lucide-static/-/lucide-static-0.460.0.tgz`, integrity
`sha512-X6pIdg7jVxv7YQ/uR241hwhNiztcAfmj181TbcX7HCxxk/3mGaRtAc6b2ftUvQBufbJE6ehgyzO2uVsa604tWg==`)
by `tools/pin-icons.mjs` into `src/icons.generated.mjs`; per-icon sha256 values are in
`third_party/lucide-static-0.460.0/SOURCE.json`. Its license, verbatim
(`third_party/lucide-static-0.460.0/LICENSE`):

```
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Feather (portions of Lucide)

Lucide names portions of its icons as Feather work under MIT. The Feather notice, verbatim from
`feather-icons` 4.29.2 (integrity
`sha512-0TaCFTnBTVCz6U+baY2UJNKne5ifGh7sMG4ZC2LoBWCZdIyPa+y6UiR4lEYGws1JOFWdee8KAsAIvu0VcXqiqA==`;
`third_party/feather-icons-4.29.2/LICENSE`):

```
The MIT License (MIT)

Copyright (c) 2013-2023 Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The bundle header repeats both notices in full, after the DeepSeek MIT notice.
