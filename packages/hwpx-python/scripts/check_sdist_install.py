#!/usr/bin/env python3
"""Install an hwpx sdist in a fresh virtualenv and smoke test the package."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import venv
from pathlib import Path


VERSION_RE = re.compile(r"^Version: (.+)$", re.MULTILINE)
NAME_RE = re.compile(r"^Name: (.+)$", re.MULTILINE)
MAX_PKG_INFO_SIZE = 1024 * 1024
MAX_SMOKE_FIXTURE_SIZE = 16 * 1024 * 1024
SMOKE_FIXTURE_RELATIVE_PATH = "crates/hwp-core/tests/fixtures/linespacing.hwpx"


def is_safe_pkg_info_path(name: str) -> bool:
    if not name or name.startswith("/") or "\\" in name or ":" in name:
        return False

    parts = name.rstrip("/").split("/")
    return len(parts) == 2 and parts[0] not in {"", ".", ".."} and parts[1] == "PKG-INFO"


def sdist_relative_name(name: str) -> str | None:
    if not name or name.startswith("/") or "\\" in name or ":" in name:
        return None

    parts = name.rstrip("/").split("/")
    if len(parts) < 2 or any(part in {"", ".", ".."} for part in parts):
        return None

    return "/".join(parts[1:])


def sdist_metadata_version(path: Path) -> str:
    with tarfile.open(path, "r:gz") as tar:
        pkg_info_members = []
        for member in tar.getmembers():
            if not member.isfile() or not member.name.endswith("/PKG-INFO"):
                continue
            if not is_safe_pkg_info_path(member.name):
                raise ValueError(f"{path.name}: unsafe PKG-INFO path {member.name!r}")
            pkg_info_members.append(member)

        if len(pkg_info_members) != 1:
            raise ValueError(f"{path.name}: expected exactly one PKG-INFO file")

        pkg_info_member = pkg_info_members[0]
        if pkg_info_member.size > MAX_PKG_INFO_SIZE:
            raise ValueError(f"{path.name}: PKG-INFO is too large")

        file = tar.extractfile(pkg_info_member)
        if file is None:
            raise ValueError(f"{path.name}: could not read PKG-INFO")
        try:
            pkg_info = file.read().decode("utf-8")
        except UnicodeDecodeError as exc:
            raise ValueError(f"{path.name}: PKG-INFO has invalid UTF-8: {exc}") from exc

    name = NAME_RE.search(pkg_info)
    if not name:
        raise ValueError(f"{path.name}: PKG-INFO is missing Name")
    if name.group(1).lower() != "hwpxkit":
        raise ValueError(
            f"{path.name}: PKG-INFO name {name.group(1)!r} does not match 'hwpxkit'"
        )

    version = VERSION_RE.search(pkg_info)
    if not version:
        raise ValueError(f"{path.name}: PKG-INFO is missing Version")
    return version.group(1)


def sdist_smoke_fixture(path: Path) -> bytes:
    with tarfile.open(path, "r:gz") as tar:
        fixture_members = []
        for member in tar.getmembers():
            if sdist_relative_name(member.name) == SMOKE_FIXTURE_RELATIVE_PATH:
                fixture_members.append(member)

        if len(fixture_members) != 1:
            raise ValueError(
                f"{path.name}: expected exactly one smoke fixture "
                f"{SMOKE_FIXTURE_RELATIVE_PATH!r}"
            )

        fixture_member = fixture_members[0]
        if not fixture_member.isfile():
            raise ValueError(f"{path.name}: smoke fixture is not a regular file")
        if fixture_member.size > MAX_SMOKE_FIXTURE_SIZE:
            raise ValueError(f"{path.name}: smoke fixture is too large")

        file = tar.extractfile(fixture_member)
        if file is None:
            raise ValueError(f"{path.name}: could not read smoke fixture")
        return file.read()


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
    sdist: Path,
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
            str(sdist),
        ],
        [python, "-c", smoke_code(expected_version, fixture_path)],
    ]


def run_command(command: list[str]) -> None:
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, check=True)


def install_and_smoke(sdist: Path) -> None:
    expected_version = sdist_metadata_version(sdist)
    fixture_data = sdist_smoke_fixture(sdist)
    with tempfile.TemporaryDirectory(prefix="hwpx-sdist-install-") as tmp:
        venv_dir = Path(tmp) / "venv"
        fixture_path = Path(tmp) / "linespacing.hwpx"
        fixture_path.write_bytes(fixture_data)
        venv.EnvBuilder(with_pip=True, clear=True).create(venv_dir)
        venv_python = venv_python_path(venv_dir)

        for command in install_commands(venv_python, sdist, expected_version, fixture_path):
            run_command(command)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sdists", nargs="+", type=Path)
    args = parser.parse_args()

    try:
        for sdist in args.sdists:
            install_and_smoke(sdist)
    except (subprocess.CalledProcessError, OSError, ValueError) as exc:
        print(exc, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
