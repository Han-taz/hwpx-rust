# Coverage Guide

Coverage reports show which parser, converter, and Python-binding paths are exercised
by tests. Coverage is a visibility signal, not a substitute for parser correctness,
snapshot review, fuzzing, or corpus testing.

## CI Coverage

CI generates an LCOV report with `cargo-llvm-cov` and uploads it as a workflow
artifact. The job does not enforce a percentage threshold yet; it is intended to make
coverage changes visible without blocking parser work on an arbitrary number.

The CI command is:

```bash
cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info
```

## Local Setup

Install the tool:

```bash
cargo install cargo-llvm-cov --locked
```

If your Rust installation uses `rustup`, ensure `llvm-tools-preview` is available:

```bash
rustup component add llvm-tools-preview
```

If `llvm-tools-preview` is unavailable but system LLVM tools are installed, set
`LLVM_COV` and `LLVM_PROFDATA`:

```bash
LLVM_COV=/path/to/llvm-cov \
LLVM_PROFDATA=/path/to/llvm-profdata \
cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info
```

## Local Reports

Generate an LCOV report:

```bash
cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info
```

Generate an HTML report for local inspection:

```bash
cargo llvm-cov --workspace --all-features --locked --html --output-dir target/llvm-cov/html
```

Open `target/llvm-cov/html/index.html` in a browser to inspect uncovered branches and
files.

## Review Policy

Coverage regressions should be reviewed when a pull request changes parser behavior,
resource limits, diagnostics, Python bindings, or conversion output. A lower coverage
percentage is not automatically wrong, but unexplained drops in HWPX parser coverage
should be treated as a quality risk.

When adding parser functionality, prefer tests that cover:

- valid HWPX XML for the supported feature
- malformed or missing attributes
- unsupported or lossy constructs that should produce diagnostics
- resource-limit behavior for untrusted input
