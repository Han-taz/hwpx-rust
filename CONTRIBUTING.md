# Contributing

## Local Checks

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo audit
```

## Fixtures and Snapshots

- Put small public test documents in `crates/hwp-core/tests/fixtures/`.
- Do not add private or sensitive documents.
- If a JSON/HTML/Markdown snapshot changes intentionally, inspect the `.snap.new`
  file before accepting it with `cargo insta accept --workspace`.

## Parser Diagnostics

Unsupported or lossy parsing should be represented with structured diagnostics rather
than silently ignored.
