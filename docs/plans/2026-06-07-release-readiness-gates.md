# Release Readiness Gates Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Strengthen the repository release/readiness gates so source distributions, CI workflows, and release documentation keep enforcing the same package and security contract.

**Architecture:** Keep the release contract in lightweight Python checker tests instead of adding another CI service. Source-distribution validation remains archive-based, workflow validation remains targeted text assertions, and release documentation validation ensures humans keep the same commands available locally.

**Tech Stack:** Python `unittest`, `tarfile`, GitHub Actions YAML as checked text, Rust/Cargo verification commands.

---

### Task 1: Commit the Implementation Plan

**Files:**
- Create: `docs/plans/2026-06-07-release-readiness-gates.md`

**Step 1: Confirm the plan is the only staged file**

Run:

```bash
git status --short docs/plans/2026-06-07-release-readiness-gates.md
```

Expected: one untracked plan file.

**Step 2: Commit the plan**

Run:

```bash
git add docs/plans/2026-06-07-release-readiness-gates.md
git commit -m "docs: plan release readiness gate implementation" -- docs/plans/2026-06-07-release-readiness-gates.md
```

Expected: commit succeeds without staging unrelated dirty files.

---

### Task 2: Require Core HWPX Parser Sources in Source Distributions

**Files:**
- Modify: `packages/hwpx-python/scripts/check_sdist_contents_test.py`
- Modify: `packages/hwpx-python/scripts/check_sdist_contents.py`

**Step 1: Write the failing test**

Add the newer HWPX parser modules to `valid_files()` in `check_sdist_contents_test.py`:

```python
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/bindata.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/header.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/package.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/section.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/xml_attr.rs": "",
```

Add a negative test:

```python
    def test_rejects_missing_hwpx_parser_modules_required_for_source_build(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            del files[f"{ROOT}/crates/hwp-core/src/parser/hwpx/package.rs"]
            del files[f"{ROOT}/crates/hwp-core/src/parser/hwpx/xml_attr.rs"]
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("package.rs", result.stderr)
            self.assertIn("xml_attr.rs", result.stderr)
```

**Step 2: Run test to verify it fails**

Run:

```bash
python3 packages/hwpx-python/scripts/check_sdist_contents_test.py
```

Expected: FAIL because `check_sdist_contents.py` does not yet require the missing HWPX parser modules.

**Step 3: Write minimal implementation**

Add the same required source files to `REQUIRED_FILES` in `check_sdist_contents.py`:

```python
    "crates/hwp-core/src/parser/hwpx/bindata.rs",
    "crates/hwp-core/src/parser/hwpx/header.rs",
    "crates/hwp-core/src/parser/hwpx/package.rs",
    "crates/hwp-core/src/parser/hwpx/section.rs",
    "crates/hwp-core/src/parser/hwpx/xml_attr.rs",
```

**Step 4: Run test to verify it passes**

Run:

```bash
python3 packages/hwpx-python/scripts/check_sdist_contents_test.py
```

Expected: PASS.

**Step 5: Commit**

Run:

```bash
git add packages/hwpx-python/scripts/check_sdist_contents.py packages/hwpx-python/scripts/check_sdist_contents_test.py
git commit -m "test: require hwpx parser sources in sdist" -- packages/hwpx-python/scripts/check_sdist_contents.py packages/hwpx-python/scripts/check_sdist_contents_test.py
```

---

### Task 3: Lock Workflow Coverage for Security, Policy, Fuzz, and Release Publishing Gates

**Files:**
- Modify: `packages/hwpx-python/scripts/check_workflows_test.py`

**Step 1: Add targeted workflow tests**

Add these tests to `PackagingWorkflowTest`:

