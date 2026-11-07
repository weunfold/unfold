# Unfold agent

This Cargo workspace contains the agent daemon, protocol, memory and task
contracts, voice queues and supporting tools. Its Rust source is Apache-2.0.

```sh
cd agentd
cargo check --workspace --locked
cargo test --workspace --locked -- --test-threads 4
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The ordinary regression suite uses synthetic local services. Real-provider,
Brain integration and model qualification lanes require their own explicit
configuration; a skipped or ignored test does not validate those integrations.
TLS regression fixtures require the `openssl` CLI on PATH and are generated locally at test time. No
private TLS keys are stored in the repository.

The app uses `rth-voice` and `rth-protocol` from this workspace. Its activation
and speech-client dependencies live in the repository's sibling `crates/`
directory. No model weights, provider credentials or voice recordings are
included. Configure providers and runtime data outside the source tree.

The event and protocol types are versioned independently of source paths.
Keep persisted identifiers stable when changing implementations.

The optional memory-search seam requires the Brain source modules configured by `RTH_MEMORY_BRAIN_SRC`. Its imports retain the bundled provider contract.

## Optional voice fixture tests

Seven device-free voice tests need macOS system-voice audio, which is not
redistributed here. On macOS with ffmpeg, run
`sh crates/rth/testdata/voice-fixtures/make.sh` from this workspace. The script
verifies the generated files against the committed SHA256SUMS instead of
rewriting its pins; different installed voice versions can produce different
bytes. Set `UNFOLD_VOICE_FIXTURES_DIR` to the absolute fixture directory, then run:

```sh
cargo test -p rth --lib --locked -- --test-threads 4 --include-ignored
```

Ordinary runs report these seven tests as ignored. The explicit fixture run
executes every original assertion after checking each consumed file's hash.

## Optional benchmark registry tests

Two registry tests need the external text-contract benchmark tree, including
its held-out promotion split, which is not distributed here. Set
`UNFOLD_BENCH_DIR` to that tree and run only the registry target:

```sh
cargo test -p rth-bench --test registry --locked -- --include-ignored --test-threads 4
```

The tamper test copies only the registry's suite, tasks and fixture files to a
temporary directory. Its original inputs stay unchanged. The separate VM
end-to-end test remains ignored; the command above does not select it.
