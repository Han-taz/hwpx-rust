#!/usr/bin/env python3
"""Tests that release documentation stays aligned with CI gates."""

from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
RELEASE_DOC = ROOT / "docs" / "release.md"
ROOT_README = ROOT / "README.md"
PYTHON_README = ROOT / "packages" / "hwpx-python" / "README.md"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
BUILD_WHEELS_WORKFLOW = ROOT / ".github" / "workflows" / "build-wheels.yml"


def text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def collapsed_text(value: str) -> str:
    return " ".join(value.split())


class ReleaseDocsTest(unittest.TestCase):
    def test_pre_release_checks_match_locked_ci_gates(self) -> None:
        release = text(RELEASE_DOC)
        required_commands = [
            "cargo fmt --all -- --check",
            "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
            "cargo test --workspace --locked",
            "cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info",
            "cargo bench -p hwp-core --bench parse_benchmark --locked -- --test",
            "cargo check --manifest-path fuzz/Cargo.toml --locked",
            "cargo deny --locked check",
            "cargo audit",
            "cargo audit --file fuzz/Cargo.lock",
        ]

        for command in required_commands:
            with self.subTest(command=command):
                self.assertIn(command, release)

    def test_pre_release_checks_include_python_checker_tests(self) -> None:
        release = text(RELEASE_DOC)
        required_commands = [
            "python3 packages/hwpx-python/scripts/check_workflows_test.py",
            "python3 packages/hwpx-python/scripts/check_release_docs_test.py",
            "python3 packages/hwpx-python/scripts/check_no_production_panics.py",
            "python3 packages/hwpx-python/scripts/check_no_production_panics_test.py",
            "python3 packages/hwpx-python/scripts/check_no_production_debug_output.py",
            "python3 packages/hwpx-python/scripts/check_no_production_debug_output_test.py",
            "python3 packages/hwpx-python/scripts/check_no_production_todos.py",
            "python3 packages/hwpx-python/scripts/check_no_production_todos_test.py",
            "python3 packages/hwpx-python/scripts/check_release_versions.py",
            "python3 packages/hwpx-python/scripts/check_release_versions_test.py",
            "python3 packages/hwpx-python/scripts/check_pyo3_gil_release.py",
            "python3 packages/hwpx-python/scripts/check_pyo3_gil_release_test.py",
            "python3 packages/hwpx-python/scripts/check_wheel_contents_test.py",
            "python3 packages/hwpx-python/scripts/check_wheel_install_test.py",
            "python3 packages/hwpx-python/scripts/check_sdist_contents_test.py",
            "python3 packages/hwpx-python/scripts/check_sdist_install_test.py",
        ]

        for command in required_commands:
            with self.subTest(command=command):
                self.assertIn(command, release)

    def test_release_docs_validate_both_wheel_contents_and_import_install(self) -> None:
        release = text(RELEASE_DOC)

        self.assertIn(
            "python3 packages/hwpx-python/scripts/check_wheel_contents.py dist/*.whl",
            release,
        )
        self.assertIn(
            "python3 packages/hwpx-python/scripts/check_wheel_install.py dist/*.whl",
            release,
        )

    def test_release_docs_cover_sdist_artifact_validation(self) -> None:
        release = text(RELEASE_DOC)
        command_tokens = [
            "python3 packages/hwpx-python/scripts/check_sdist_contents.py dist/*.tar.gz",
            "python3 packages/hwpx-python/scripts/check_sdist_install.py dist/*.tar.gz",
        ]
        prose_tokens = [
            "Source distributions must include the Rust workspace sources required for local builds",
            "include Python typing files",
            "metadata versions synchronized",
        ]

        for token in command_tokens:
            with self.subTest(token=token):
                self.assertIn(token, release)
        release_prose = collapsed_text(release)
        for token in prose_tokens:
            with self.subTest(token=token):
                self.assertIn(token, release_prose)

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

    def test_distribution_docs_use_hwpxkit_name(self) -> None:
        self.assertIn("pip install hwpxkit", text(ROOT_README))
        self.assertIn("pip install hwpxkit", text(PYTHON_README))
        self.assertIn("PyPI project: `hwpxkit`", text(RELEASE_DOC))
        self.assertIn("https://pypi.org/p/hwpxkit", text(BUILD_WHEELS_WORKFLOW))

        doc_paths = [
            ROOT_README,
            PYTHON_README,
            RELEASE_DOC,
            BUILD_WHEELS_WORKFLOW,
            *sorted((ROOT / "docs" / "plans").glob("*.md")),
        ]
        forbidden_tokens = [
            "pip install hwpx\n",
            "PyPI project: `hwpx`",
            "https://pypi.org/p/hwpx\n",
            "hwpx-0.2.0.tar.gz",
        ]

        for path in doc_paths:
            contents = text(path)
            relative_path = path.relative_to(ROOT)
            for token in forbidden_tokens:
                with self.subTest(path=relative_path, token=token):
                    self.assertNotIn(token, contents)

    def test_release_docs_compile_all_packaging_scripts(self) -> None:
        release = text(RELEASE_DOC)
        script_paths = sorted(
            path.relative_to(ROOT).as_posix()
            for path in (ROOT / "packages" / "hwpx-python" / "scripts").glob("*.py")
        )

        for script_path in script_paths:
            with self.subTest(script=script_path):
                self.assertIn(script_path, release)

    def test_ci_and_release_workflows_run_this_doc_guard(self) -> None:
        command = "python packages/hwpx-python/scripts/check_release_docs_test.py"

        self.assertIn(command, text(CI_WORKFLOW))
        self.assertIn(command, text(BUILD_WHEELS_WORKFLOW))


if __name__ == "__main__":
    unittest.main()