```python
    def test_ci_runs_security_policy_fuzz_and_format_jobs(self) -> None:
        ci = workflow_text(CI_WORKFLOW)
        expectations = {
            "fuzz": [
                "cargo check --manifest-path fuzz/Cargo.toml --locked",
                "cargo fuzz build parse_auto",
                "cargo fuzz build parse_hwpx",
            ],
            "fmt": ["cargo fmt --all -- --check"],
            "audit": ["cargo audit", "cargo audit --file fuzz/Cargo.lock"],
            "cargo-deny": ["EmbarkStudios/cargo-deny-action@v2", "arguments: --locked"],
        }

        for job_name, tokens in expectations.items():
            job = job_section(ci, job_name)
            for token in tokens:
                with self.subTest(job=job_name, token=token):
                    self.assertIn(token, job)

    def test_release_workflow_uses_tag_trigger_and_trusted_publishing_controls(self) -> None:
        workflow = workflow_text(BUILD_WHEELS_WORKFLOW)
        publish = job_section(workflow, "publish-pypi")

        self.assertIn("tags:", workflow)
        self.assertIn("- 'v*'", workflow)
        self.assertIn("environment:", publish)
        self.assertIn("name: pypi", publish)
        self.assertIn("id-token: write", publish)
        self.assertIn("pypa/gh-action-pypi-publish@release/v1", publish)
```

**Step 2: Run workflow tests**

Run:

```bash
python3 packages/hwpx-python/scripts/check_workflows_test.py
```

Expected: PASS. This task adds regression coverage for already-present workflow gates.

**Step 3: Commit**

Run:

```bash
git add packages/hwpx-python/scripts/check_workflows_test.py
git commit -m "test: lock release workflow security gates" -- packages/hwpx-python/scripts/check_workflows_test.py
```

---

### Task 4: Lock Release Documentation to Artifact and Trusted Publishing Contracts

**Files:**
- Modify: `packages/hwpx-python/scripts/check_release_docs_test.py`

**Step 1: Add release documentation tests**

Add these tests to `ReleaseDocsTest`:

```python
    def test_release_docs_cover_sdist_artifact_validation(self) -> None:
        release = text(RELEASE_DOC)
        required_tokens = [
            "python3 packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "python3 packages/hwpx-python/scripts/check_sdist_install.py dist/*.tar.gz",
            "Source distributions must include the Rust workspace sources required for local builds",
            "include Python typing files",
            "metadata versions synchronized",
        ]

        for token in required_tokens:
            with self.subTest(token=token):
                self.assertIn(token, release)

    def test_release_docs_describe_trusted_publishing_controls(self) -> None:
        release = text(RELEASE_DOC)
        required_tokens = [
            "Trusted Publishing",
            "Workflow: `build-wheels.yml`",
            "Environment: `pypi`",
            "id-token: write",
            "pypa/gh-action-pypi-publish@release/v1",
        ]

        for token in required_tokens:
            with self.subTest(token=token):
                self.assertIn(token, release)
```

**Step 2: Run release docs tests**

Run:

```bash
python3 packages/hwpx-python/scripts/check_release_docs_test.py
```

Expected: PASS. This task adds regression coverage for already-present release docs.

**Step 3: Commit**

Run:

```bash
git add packages/hwpx-python/scripts/check_release_docs_test.py
git commit -m "test: lock release documentation publishing contract" -- packages/hwpx-python/scripts/check_release_docs_test.py
```

---

### Task 5: Run Focused Python Gate Tests

**Files:**
- Test-only task.

**Step 1: Run focused scripts**

Run:

```bash
python3 packages/hwpx-python/scripts/check_sdist_contents_test.py
python3 packages/hwpx-python/scripts/check_workflows_test.py
python3 packages/hwpx-python/scripts/check_release_docs_test.py
```

Expected: all pass.

**Step 2: Run all checker tests**

Run:

```bash
python3 -m unittest discover -s packages/hwpx-python/scripts -p '*_test.py'
```

Expected: all checker tests pass.

---

### Task 6: Run Final Repository Verification for This Slice

**Files:**
- Test-only task.

**Step 1: Rust parser/library checks**

Run with the stable Rust toolchain on PATH:

```bash
cargo test -p hwp-core --locked
cargo clippy -p hwp-core --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

Expected: all pass.

**Step 2: Production hygiene checks**

Run:

```bash
python3 packages/hwpx-python/scripts/check_no_production_panics.py
python3 packages/hwpx-python/scripts/check_no_production_debug_output.py
python3 packages/hwpx-python/scripts/check_no_production_todos.py
git diff --check
find . -name '*.snap.new' -print
```

Expected: hygiene scripts pass, `git diff --check` returns 0, and `find` prints nothing.

**Step 3: Report status**

Summarize:
- Plan and implementation commits created.
- Focused and full Python checker tests passed.
- Rust verification and hygiene gates passed or list exact failures if any remain.
