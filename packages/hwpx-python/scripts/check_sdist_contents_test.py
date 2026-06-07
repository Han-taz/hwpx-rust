#!/usr/bin/env python3
"""Tests for check_sdist_contents.py."""

from __future__ import annotations

import io
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_sdist_contents.py")
ROOT = "hwpxkit-0.2.0"
VERSION = "0.2.0"


def add_bytes(tar: tarfile.TarFile, name: str, data: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = 0o644
    tar.addfile(info, io.BytesIO(data))


def add_text(tar: tarfile.TarFile, name: str, text: str) -> None:
    add_bytes(tar, name, text.encode("utf-8"))


def make_sdist(path: Path, files: dict[str, str | bytes]) -> None:
    with tarfile.open(path, "w:gz") as tar:
        for name, content in files.items():
            data = content if isinstance(content, bytes) else content.encode("utf-8")
            add_bytes(tar, name, data)


def valid_files(version: str = VERSION) -> dict[str, str]:
    return {
        f"{ROOT}/PKG-INFO": f"Metadata-Version: 2.1\nName: hwpxkit\nVersion: {version}\n",
        f"{ROOT}/Cargo.lock": f"""
version = 4

[[package]]
name = "hwp-core"
version = "{version}"

[[package]]
name = "hwpx-python"
version = "{version}"
""".lstrip(),
        f"{ROOT}/Cargo.toml": f"""
[workspace.package]
version = "{version}"
""".lstrip(),
        f"{ROOT}/README.md": "# hwpx\n",
        f"{ROOT}/crates/hwp-core/Cargo.toml": """
[package]
name = "hwp-core"
version.workspace = true
""".lstrip(),
        f"{ROOT}/crates/hwp-core/src/lib.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/bindata.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/mod.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/container.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/header.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/package.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/section.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/xml_attr.rs": "",
        f"{ROOT}/crates/hwp-core/src/parser/hwpx/xml_budget.rs": "",
        f"{ROOT}/packages/hwpx-python/Cargo.toml": f"""
[package]
name = "hwpx-python"
version = "{version}"
""".lstrip(),
        f"{ROOT}/pyproject.toml": f"""
[project]
name = "hwpxkit"
version = "{version}"
""".lstrip(),
        f"{ROOT}/packages/hwpx-python/src/lib.rs": "",
        f"{ROOT}/python/hwpx/__init__.py": (
            f'__version__ = "{version}"\n'
        ),
        f"{ROOT}/python/hwpx/__init__.pyi": """
from typing import TypedDict

class DiagnosticSummary(TypedDict):
    total: int
    max_items: int
    truncated: bool
    by_severity: dict[str, int]
    by_category: dict[str, int]

class DiagnosticReport(TypedDict):
    summary: DiagnosticSummary

class Document:
    @property
    def warnings(self) -> tuple[str, ...]: ...
    def diagnostic_report(self) -> DiagnosticReport: ...
""".lstrip(),
        f"{ROOT}/python/hwpx/py.typed": "",
    }


class CheckSdistContentsTest(unittest.TestCase):
    def run_checker(self, sdist: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(sdist)],
            text=True,
            capture_output=True,
            check=False,
        )

    def test_accepts_valid_sdist(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            make_sdist(sdist, valid_files())

            result = self.run_checker(sdist)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_generated_artifacts_and_missing_required_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            del files[f"{ROOT}/python/hwpx/py.typed"]
            files[f"{ROOT}/packages/hwpx-python/.venv/bin/python"] = ""
            files[f"{ROOT}/packages/hwpx-python/scripts/__pycache__/checker.pyc"] = ""
            files[f"{ROOT}/packages/hwpx-python/hwpx.cpython-314-darwin.so"] = ""
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(".venv", result.stderr)
            self.assertIn("__pycache__", result.stderr)
            self.assertIn("py.typed", result.stderr)
            self.assertIn(".so", result.stderr)

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

    def test_rejects_version_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/python/hwpx/__init__.py"] = (
                '__version__ = "0.2.1"\n'
            )
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("0.2.1", result.stderr)
            self.assertIn("0.2.0", result.stderr)

    def test_rejects_pkg_info_name_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/PKG-INFO"] = (
                "Metadata-Version: 2.1\nName: not-hwpxkit\nVersion: 0.2.0\n"
            )
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("PKG-INFO name", result.stderr)
            self.assertIn("not-hwpx", result.stderr)

    def test_rejects_missing_diagnostic_report_stub(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/python/hwpx/__init__.pyi"] = """
class Document:
    @property
    def warnings(self) -> tuple[str, ...]: ...
""".lstrip()
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("diagnostic_report", result.stderr)

    def test_rejects_missing_diagnostic_summary_metadata_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/python/hwpx/__init__.pyi"] = """
from typing import TypedDict

class DiagnosticSummary(TypedDict):
    total: int
    by_severity: dict[str, int]
    by_category: dict[str, int]

class DiagnosticReport(TypedDict):
    summary: DiagnosticSummary

class Document:
    @property
    def warnings(self) -> tuple[str, ...]: ...
    def diagnostic_report(self) -> DiagnosticReport: ...
""".lstrip()
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("DiagnosticSummary.max_items", result.stderr)
            self.assertIn("DiagnosticSummary.truncated", result.stderr)

    def test_rejects_malicious_member_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/../evil.py"] = ""
            files["/tmp/absolute.py"] = ""
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsafe tar member path", result.stderr)

    def test_rejects_oversized_required_text_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/PKG-INFO"] += "x" * (1024 * 1024 + 1)
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("PKG-INFO is too large", result.stderr)

    def test_rejects_invalid_utf8_required_text_file_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.0.tar.gz"
            files = valid_files()
            files[f"{ROOT}/PKG-INFO"] = b"\xff"
            make_sdist(sdist, files)

            result = self.run_checker(sdist)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("invalid UTF-8", result.stderr)
            self.assertIn("PKG-INFO", result.stderr)
            self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
