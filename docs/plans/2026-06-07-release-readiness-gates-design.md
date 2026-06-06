# Release Readiness Gates Design

## Goal

Make the repository's release and readiness gates hard to drift out of sync. The
project already documents a broad release process and has CI jobs for tests,
formatting, clippy, coverage, fuzz builds, dependency policy, Python packaging, and
wheel builds. This work adds stronger local checkers so the documented process, CI
workflows, and package validation scripts keep enforcing the same release contract.

## Scope

Included:
- Strengthen release documentation checks so `docs/release.md` stays aligned with
  the checker scripts and workflow gates.
- Strengthen workflow checks so CI keeps running the required Rust, Python,
  dependency, fuzz, benchmark, wheel, and source-distribution gates.
- Strengthen wheel and source-distribution checker tests where they do not yet
  prove the documented package contract.
- Keep checks deterministic and runnable without private credentials or network
  services.

Excluded:
- Publishing to PyPI or creating a GitHub release.
- Adding timing thresholds to CI benchmarks, because shared runners are too noisy
  for reliable performance assertions.
- Replacing existing CI workflows with a new release system.

## Architecture

The release contract should have three layers:

1. `docs/release.md` describes the human-facing release checklist.
2. `.github/workflows/*.yml` runs the automated CI and release gates.
3. `packages/hwpx-python/scripts/*` validates workflow shape, release docs, package
   contents, version synchronization, and Python binding safety.

The checker scripts should treat the docs and workflows as structured text inputs
and assert required commands, jobs, steps, permissions, and artifact validations are
present. They should avoid brittle full-file snapshots; targeted assertions give a
stable signal while still allowing workflow formatting and ordering to evolve.

Package artifact validators should continue inspecting built wheel and source
distribution archives directly. They are the authoritative evidence for release
artifact shape: one native extension, `abi3` wheel compatibility where expected,
typing marker files, synchronized metadata, no generated caches, and source contents
required for local builds.

## Error Handling

Checker failures should report concrete missing or mismatched items rather than a
generic "invalid workflow" message. Each checker should fail closed: if a required
document, workflow, archive member, metadata field, or version source cannot be read,
the check should fail with the path and expected invariant.

## Testing

Use focused Python tests for the release/readiness gate scripts:
- Positive tests with minimal representative docs, workflows, wheels, and sdists.
- Negative tests for missing required commands, missing workflow steps, mismatched
  versions, missing type marker files, extra generated artifacts, and unsafe archive
  contents.
- Full script discovery with `python3 -m unittest discover -s packages/hwpx-python/scripts -p '*_test.py'`.

Final verification for the slice should include:
- `cargo test -p hwp-core --locked`
- `cargo clippy -p hwp-core --all-targets --all-features --locked -- -D warnings`
- `cargo fmt --all -- --check`
- Python checker test discovery
- production panic/debug/TODO checkers
- `git diff --check`
- no `.snap.new` files
