#!/usr/bin/env python3
"""Tests for Python packaging gates in GitHub workflows."""

from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
BUILD_WHEELS_WORKFLOW = ROOT / ".github" / "workflows" / "build-wheels.yml"


def workflow_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def job_section(text: str, job_name: str) -> str:
    marker = f"\n  {job_name}:\n"
    start = text.find(marker)
    if start == -1:
        raise AssertionError(f"workflow is missing job {job_name!r}")
    next_job = text.find("\n  ", start + len(marker))
    while next_job != -1:
        following = text.find("\n", next_job + 1)
        line = text[next_job + 1:following if following != -1 else len(text)]
        if line.startswith("  ") and not line.startswith("    "):
            return text[start:next_job]
        next_job = text.find("\n  ", next_job + 1)
    return text[start:]


class PackagingWorkflowTest(unittest.TestCase):
    def test_ci_uses_locked_cargo_commands_for_reproducible_builds(self) -> None:
        ci = workflow_text(CI_WORKFLOW)
        required_by_job = {
            "test": [
                "cargo test --workspace --locked",
                "cargo test -p hwpx-python --no-default-features --locked",
            ],
            "coverage": [
                "cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info",
            ],
            "clippy": [
                "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
                "cargo clippy -p hwpx-python --no-default-features --all-targets --locked -- -D warnings",
            ],
            "docs": [
                'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --locked --no-deps',
            ],
            "benchmark": [
                "cargo bench -p hwp-core --bench parse_benchmark --locked -- --test",
            ],
        }

        for job_name, commands in required_by_job.items():
            job = job_section(ci, job_name)
            for command in commands:
                with self.subTest(job=job_name, command=command):
                    self.assertIn(command, job)

    def test_ci_runs_rustdoc_with_warnings_as_errors(self) -> None:
        docs_job = job_section(workflow_text(CI_WORKFLOW), "docs")

        self.assertIn(
            'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --locked --no-deps',
            docs_job,
        )

    def test_ci_runs_hwpx_python_no_default_feature_tests(self) -> None:
        ci = workflow_text(CI_WORKFLOW)
        test_job = job_section(ci, "test")
        clippy_job = job_section(ci, "clippy")

        self.assertIn("cargo test -p hwpx-python --no-default-features --locked", test_job)
        self.assertIn(
            "cargo clippy -p hwpx-python --no-default-features --all-targets --locked -- -D warnings",
            clippy_job,
        )

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

    def test_ci_runs_python_packaging_checker_tests_and_artifact_checks(self) -> None:
        text = workflow_text(CI_WORKFLOW)

        required_commands = [
            "python packages/hwpx-python/scripts/check_workflows_test.py",
            "python packages/hwpx-python/scripts/check_no_production_panics.py",
            "python packages/hwpx-python/scripts/check_no_production_panics_test.py",
            "python packages/hwpx-python/scripts/check_no_production_debug_output.py",
            "python packages/hwpx-python/scripts/check_no_production_debug_output_test.py",
            "python packages/hwpx-python/scripts/check_no_production_todos.py",
            "python packages/hwpx-python/scripts/check_no_production_todos_test.py",
            "python packages/hwpx-python/scripts/check_pyo3_gil_release_test.py",
            "python packages/hwpx-python/scripts/check_wheel_contents_test.py",
            "python packages/hwpx-python/scripts/check_wheel_install_test.py",
            "python packages/hwpx-python/scripts/check_sdist_contents_test.py",
            "python packages/hwpx-python/scripts/check_sdist_install_test.py",
            "python packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_wheel_install.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "python packages/hwpx-python/scripts/check_sdist_install.py dist/*.tar.gz",
        ]

        for command in required_commands:
            with self.subTest(command=command):
                self.assertIn(command, text)

    def test_release_workflow_runs_checker_tests_before_artifact_upload(self) -> None:
        text = workflow_text(BUILD_WHEELS_WORKFLOW)

        required_commands = [
            "python packages/hwpx-python/scripts/check_workflows_test.py",
            "python packages/hwpx-python/scripts/check_no_production_panics.py",
            "python packages/hwpx-python/scripts/check_no_production_panics_test.py",
            "python packages/hwpx-python/scripts/check_no_production_debug_output.py",
            "python packages/hwpx-python/scripts/check_no_production_debug_output_test.py",
            "python packages/hwpx-python/scripts/check_no_production_todos.py",
            "python packages/hwpx-python/scripts/check_no_production_todos_test.py",
            "python packages/hwpx-python/scripts/check_pyo3_gil_release_test.py",
            "python packages/hwpx-python/scripts/check_wheel_contents_test.py",
            "python packages/hwpx-python/scripts/check_wheel_install_test.py",
            "python packages/hwpx-python/scripts/check_sdist_contents_test.py",
            "python packages/hwpx-python/scripts/check_sdist_install_test.py",
            "python packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_wheel_install.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "python packages/hwpx-python/scripts/check_sdist_install.py dist/*.tar.gz",
        ]

        for command in required_commands:
            with self.subTest(command=command):
                self.assertIn(command, text)

    def test_release_job_revalidates_downloaded_artifacts_before_github_release(self) -> None:
        release = job_section(workflow_text(BUILD_WHEELS_WORKFLOW), "release")

        required_order = [
            "actions/download-artifact@v8",
            "python packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "softprops/action-gh-release@v3",
        ]

        previous_index = -1
        for token in required_order:
            with self.subTest(token=token):
                index = release.find(token)
                self.assertGreater(index, previous_index)
                previous_index = index

    def test_publish_job_revalidates_downloaded_artifacts_before_pypi_publish(self) -> None:
        publish = job_section(workflow_text(BUILD_WHEELS_WORKFLOW), "publish-pypi")

        required_order = [
            "actions/download-artifact@v8",
            "python packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl",
            "python packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "pypa/gh-action-pypi-publish@release/v1",
        ]

        previous_index = -1
        for token in required_order:
            with self.subTest(token=token):
                index = publish.find(token)
                self.assertGreater(index, previous_index)
                previous_index = index

    def test_release_workflow_uses_tag_trigger_and_trusted_publishing_controls(self) -> None:
        workflow = workflow_text(BUILD_WHEELS_WORKFLOW)
        publish = job_section(workflow, "publish-pypi")

        self.assertIn("tags:", workflow)
        self.assertIn("- 'v*'", workflow)
        self.assertIn("environment:", publish)
        self.assertIn("name: pypi", publish)
        self.assertIn("id-token: write", publish)
        self.assertIn("pypa/gh-action-pypi-publish@release/v1", publish)


if __name__ == "__main__":
    unittest.main()
