# Fuzzing Guide

This project uses `cargo-fuzz` and libFuzzer to exercise parser entrypoints with
malformed and semi-structured inputs. Fuzzing is part of the security workflow because
HWP/HWPX documents are untrusted input.

## Targets

The fuzz workspace lives in `fuzz/`.

| Target | Entry point | Purpose |
| --- | --- | --- |
| `parse_auto` | `HwpParser::parse` | Exercises format detection plus HWP/HWPX dispatch. |
| `parse_hwpx` | `hwp_core::parser::hwpx::parse` | Exercises the HWPX ZIP/XML parser directly. |

Targets ignore inputs larger than 2 MiB to keep local fuzzing focused on parser edge
cases instead of spending cycles on oversized payloads. The production parser still
enforces its own ZIP, XML, and section resource limits.

## Setup

Install the pinned Rust toolchain and `cargo-fuzz` version used by CI:

```bash
rustup toolchain install nightly-2025-06-01
cargo install cargo-fuzz --version 0.13.1 --locked
```

`cargo-fuzz` uses libFuzzer and typically runs with a nightly Rust toolchain. CI builds
the fuzz targets on `nightly-2025-06-01` to catch target and API drift; it does not run
open-ended fuzz campaigns on pull requests.

## Local Runs

List targets:

```bash
cargo +nightly-2025-06-01 check --manifest-path fuzz/Cargo.toml --locked
cargo +nightly-2025-06-01 fuzz list
```

Build targets:

```bash
cargo +nightly-2025-06-01 fuzz build parse_auto
cargo +nightly-2025-06-01 fuzz build parse_hwpx
```

Run a short local campaign:

```bash
cargo +nightly-2025-06-01 fuzz run parse_hwpx -- -max_total_time=300
```

Run longer campaigns locally or in a dedicated fuzzing environment:

```bash
cargo +nightly-2025-06-01 fuzz run parse_auto
cargo +nightly-2025-06-01 fuzz run parse_hwpx
```

## Failure Handling

When libFuzzer finds a crash or timeout, it writes artifacts under
`fuzz/artifacts/<target>/`. Minimize a crashing input before adding a regression test:

```bash
cargo +nightly-2025-06-01 fuzz tmin parse_hwpx fuzz/artifacts/parse_hwpx/crash-...
```

After minimization:

1. Reproduce the issue with the minimized artifact.
2. Add a focused regression test in `crates/hwp-core/tests` or a parser unit test.
3. Fix the parser without weakening resource limits.
4. Keep the minimized artifact only if it is safe to publish and useful as a fixture.

## CI Policy

Pull request CI builds fuzz targets but does not run timing-dependent fuzz campaigns.
This catches stale fuzz targets and parser API drift while keeping CI deterministic.
Long-running fuzz jobs should be scheduled outside normal PR checks or run manually
before high-risk parser changes.
