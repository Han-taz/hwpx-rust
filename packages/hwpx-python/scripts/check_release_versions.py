#!/usr/bin/env python3
"""Check Rust and Python release metadata versions are synchronized."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path
from typing import Any


VERSION_RE = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")
INIT_VERSION_RE = re.compile(r"^__version__ = [\"']([^\"']+)[\"']$", re.MULTILINE)
WORKSPACE_PACKAGES = ("hwp-core", "hwpx-python")
PYPI_PROJECT_NAME = "hwpxkit"
PYTHON_MODULE_NAME = "hwpxkit._native"
PYTHON_RUST_LIB_NAME = "_native"


def default_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as file:
        return tomllib.load(file)


def package_version(path: Path) -> str | None:
    data = load_toml(path)
    package = data.get("package", {})
    version = package.get("version")
    if isinstance(version, str):
        return version

    workspace_version = package.get("version.workspace")
    if workspace_version is True:
        return None

    return None


def check_versions(root: Path) -> list[str]:
    errors: list[str] = []

    workspace_toml = load_toml(root / "Cargo.toml")
    workspace_version = (
        workspace_toml.get("workspace", {})
        .get("package", {})
        .get("version")
    )
    if not isinstance(workspace_version, str):
        return ["Cargo.toml: missing [workspace.package] version"]
    if not VERSION_RE.fullmatch(workspace_version):
        errors.append(f"Cargo.toml: workspace version {workspace_version!r} is not valid semver")

    package_paths = (
        root / "crates/hwp-core/Cargo.toml",
        root / "packages/hwpx-python/Cargo.toml",
    )
    for path in package_paths:
        version = package_version(path)
        if version is not None and version != workspace_version:
            errors.append(
                f"{path.relative_to(root)} version {version!r} "
                f"does not match workspace version {workspace_version!r}"
            )

    python_cargo = load_toml(root / "packages/hwpx-python/Cargo.toml")
    python_lib_name = python_cargo.get("lib", {}).get("name")
    if python_lib_name != PYTHON_RUST_LIB_NAME:
        errors.append(
            "packages/hwpx-python/Cargo.toml lib name "
            f"{python_lib_name!r} does not match {PYTHON_RUST_LIB_NAME!r}"
        )

    pyproject = load_toml(root / "packages/hwpx-python/pyproject.toml")
    pyproject_name = pyproject.get("project", {}).get("name")
    if pyproject_name != PYPI_PROJECT_NAME:
        errors.append(
            "packages/hwpx-python/pyproject.toml project name "
            f"{pyproject_name!r} does not match {PYPI_PROJECT_NAME!r}"
        )

    pyproject_version = pyproject.get("project", {}).get("version")
    if pyproject_version != workspace_version:
        errors.append(
            "packages/hwpx-python/pyproject.toml version "
            f"{pyproject_version!r} does not match workspace version {workspace_version!r}"
        )

    module_name = pyproject.get("tool", {}).get("maturin", {}).get("module-name")
    if module_name != PYTHON_MODULE_NAME:
        errors.append(
            "packages/hwpx-python/pyproject.toml module-name "
            f"{module_name!r} does not match {PYTHON_MODULE_NAME!r}"
        )

    init_path = root / "packages/hwpx-python/python/hwpxkit/__init__.py"
    if not init_path.is_file():
        errors.append("packages/hwpx-python/python/hwpxkit/__init__.py is missing")
    else:
        init_text = init_path.read_text(encoding="utf-8")
        init_match = INIT_VERSION_RE.search(init_text)
        if not init_match:
            errors.append("packages/hwpx-python/python/hwpxkit/__init__.py is missing __version__")
        elif init_match.group(1) != workspace_version:
            errors.append(
                "packages/hwpx-python/python/hwpxkit/__init__.py version "
                f"{init_match.group(1)!r} does not match workspace version {workspace_version!r}"
            )

    cargo_lock = load_toml(root / "Cargo.lock")
    lock_packages = cargo_lock.get("package", [])
    found_workspace_packages: set[str] = set()
    for package in lock_packages:
        name = package.get("name")
        if name not in WORKSPACE_PACKAGES:
            continue
        found_workspace_packages.add(name)
        lock_version = package.get("version")
        if lock_version != workspace_version:
            errors.append(
                f"Cargo.lock package {name} version {lock_version!r} "
                f"does not match workspace version {workspace_version!r}"
            )

    missing_lock_packages = sorted(set(WORKSPACE_PACKAGES) - found_workspace_packages)
    for package in missing_lock_packages:
        errors.append(f"Cargo.lock is missing workspace package {package}")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=default_root())
    args = parser.parse_args()

    errors = check_versions(args.root.resolve())
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
