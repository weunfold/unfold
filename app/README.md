# Build the Unfold macOS app

The native host is `src-tauri/`; its interface is `ui/`. The app uses Tauri 2 and
targets macOS on Apple Silicon. Rust source is Apache-2.0. Bundled fonts retain
the OFL notices in `ui/ds/fonts/`; native dependency notices are in
`src-tauri/licenses/`.

## Build inputs

Use the committed Cargo.lock. Supply the two pinned shared libraries named in
`src-tauri/src/sherpa_pins.rs` in a directory containing only those libraries.
Set `SHERPA_ONNX_LIB_DIR` to that directory. The build checks their exact sizes
and SHA256 hashes before copying them into the generated frameworks directory.

Supply the three ONNX files listed in `src-tauri/turn-models/pins.json`, using
the exact filenames, sizes and hashes in that file. Set
`UNFOLD_TURN_MODELS_DIR` to their directory. No model bytes or base64-encoded
models are committed here. The build fails on missing or mismatched inputs.

```sh
cd app/src-tauri
cargo check --locked --features self_pane,voice_pane,menu_bar
cargo test --locked --features self_pane,voice_pane,menu_bar -- --test-threads 4
```

Tests that exercise inference need the model inputs. Tests that open native
audio devices require the corresponding macOS permissions and hardware.
Compilation alone does not validate either path. No signing or installation
is part of these commands.

The app's DSH pack pin remains the published pack recorded in `pack.pin`.
Building source does not create or publish a new pack. The optional modern
plugins in `../plugins` are a separate installation, and are not added to that
pack by this source change.

## Interface tests

The interface tests use Playwright with synthetic Tauri responses. Supply an
existing `playwright-core` module and its compatible Chromium installation:

```sh
PLAYWRIGHT_CORE=/path/to/playwright-core/index.mjs node --test --test-concurrency=4 app/test/*.test.mjs
```

Run this command from the repository root. An existing headless shell can be
selected with `CHROME_HEADLESS_SHELL` where supported by the test.

## Optional dictation calibration audio

Recordings are not included. The optional calibration action reports that the
test audio is unavailable unless `UNFOLD_DICTATION_TEST_CLIPS` points to an
externally supplied directory containing the two FLEURS files named by the
calibration code. Their attribution is retained in
`src-tauri/dictation/test-clips/NOTICE.txt`. This does not affect ordinary
microphone capture. Voice-pack reference recordings are also distributed
separately from source.

## Providers and updates

Hosted providers receive the conversation data required by the selected route.
No hosted key belongs in this source tree. The application's existing signed
update endpoint remains separate from the optional plugin release tags.

Old installs can set `UNFOLD_LEGACY_DEEPSEEK_SERVICE` to migrate a pre-0.4.2 key.
