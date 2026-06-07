#!/usr/bin/env python3
"""Tests for check_sdist_install.py."""

from __future__ import annotations

import importlib.util
import io
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_sdist_install.py")


def load_script():
    spec = importlib.util.spec_from_file_location("check_sdist_install", SCRIPT)
    if spec is None or spec.loader is None:
        raise AssertionError(f"could not load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def add_bytes(tar: tarfile.TarFile, name: str, data: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = 0o644
    tar.addfile(info, io.BytesIO(data))


def add_text(tar: tarfile.TarFile, name: str, text: str) -> None:
    add_bytes(tar, name, text.encode("utf-8"))


def make_sdist(
    path: Path,
    pkg_info: str | None = "Metadata-Version: 2.1\nName: hwpxkit\nVersion: 0.2.1\n",
) -> None:
    with tarfile.open(path, "w:gz") as tar:
        if pkg_info is not None:
            add_text(tar, "hwpxkit-0.2.1/PKG-INFO", pkg_info)


class CheckSdistInstallTest(unittest.TestCase):
    def test_reads_pkg_info_version(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            make_sdist(sdist)

            self.assertEqual(module.sdist_metadata_version(sdist), "0.2.1")

    def test_rejects_missing_pkg_info_version(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            make_sdist(sdist, pkg_info=None)

            with self.assertRaisesRegex(ValueError, "PKG-INFO"):
                module.sdist_metadata_version(sdist)

    def test_rejects_pkg_info_name_mismatch(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            make_sdist(
                sdist,
                pkg_info="Metadata-Version: 2.1\nName: not-hwpxkit\nVersion: 0.2.1\n",
            )

            with self.assertRaisesRegex(ValueError, "PKG-INFO name"):
                module.sdist_metadata_version(sdist)

    def test_rejects_unsafe_pkg_info_member_path(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            with tarfile.open(sdist, "w:gz") as tar:
                add_text(tar, "../PKG-INFO", "Version: 0.2.1\n")

            with self.assertRaisesRegex(ValueError, "unsafe PKG-INFO path"):
                module.sdist_metadata_version(sdist)

    def test_rejects_oversized_pkg_info(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            oversized_pkg_info = "Version: 0.2.1\n" + ("x" * (1024 * 1024 + 1))
            make_sdist(sdist, pkg_info=oversized_pkg_info)

            with self.assertRaisesRegex(ValueError, "PKG-INFO is too large"):
                module.sdist_metadata_version(sdist)

    def test_rejects_invalid_utf8_pkg_info(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            with tarfile.open(sdist, "w:gz") as tar:
                add_bytes(tar, "hwpxkit-0.2.1/PKG-INFO", b"\xff")

            with self.assertRaisesRegex(ValueError, "invalid UTF-8"):
                module.sdist_metadata_version(sdist)

    def test_install_commands_use_clean_venv_and_runtime_smoke(self) -> None:
        module = load_script()
        fixture = Path("/tmp/fixtures/linespacing.hwpx")
        commands = module.install_commands(
            Path("/tmp/venv/bin/python"),
            Path("/tmp/dist/hwpxkit-0.2.1.tar.gz"),
            "0.2.1",
            fixture,
        )

        self.assertEqual(commands[0][:3], ["/tmp/venv/bin/python", "-m", "pip"])
        self.assertIn("install", commands[0])
        self.assertIn("--no-deps", commands[0])
        self.assertIn("--force-reinstall", commands[0])
        self.assertIn("/tmp/dist/hwpxkit-0.2.1.tar.gz", commands[0])
        self.assertEqual(commands[1][:2], ["/tmp/venv/bin/python", "-c"])
        self.assertIn("import hwpxkit", commands[1][2])
        self.assertNotIn("import hwpx\n", commands[1][2])
        self.assertIn('find_spec("hwpx") is None', commands[1][2])
        self.assertIn("__version__", commands[1][2])
        self.assertIn("0.2.1", commands[1][2])
        self.assertIn("parse_file(str(fixture_path))", commands[1][2])
        self.assertIn("hwpxkit.parse(fixture_path.read_bytes())", commands[1][2])
        self.assertIn("to_markdown", commands[1][2])
        self.assertIn("to_html", commands[1][2])
        self.assertIn("to_json", commands[1][2])
        self.assertIn(str(fixture), commands[1][2])

    def test_reads_smoke_fixture_from_sdist(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            with tarfile.open(sdist, "w:gz") as tar:
                add_text(tar, "hwpxkit-0.2.1/PKG-INFO", "Name: hwpxkit\nVersion: 0.2.1\n")
                add_bytes(
                    tar,
                    "hwpxkit-0.2.1/crates/hwp-core/tests/fixtures/linespacing.hwpx",
                    b"fixture bytes",
                )

            self.assertEqual(module.sdist_smoke_fixture(sdist), b"fixture bytes")

    def test_rejects_missing_smoke_fixture(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            sdist = Path(tmp) / "hwpxkit-0.2.1.tar.gz"
            make_sdist(sdist)

            with self.assertRaisesRegex(ValueError, "smoke fixture"):
                module.sdist_smoke_fixture(sdist)


if __name__ == "__main__":
    unittest.main()
