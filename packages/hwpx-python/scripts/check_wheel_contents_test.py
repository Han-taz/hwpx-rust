#!/usr/bin/env python3
"""Tests for check_wheel_contents.py."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
import warnings
import zipfile
import base64
import hashlib
from collections.abc import Iterable
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_wheel_contents.py")
VERSION = "0.2.1"
RECORD_NAME = "hwpxkit-0.2.1.dist-info/RECORD"


def load_script():
    spec = importlib.util.spec_from_file_location("check_wheel_contents", SCRIPT)
    if spec is None or spec.loader is None:
        raise AssertionError(f"could not load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def record_hash(data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode("ascii")
    return digest.rstrip("=")


def with_record(files: dict[str, str | bytes]) -> dict[str, str | bytes]:
    recorded = dict(files)
    rows: list[str] = []
    for name, content in recorded.items():
        if name == RECORD_NAME:
            continue
        data = content if isinstance(content, bytes) else content.encode("utf-8")
        rows.append(f"{name},sha256={record_hash(data)},{len(data)}")
    rows.append(f"{RECORD_NAME},,")
    recorded[RECORD_NAME] = "\n".join(rows) + "\n"
    return recorded


def with_windows_record(files: dict[str, str | bytes]) -> dict[str, str | bytes]:
    recorded = with_record(files)
    record = recorded[RECORD_NAME]
    assert isinstance(record, str)
    recorded[RECORD_NAME] = record.replace("/", "\\")
    return recorded


def make_wheel(path: Path, files: dict[str, str | bytes]) -> None:
    make_wheel_entries(path, files.items())


def make_wheel_entries(
    path: Path,
    entries: Iterable[tuple[str, str | bytes]],
) -> None:
    with zipfile.ZipFile(path, "w") as wheel:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            for name, content in entries:
                data = content if isinstance(content, bytes) else content.encode("utf-8")
                wheel.writestr(name, data)


def valid_files(version: str = VERSION) -> dict[str, str | bytes]:
    return {
        "hwpxkit/__init__.py": f'__version__ = "{version}"\n',
        "hwpxkit/__init__.pyi": """
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
        "hwpxkit/py.typed": "",
        "hwpxkit/_native.abi3.so": b"\x7fELF",
        "hwpxkit-0.2.1.dist-info/METADATA": (
            f"Metadata-Version: 2.1\nName: hwpxkit\nVersion: {version}\n"
        ),
        "hwpxkit-0.2.1.dist-info/WHEEL": (
            "Wheel-Version: 1.0\n"
            "Generator: maturin\n"
            "Root-Is-Purelib: false\n"
            "Tag: cp38-abi3-macosx_11_0_arm64\n"
        ),
        RECORD_NAME: "",
    }


