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
