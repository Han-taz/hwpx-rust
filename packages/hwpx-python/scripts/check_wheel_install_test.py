#!/usr/bin/env python3
"""Tests for check_wheel_install.py."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_wheel_install.py")


def load_script():
    spec = importlib.util.spec_from_file_location("check_wheel_install", SCRIPT)
    if spec is None or spec.loader is None:
        raise AssertionError(f"could not load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def make_wheel(
    path: Path,
    metadata: str | None = "Metadata-Version: 2.1\nName: hwpx\nVersion: 0.2.0\n",
) -> None:
    with zipfile.ZipFile(path, "w") as wheel:
        if metadata is not None:
            wheel.writestr("hwpx-0.2.0.dist-info/METADATA", metadata)


class CheckWheelInstallTest(unittest.TestCase):
    def test_reads_wheel_metadata_version(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpx-0.2.0-cp38-abi3-macosx_11_0_arm64.whl"
            make_wheel(wheel)

            self.assertEqual(module.wheel_metadata_version(wheel), "0.2.0")

    def test_rejects_missing_wheel_metadata(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpx-0.2.0-cp38-abi3-macosx_11_0_arm64.whl"
            make_wheel(wheel, metadata=None)

            with self.assertRaisesRegex(ValueError, "METADATA"):
                module.wheel_metadata_version(wheel)

    def test_rejects_wheel_metadata_name_mismatch(self) -> None:
        module = load_script()
        with tempfile.TemporaryDirectory() as tmp:
            wheel = Path(tmp) / "hwpx-0.2.0-cp38-abi3-macosx_11_0_arm64.whl"
            make_wheel(
                wheel,
                metadata="Metadata-Version: 2.1\nName: not-hwpx\nVersion: 0.2.0\n",
            )

            with self.assertRaisesRegex(ValueError, "METADATA name"):
                module.wheel_metadata_version(wheel)

    def test_install_commands_use_clean_venv_and_runtime_smoke(self) -> None:
        module = load_script()
        fixture = Path("/tmp/fixtures/linespacing.hwpx")
        commands = module.install_commands(
            Path("/tmp/venv/bin/python"),
            Path("/tmp/dist/hwpx-0.2.0-cp38-abi3-linux_x86_64.whl"),
            "0.2.0",
            fixture,
        )

        self.assertEqual(commands[0][:3], ["/tmp/venv/bin/python", "-m", "pip"])
        self.assertIn("install", commands[0])
        self.assertIn("--force-reinstall", commands[0])
        self.assertIn("/tmp/dist/hwpx-0.2.0-cp38-abi3-linux_x86_64.whl", commands[0])
        self.assertEqual(commands[1][:2], ["/tmp/venv/bin/python", "-c"])
        self.assertIn("__version__", commands[1][2])
        self.assertIn("parse_file(str(fixture_path))", commands[1][2])
        self.assertIn("hwpx.parse(fixture_path.read_bytes())", commands[1][2])
        self.assertIn("to_markdown", commands[1][2])
        self.assertIn("to_html", commands[1][2])
        self.assertIn("to_json", commands[1][2])
        self.assertIn(str(fixture), commands[1][2])


if __name__ == "__main__":
    unittest.main()