class CheckWheelContentsTest(unittest.TestCase):
    def run_checker(self, wheel: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(wheel)],
            text=True,
            capture_output=True,
            check=False,
        )

    def test_accepts_valid_wheel(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            make_wheel(wheel, with_record(valid_files()))

            result = self.run_checker(wheel)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_literal_glob_pattern_argument(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            make_wheel(wheel, with_record(valid_files()))

            result = self.run_checker(Path(tmp) / "*.whl")

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_windows_record_path_separators(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-win_amd64.whl"
            make_wheel(wheel, with_windows_record(valid_files()))

            result = self.run_checker(wheel)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_crlf_python_init_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-win_amd64.whl"
            files = valid_files()
            files["hwpxkit/__init__.py"] = '__version__ = "0.2.1"\r\n'
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_missing_diagnostic_report_stub(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit/__init__.pyi"] = """
class Document:
    @property
    def warnings(self) -> tuple[str, ...]: ...
""".lstrip()
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("diagnostic_report", result.stderr)

    def test_rejects_missing_required_files_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            del files["hwpxkit/__init__.pyi"]
            del files["hwpxkit/py.typed"]
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing required package files", result.stderr)
            self.assertIn("hwpxkit/__init__.pyi", result.stderr)
            self.assertIn("hwpxkit/py.typed", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_rejects_unreadable_wheel_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            wheel.write_bytes(b"not a zip file")

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("failed to read wheel", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_rejects_unsafe_member_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["../evil.py"] = ""
            files["/tmp/absolute.py"] = ""
            files[r"hwpx\backslash.py"] = ""
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsafe wheel member path", result.stderr)
            self.assertIn("../evil.py", result.stderr)
            self.assertIn("/tmp/absolute.py", result.stderr)
            self.assertIn("backslash path separator", result.stderr)

    def test_rejects_duplicate_member_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            entries = list(valid_files().items())
            entries.append(("hwpxkit/__init__.py", '__version__ = "0.2.1"\n'))
            make_wheel_entries(wheel, entries)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate wheel member paths", result.stderr)
            self.assertIn("hwpxkit/__init__.py", result.stderr)

    def test_rejects_oversized_member_before_record_validation(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            make_wheel(wheel, with_record(valid_files()))

            module.MAX_WHEEL_MEMBER_SIZE = 8
            errors = module.validate_wheel(wheel)

            self.assertTrue(
                any("hwpxkit/__init__.py is too large" in error for error in errors),
                errors,
            )

    def test_rejects_missing_wheel_dist_info_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            del files["hwpxkit-0.2.1.dist-info/WHEEL"]
            del files["hwpxkit-0.2.1.dist-info/RECORD"]
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing wheel dist-info files", result.stderr)
            self.assertIn("hwpxkit-0.2.1.dist-info/WHEEL", result.stderr)
            self.assertIn("hwpxkit-0.2.1.dist-info/RECORD", result.stderr)

    def test_rejects_metadata_name_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/METADATA"] = (
                "Metadata-Version: 2.1\nName: not-hwpxkit\nVersion: 0.2.1\n"
            )
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("metadata name", result.stderr)
            self.assertIn("not-hwpx", result.stderr)

    def test_rejects_invalid_utf8_metadata_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/METADATA"] = b"\xff"
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("invalid UTF-8", result.stderr)
            self.assertIn("METADATA", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_rejects_invalid_utf8_wheel_metadata_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/WHEEL"] = b"\xff"
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("invalid UTF-8", result.stderr)
            self.assertIn("WHEEL", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_rejects_empty_wheel_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/WHEEL"] = ""
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Wheel-Version", result.stderr)
            self.assertIn("Root-Is-Purelib", result.stderr)
            self.assertIn("Tag", result.stderr)

    def test_rejects_purelib_wheel_metadata_for_native_extension(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/WHEEL"] = (
                "Wheel-Version: 1.0\n"
                "Generator: maturin\n"
                "Root-Is-Purelib: true\n"
                "Tag: cp38-abi3-macosx_11_0_arm64\n"
            )
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Root-Is-Purelib", result.stderr)
            self.assertIn("false", result.stderr)

    def test_rejects_non_abi3_wheel_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit-0.2.1.dist-info/WHEEL"] = (
                "Wheel-Version: 1.0\n"
                "Generator: maturin\n"
                "Root-Is-Purelib: false\n"
                "Tag: py3-none-any\n"
            )
            make_wheel(wheel, with_record(files))

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("abi3", result.stderr)
            self.assertIn("Tag", result.stderr)

    def test_rejects_record_missing_wheel_member(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = with_record(valid_files())
            record = files[RECORD_NAME]
            assert isinstance(record, str)
            files[RECORD_NAME] = "\n".join(
                line for line in record.splitlines() if "hwpxkit/py.typed" not in line
            ) + "\n"
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RECORD is missing entries", result.stderr)
            self.assertIn("hwpxkit/py.typed", result.stderr)

    def test_rejects_record_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = with_record(valid_files())
            record = files[RECORD_NAME]
            assert isinstance(record, str)
            files[RECORD_NAME] = record.replace("sha256=", "sha256=broken", 1)
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RECORD hash mismatch", result.stderr)

    def test_rejects_record_size_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = with_record(valid_files())
            record = files[RECORD_NAME]
            assert isinstance(record, str)
            files[RECORD_NAME] = record.replace(",0\n", ",999\n", 1)
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RECORD size mismatch", result.stderr)

    def test_rejects_record_extra_entry(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = with_record(valid_files())
            record = files[RECORD_NAME]
            assert isinstance(record, str)
            files[RECORD_NAME] = record + "hwpxkit/missing.py,sha256=abc,1\n"
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RECORD references missing wheel members", result.stderr)

    def test_rejects_invalid_utf8_record_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files[RECORD_NAME] = b"\xff"
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("invalid UTF-8", result.stderr)
            self.assertIn("RECORD", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_rejects_missing_diagnostic_summary_metadata_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpxkit-0.2.1-cp38-abi3-macosx.whl"
            files = valid_files()
            files["hwpxkit/__init__.pyi"] = """
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
            make_wheel(wheel, files)

            result = self.run_checker(wheel)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("DiagnosticSummary.max_items", result.stderr)
            self.assertIn("DiagnosticSummary.truncated", result.stderr)


if __name__ == "__main__":
    unittest.main()
