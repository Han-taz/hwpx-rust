#!/usr/bin/env python3
"""Install an hwpx wheel in a fresh virtualenv and smoke test the package."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
import venv
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
DEFAULT_FIXTURE = ROOT / "crates/hwp-core/tests/fixtures/linespacing.hwpx"
VERSION_RE = re.compile(r"^Version: (.+)$", re.MULTILINE)
NAME_RE = re.compile(r"^Name: (.+)$", re.MULTILINE)
MAX_METADATA_SIZE = 1024 * 1024
MAX_SMOKE_FIXTURE_SIZE = 16 * 1024 * 1024


def is_safe_metadata_path(name: str) -> bool:
    if not name or name.startswith("/") or "\\" in name or ":" in name:
        return False

    parts = name.rstrip("/").split("/")
    return (
        len(parts) == 2
        and parts[0] not in {"", ".", ".."}
        and parts[0].endswith(".dist-info")
        and parts[1] == "METADATA"
    )


def wheel_metadata_version(path: Path) -> str:
    with zipfile.ZipFile(path) as wheel:
        metadata_infos = [
            info
            for info in wheel.infolist()
            if info.filename.endswith(".dist-info/METADATA")
        ]

        if len(metadata_infos) != 1:
            raise ValueError(f"{path.name}: expected exactly one wheel METADATA file")

        metadata_info = metadata_infos[0]
        if not is_safe_metadata_path(metadata_info.filename):
            raise ValueError(
                f"{path.name}: unsafe wheel METADATA path {metadata_info.filename!r}"
            )
        if metadata_info.file_size > MAX_METADATA_SIZE:
            raise ValueError(f"{path.name}: wheel METADATA is too large")

        try:
            metadata = wheel.read(metadata_info).decode("utf-8")
        except UnicodeDecodeError as exc:
            raise ValueError(f"{path.name}: wheel METADATA has invalid UTF-8: {exc}") from exc

    name = NAME_RE.search(metadata)
    if not name:
        raise ValueError(f"{path.name}: wheel METADATA is missing Name")
    if name.group(1).lower() != "hwpxkit":
        raise ValueError(
            f"{path.name}: wheel METADATA name {name.group(1)!r} does not match 'hwpxkit'"
        )

    version = VERSION_RE.search(metadata)
    if not version:
        raise ValueError(f"{path.name}: wheel METADATA is missing Version")
    return version.group(1)


def validate_fixture(path: Path) -> None:
    if not path.is_file():
        raise ValueError(f"smoke fixture is not a regular file: {path}")
    if path.stat().st_size > MAX_SMOKE_FIXTURE_SIZE:
        raise ValueError(f"smoke fixture is too large: {path}")


def venv_python_path(venv_dir: Path) -> Path:
    if os.name == "nt":
        return venv_dir / "Scripts" / "python.exe"
    return venv_dir / "bin" / "python"


def smoke_code(expected_version: str, fixture_path: Path) -> str:
    return f"""
import importlib.metadata
import json
from pathlib import Path
import hwpx

expected_version = {expected_version!r}
fixture_path = Path({str(fixture_path)!r})
assert importlib.metadata.version("hwpxkit") == expected_version
assert hwpx.__version__ == expected_version
assert callable(hwpx.parse)
assert callable(hwpx.parse_file)

doc = hwpx.parse_file(str(fixture_path))
doc_from_bytes = hwpx.parse(fixture_path.read_bytes())
assert doc.section_count == doc_from_bytes.section_count
assert doc.section_count >= 1
assert isinstance(doc.version, str)

text = doc.get_text()
markdown = doc.to_markdown()
html = doc.to_html()
payload = json.loads(doc.to_json())
diagnostics = doc.diagnostic_report()

assert isinstance(text, str) and text
assert isinstance(markdown, str) and markdown
assert isinstance(html, str) and html
assert isinstance(payload, dict)
assert set(diagnostics) == {{"items", "summary"}}
""".strip()


def install_commands(
    venv_python: Path,
    wheel: Path,
    expected_version: str,
    fixture_path: Path,
) -> list[list[str]]:
    python = str(venv_python)
    return [
        [
            python,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--no-cache-dir",
            "--no-deps",
            "--force-reinstall",
            str(wheel),
        ],
        [python, "-c", smoke_code(expected_version, fixture_path)],
    ]


def run_command(command: list[str]) -> None:
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, check=True)


def install_and_smoke(wheel: Path, fixture_path: Path) -> None:
    expected_version = wheel_metadata_version(wheel)
    validate_fixture(fixture_path)
    with tempfile.TemporaryDirectory(prefix="hwpx-wheel-install-") as tmp:
        venv_dir = Path(tmp) / "venv"
        venv.EnvBuilder(with_pip=True, clear=True).create(venv_dir)
        venv_python = venv_python_path(venv_dir)

        for command in install_commands(venv_python, wheel, expected_version, fixture_path):
            run_command(command)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheels", nargs="+", type=Path)
    parser.add_argument("--fixture", type=Path, default=DEFAULT_FIXTURE)
    args = parser.parse_args()

    try:
        for wheel in args.wheels:
            install_and_smoke(wheel, args.fixture)
    except (subprocess.CalledProcessError, OSError, ValueError, zipfile.BadZipFile) as exc:
        print(exc, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
