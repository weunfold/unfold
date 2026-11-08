# NOTICE: @automagik/unfold-trajectory-dsh

This plugin is a modified copy of **@deepseek-ai/dsh-client-ui-trajectory 0.2.1-rc.3**
(MIT License, Copyright (c) 2026 DeepSeek). The full license text is in `LICENSE` (identical to
`upstream/LICENSE`). The built `client.js` repeats every notice below in a preserved `/*! ... */`
header. The changes are Copyright 2026 Namastex Labs and the Automagik contributors, under the
same MIT License.

## Upstream, as copied

| item | value |
| --- | --- |
| package | `@deepseek-ai/dsh-client-ui-trajectory` |
| version | `0.2.1-rc.3` |
| copied | `LICENSE`, `package.json` and `lib/client.js`, verbatim, into `upstream/`; `upstream/SHA256SUMS` lists every file of the published package |
| `upstream/lib/client.js` sha256 | `b11426f89d3b4302964b491d7826a29fe3de390be4bbfae325375f54fe6919ad` (the same bytes as 0.2.1-rc.2) |
| license | MIT, Copyright (c) 2026 DeepSeek |

`upstream/` is never edited. `scripts/build-client.mjs` refuses to run if the bundle hash differs,
and `test/build.test.js` and `test/public.test.js` check the full hashes.

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

## What changed

`scripts/build-client.mjs` applies the patches below to a copy of the upstream bundle. Each patch
must match the stated number of times or the build fails. Region names are the upstream source
files as the bundle's `//#region` comments name them.

| region | change | matches |
| --- | --- | --- |
| top (module requires) | load the Unfold runtime (taxonomy, adapter, shared stream, badges, styles) inside the module factory | 1 |
| locales.js | own dictionary namespace (never the upstream one) | 1 |
| locales.js | Unfold keys added to both dictionaries | 2 (en, zh) |
| TrajectoryTable.js | turn header cards: the Unfold turn card replaces "Turn N" when one exists; the between-turns label is the short "Between" | 1 |
| TrajectoryTable.js | ledger row: the coloured Unfold kind badge for Unfold records | 1 |
| TrajectoryTable.js | inspector header: the Unfold kind badge | 1 |
| TrajectoryTable.js | inspector Summary: no token rows for Unfold records (tokens are not attributed) | 1 |
| TrajectoryTable.js | inspector Summary: the Unfold Timing block (named endpoints, clock, evidence ids, "not measured: reason") | 1 |
| timeline.js | Unfold records: their own kind, lane (five lanes) and span honesty (measured / instant / unknown) | 2 |
| TrajectoryTable.js | turn header card: the measured chips on the turn's first row | 1 |
| TrajectoryTimeline.js | span honesty attribute (unknown = start marker + hatched, non-scaled "?" tail in every mode; instant = tick in timed modes) | 1 |
| TrajectoryTimeline.js | tooltip heading for Unfold kinds | 1 |
| TrajectoryTimeline.js | five lane labels: Hear, Think, Speak, Act, Life | 1 |
| TrajectoryTimeline.module.css | room for five lanes | 1 |
| TrajectoryView.js | the Unfold view (same toolbar, overview, ledger and inspector; Unfold data; kind filter chips and session strip) | 1 |
| index.js | registration: its own conversation.view entry "unfold-trajectory" labelled "Unfold", its own right-sidebar tab kind "unfold.trajectory" and the Bot header "Trajectory" button that opens it; no upstream Definitions, no uiSession hook, upstream Trajectory untouched | 1 |
| all | own loader id, style-tag ids and plugin dataset (never the upstream ones) | 9 |
| duration-store.js, string-wrapping-store.js | own persisted-preference names | 2 |
| TrajectoryTable.js | status: Unfold records take their state from the validated Unfold outcome, never the upstream "complete" fallback | 1 |
| TrajectoryTable.js | status labels for the Unfold-only states (pending, cancelled, not attributed, observed, unclassified) | 1 |
| TrajectoryTable.js | no upstream "Started / session timestamps" timing block for Unfold records (their timing is the Unfold Timing block) | 1 |
| TrajectoryTimeline.js | tooltip detail: a measured duration only, never observed_at -> observed_at + duration | 1 |
| timeline.js | timed ranges: an Unfold record is an instant at its observation time, never observed_at + a producer duration | 1 |
| TrajectoryToolbar.js | duration toggle hook (unfoldSequenceOnly): keeps the toggle and maps Duration to the producer-clock overview; the time toggle stays hidden upstream | 2 (signature, button) |
| CSS-module regions | region comments name each CSS module by its path in the upstream repository, never the machine that built it | 4 |

After the patches, the build also:

- renames the four CSS-module class prefixes to `uft…`, so upstream rules never reach this
  view's elements and this view's rules never reach upstream's, even with both plugins loaded;
- bundles `src/runtime.mjs` with esbuild into the module factory. It imports `src/adapter.mjs`,
  `src/taxonomy.mjs`, `src/status.mjs`, `src/clock-layout.mjs`, `src/icons.generated.mjs`, the
  Mind metrics and moments (`src/mind/`, copies of `@automagik/unfold-mind-dsh`'s), and a copy of
  the Mind client's stream reducer for the one shared `/api/unfold-mind/stream` subscription;
- writes the license header.

DeepSeek's own Trajectory view is untouched. It is a separate package, loaded or not by the
profile as before.

## Our own code

`src/` (taxonomy, adapter, status, clock layout, runtime, view and index regions; not the
generated icon data), `scripts/`, `test/` and `index.js`. `src/mind/`, `test/support/` and
`catalog/` are `@automagik/unfold-mind-dsh`'s files byte for byte, pinned by sha256 in
`test/mind-copy.test.js`: the metrics and moments that explain a reply, bundled into `client.js`,
and the Mind stream, event window and catalog the tests drive; the test files are not shipped.

## Third-party notices

### Lucide icons (lucide-static 0.460.0)

The badge icons' element data is copied from `lucide-static` 0.460.0
(`https://registry.npmjs.org/lucide-static/-/lucide-static-0.460.0.tgz`, integrity
`sha512-X6pIdg7jVxv7YQ/uR241hwhNiztcAfmj181TbcX7HCxxk/3mGaRtAc6b2ftUvQBufbJE6ehgyzO2uVsa604tWg==`)
by `scripts/pin-icons.mjs` into `src/icons.generated.mjs`; per-icon sha256 values are in
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

The bundle header repeats the Lucide and Feather notices in full, after the DeepSeek MIT notice,
and then the jsdiff notice below.

### jsdiff (diff 9.0.0), inside upstream's bundle

Upstream's `lib/client.js` bundles jsdiff 9.0.0 (`https://registry.npmjs.org/diff/-/diff-9.0.0.tgz`,
integrity
`sha512-svtcdpS8CgJyqAjEQIXdb3OjhFVVYjzGAPO8WGCmRbrml64SPw/jJD4GoE98aR7r25A0XcgrK3F02yw9R/vhQw==`),
and so does this fork's `client.js`. Its license, verbatim (`third_party/diff-9.0.0/LICENSE`):

```
BSD 3-Clause License

Copyright (c) 2009-2015, Kevin Decker <kpdecker@gmail.com>
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

The vendored upstream client has machine-specific CSS region comments normalized to repository-relative paths. Its original upstream digest remains in the generated client notice; upstream/SHA256SUMS records the normalized source.
