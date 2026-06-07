# Performance Guide

Performance work should be measured against public HWPX fixtures and kept separate
from correctness changes when possible. The parser is optimized for predictable memory
use, bounded processing of untrusted files, and low allocation pressure in hot XML and
rendering paths.

## Benchmark Targets

The Rust benchmark harness lives at `crates/hwp-core/benches/parse_benchmark.rs` and
covers:

- HWPX parsing on a deterministic synthetic document
- Markdown conversion on a successfully parsed synthetic document
- HWPX parsing on a deterministic table-heavy synthetic document
- Markdown conversion on a successfully parsed table-heavy synthetic document
- HWPX parsing on a public fixture, when one is available
- Markdown conversion for a successfully parsed public fixture, when one is available

Run the benchmark from the repository root:

```bash
cargo bench -p hwp-core --bench parse_benchmark --locked
```

To compile the benchmark and run Criterion's test mode without collecting timing data:

```bash
cargo bench -p hwp-core --bench parse_benchmark --locked -- --test
```

CI uses test mode as a compile guard. It catches broken benchmark targets and API drift
without failing pull requests on noisy wall-clock timing changes from shared runners.

## Measuring a Change

For parser or converter performance changes:

1. Run `cargo test --workspace --locked` first to confirm output behavior.
2. Run `cargo bench -p hwp-core --bench parse_benchmark --locked` on the baseline branch.
3. Run the same benchmark on the candidate branch.
4. Compare Criterion reports under `target/criterion`.
5. Include the fixture name, machine type, OS, Rust version, and benchmark summary in
   the pull request.

Do not claim a speedup from a single noisy run. Prefer repeated local runs on an idle
machine, and treat CI benchmark test mode as a build guard rather than a timing source.

## Fixture Policy

The benchmark target always includes deterministic paragraph-heavy and table-heavy
synthetic HWPX workloads from `crates/hwp-core/benches/hwpx_bench_data.rs`. This keeps
the benchmark runnable even when fixture discovery changes and gives parser changes a
stable signal for both run-text and table-cell paths. Public fixture benchmarks may
additionally use small documents from `crates/hwp-core/tests/fixtures`.

Never add private documents, customer data, or documents with sensitive metadata. If
a larger benchmark corpus is needed, document its provenance and keep it out of the
default test path unless it is safe for public distribution.

## Performance Invariants

Parser performance work must preserve these invariants:

- HWPX ZIP, XML, and section resource limits remain enforced.
- Unsupported or lossy parsing remains visible through diagnostics.
- Image output paths remain caller-controlled and sanitized.
- Python CPU-bound APIs keep releasing the GIL while Rust code runs.
- Snapshot output changes are reviewed intentionally.

## Python Workloads

Python users should prefer the native package APIs:

- `hwpxkit.parse`
- `hwpxkit.parse_file`
- `Document.to_markdown`
- `Document.to_html`
- `Document.to_json`
- `Document.get_text`

These APIs delegate CPU-bound work to Rust and release the Python GIL for parse and
conversion work, which improves behavior in threaded Python applications.
