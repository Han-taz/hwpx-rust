## Summary

Describe the change and the user-visible behavior it affects.

## Risk

- [ ] Parser behavior
- [ ] Conversion output
- [ ] Parser or converter performance
- [ ] Fuzzing or parser security
- [ ] Coverage or test quality
- [ ] Python API or packaging
- [ ] Dependencies or CI
- [ ] Documentation only

## Verification

List the exact commands run, with any relevant fixture or platform details.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo bench -p hwp-core --bench parse_benchmark --locked -- --test
cargo check --manifest-path fuzz/Cargo.toml --locked
python3 packages/hwpx-python/scripts/check_release_versions.py
```

## Security and Privacy

- [ ] No private or sensitive documents were added as fixtures.
- [ ] Untrusted document inputs remain bounded by parser resource limits.
- [ ] New file writes use caller-controlled paths and sanitized document metadata.
- [ ] New dependencies pass the repository dependency policy.

## Diagnostics

- [ ] Unsupported, recovered, or lossy parsing behavior is surfaced through diagnostics.
- [ ] Snapshot changes were inspected manually when output changed.

## Performance Evidence

For changes that claim speedups, include the fixture, platform, Rust version, and
Criterion summary. CI benchmark test mode proves the benchmark target compiles; it is
not timing evidence.

## Coverage Notes

For parser, diagnostics, conversion, or Python binding changes, mention whether the
LCOV artifact or local coverage report shows newly added paths are exercised.
