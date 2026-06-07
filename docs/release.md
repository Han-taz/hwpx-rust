# Release Process

This project releases a Rust workspace and a Python package backed by a PyO3 native
extension. Releases should be reproducible from a clean checkout and should not depend
on local generated artifacts.

## Pre-Release Checks

Run these from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info
cargo bench -p hwp-core --bench parse_benchmark --locked -- --test
cargo check --manifest-path fuzz/Cargo.toml --locked
cargo deny --locked check
cargo audit
cargo audit --file fuzz/Cargo.lock
python3 packages/hwpx-python/scripts/check_workflows_test.py
python3 packages/hwpx-python/scripts/check_release_docs_test.py
python3 packages/hwpx-python/scripts/check_no_production_panics.py
python3 packages/hwpx-python/scripts/check_no_production_panics_test.py
python3 packages/hwpx-python/scripts/check_no_production_debug_output.py
python3 packages/hwpx-python/scripts/check_no_production_debug_output_test.py
python3 packages/hwpx-python/scripts/check_no_production_todos.py
python3 packages/hwpx-python/scripts/check_no_production_todos_test.py
python3 packages/hwpx-python/scripts/check_release_versions.py
python3 packages/hwpx-python/scripts/check_release_versions_test.py
python3 packages/hwpx-python/scripts/check_pyo3_gil_release.py
python3 packages/hwpx-python/scripts/check_pyo3_gil_release_test.py
python3 packages/hwpx-python/scripts/check_wheel_contents_test.py
python3 packages/hwpx-python/scripts/check_wheel_install_test.py
python3 packages/hwpx-python/scripts/check_sdist_contents_test.py
python3 packages/hwpx-python/scripts/check_sdist_install_test.py
python3 -m py_compile \
  packages/hwpx-python/scripts/check_no_production_debug_output.py \
  packages/hwpx-python/scripts/check_no_production_debug_output_test.py \
  packages/hwpx-python/scripts/check_no_production_todos.py \
  packages/hwpx-python/scripts/check_no_production_todos_test.py \
  packages/hwpx-python/scripts/check_no_production_panics.py \
  packages/hwpx-python/scripts/check_no_production_panics_test.py \
  packages/hwpx-python/scripts/check_pyo3_gil_release.py \
  packages/hwpx-python/scripts/check_pyo3_gil_release_test.py \
  packages/hwpx-python/scripts/check_release_docs_test.py \
  packages/hwpx-python/scripts/check_release_versions.py \
  packages/hwpx-python/scripts/check_release_versions_test.py \
  packages/hwpx-python/scripts/check_sdist_contents.py \
  packages/hwpx-python/scripts/check_sdist_contents_test.py \
  packages/hwpx-python/scripts/check_sdist_install.py \
  packages/hwpx-python/scripts/check_sdist_install_test.py \
  packages/hwpx-python/scripts/check_wheel_contents.py \
  packages/hwpx-python/scripts/check_wheel_contents_test.py \
  packages/hwpx-python/scripts/check_wheel_install.py \
  packages/hwpx-python/scripts/check_wheel_install_test.py \
  packages/hwpx-python/scripts/check_workflows_test.py
```

Build and validate a local wheel:

```bash
uv tool run maturin build \
  --release \
  --locked \
  --compatibility pypi \
  --interpreter python3 \
  --out dist \
  -m packages/hwpx-python/Cargo.toml

python3 packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl
python3 packages/hwpx-python/scripts/check_wheel_install.py dist/*.whl
```

Build the source distribution:

```bash
uv tool run maturin sdist --out dist -m packages/hwpx-python/Cargo.toml
python3 packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz
python3 packages/hwpx-python/scripts/check_sdist_install.py dist/*.tar.gz
```

Inspect the generated artifacts before publishing. Wheels must contain exactly one
native extension, include `hwpxkit/__init__.pyi` and `hwpxkit/py.typed`, and omit generated
cache files or local extension artifacts. Source distributions must include the Rust
workspace sources required for local builds, include Python typing files, keep release
metadata versions synchronized, and omit local virtualenv, cache, and build outputs.
They must also install successfully from source in a fresh virtual environment and
import with matching runtime and package metadata versions.

Coverage usage is documented in [docs/coverage.md](coverage.md). Coverage should be
reviewed for parser, diagnostics, conversion, and Python binding changes, but releases
are not blocked on a hard percentage threshold.

## Versioning

Keep these versions in sync:

- `Cargo.toml` `[workspace.package]`
- `Cargo.lock` workspace package entries
- `packages/hwpx-python/Cargo.toml`
- `packages/hwpx-python/pyproject.toml`
- `packages/hwpx-python/python/hwpxkit/__init__.py`
- release tag name

Use `python3 packages/hwpx-python/scripts/check_release_versions.py` before tagging.
Use tags with a `v` prefix, for example `v0.2.1`.

## GitHub Release

Pushing a `v*` tag triggers the wheel workflow. The workflow builds:

- Linux x86_64 manylinux2014 wheel
- Linux aarch64 manylinux2014 wheel
- macOS x86_64 wheel
- macOS aarch64 wheel
- Windows x86_64 wheel
- source distribution

The release job uploads wheels and the source distribution to the GitHub Release.

## PyPI Publishing

Tagged releases publish the same wheel and source-distribution artifacts to PyPI using
Trusted Publishing. The workflow does not use a long-lived PyPI API token.

Before the first release, configure PyPI with a GitHub trusted publisher for:

- PyPI project: `hwpxkit`
- GitHub repository: `Han-taz/hwpx-rust`
- Workflow: `build-wheels.yml`
- Environment: `pypi`

The `publish-pypi` job requests GitHub's OIDC token with `id-token: write` and uses
`pypa/gh-action-pypi-publish@release/v1` without a username or password. The `pypi`
environment should require manual approval or protected-branch/tag rules before
publishing. PyPI publish attestations are produced by the PyPA publish action when
Trusted Publishing is used.

## Post-Release Checks

After release artifacts are available:

- Install the wheel in a fresh virtual environment.
- Install the source distribution in a fresh virtual environment.
- Import `hwpxkit` and verify `hwpxkit.__version__`.
- Parse at least one public HWPX fixture.
- Run `to_markdown`, `to_html`, `to_json`, `get_text`, and `diagnostic_report`.
- Confirm the PyPI release contains all wheels, the source distribution, and publish
  attestations.
- Confirm the release notes mention parser safety, diagnostics, compatibility, and
  known limitations for the release.
