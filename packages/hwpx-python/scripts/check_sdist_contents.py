#!/usr/bin/env python3
"""Validate packaged hwpxkit source distribution contents."""

from __future__ import annotations

import argparse
import ast
import re
import sys
import tarfile
import tomllib
from pathlib import PurePosixPath
from typing import Any


REQUIRED_FILES = {
    "PKG-INFO",
    "Cargo.lock",
    "Cargo.toml",
    "README.md",
    "crates/hwp-core/Cargo.toml",
    "crates/hwp-core/src/lib.rs",
    "crates/hwp-core/src/parser/hwpx/bindata.rs",
    "crates/hwp-core/src/parser/hwpx/mod.rs",
    "crates/hwp-core/src/parser/hwpx/container.rs",
    "crates/hwp-core/src/parser/hwpx/header.rs",
    "crates/hwp-core/src/parser/hwpx/package.rs",
    "crates/hwp-core/src/parser/hwpx/section.rs",
    "crates/hwp-core/src/parser/hwpx/xml_attr.rs",
    "crates/hwp-core/src/parser/hwpx/xml_budget.rs",
    "packages/hwpx-python/Cargo.toml",
    "pyproject.toml",
    "packages/hwpx-python/src/lib.rs",
    "python/hwpxkit/__init__.py",
    "python/hwpxkit/__init__.pyi",
    "python/hwpxkit/py.typed",
}

FORBIDDEN_COMPONENTS = {"__pycache__", ".venv", "target", "dist", ".maturin"}
FORBIDDEN_SUFFIXES = (".pyc", ".pyo", ".so", ".pyd", ".dylib", ".dll")
INIT_VERSION_RE = re.compile(r"^__version__ = [\"']([^\"']+)[\"']$", re.MULTILINE)
PKG_INFO_NAME_RE = re.compile(r"^Name: (.+)$", re.MULTILINE)
PKG_INFO_VERSION_RE = re.compile(r"^Version: (.+)$", re.MULTILINE)
STUB_WARNINGS_RE = re.compile(
    r"(?m)^\s*@property\s*\n\s*def warnings\(self\)\s*->\s*[^:]+:"
)
STUB_DIAGNOSTIC_REPORT_RE = re.compile(
    r"(?m)^\s*def diagnostic_report\(self\)\s*->\s*[^:]+:"
)
DIAGNOSTIC_SUMMARY_FIELDS = {
    "total",
    "max_items",
    "truncated",
    "by_severity",
    "by_category",
}
MAX_TEXT_MEMBER_SIZE = 1024 * 1024


def unsafe_path_reason(name: str) -> str | None:
    if not name:
        return "empty path"
    if name.startswith("/"):
        return "absolute path"
    if "\\" in name:
        return "backslash path separator"
    if ":" in name:
        return "colon path component"

    trimmed = name.rstrip("/")
    if not trimmed:
        return "empty path"

    parts = trimmed.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return "empty, current, or parent directory component"

    return None


def relative_name(name: str, root: str) -> str | None:
    trimmed = name.rstrip("/")
    prefix = f"{root}/"
    if trimmed == root:
        return ""
    if trimmed.startswith(prefix):
        return trimmed[len(prefix):]
    return None


def load_member_text(
    tar: tarfile.TarFile,
    member: tarfile.TarInfo,
    sdist_name: str,
    rel: str,
    errors: list[str],
) -> str:
    if member.size > MAX_TEXT_MEMBER_SIZE:
        errors.append(f"{sdist_name}: {rel} is too large")
        return ""

    file = tar.extractfile(member)
    if file is None:
        return ""
    try:
        return file.read().decode("utf-8")
    except UnicodeDecodeError as exc:
        errors.append(f"{sdist_name}: {rel} has invalid UTF-8: {exc}")
        return ""


def annotated_class_fields(source: str, class_name: str) -> set[str]:
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return set()

    for node in tree.body:
        if not isinstance(node, ast.ClassDef) or node.name != class_name:
            continue
        return {
            statement.target.id
            for statement in node.body
            if isinstance(statement, ast.AnnAssign)
            and isinstance(statement.target, ast.Name)
        }

    return set()


def load_toml_text(text: str, label: str, errors: list[str]) -> dict[str, Any]:
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        errors.append(f"{label}: invalid TOML: {exc}")
        return {}
    if not isinstance(data, dict):
        return {}
    return data


def package_version(data: dict[str, Any]) -> str | None:
    version = data.get("package", {}).get("version")
    return version if isinstance(version, str) else None


def check_versions(
    sdist_name: str,
    text_files: dict[str, str],
    errors: list[str],
) -> None:
    pkg_info = text_files.get("PKG-INFO", "")
    pkg_info_name = PKG_INFO_NAME_RE.search(pkg_info)
    if not pkg_info_name:
        errors.append(f"{sdist_name}: PKG-INFO is missing Name")
    elif pkg_info_name.group(1).lower() != "hwpxkit":
        errors.append(
            f"{sdist_name}: PKG-INFO name {pkg_info_name.group(1)!r} "
            "does not match 'hwpxkit'"
        )

    pkg_info_version = PKG_INFO_VERSION_RE.search(pkg_info)
    if not pkg_info_version:
        errors.append(f"{sdist_name}: PKG-INFO is missing Version")

    workspace_toml = load_toml_text(text_files.get("Cargo.toml", ""), "Cargo.toml", errors)
    workspace_version = (
        workspace_toml.get("workspace", {})
        .get("package", {})
        .get("version")
    )
    if not isinstance(workspace_version, str):
        errors.append(f"{sdist_name}: Cargo.toml is missing [workspace.package] version")
        workspace_version = None

    pyproject_toml = load_toml_text(
        text_files.get("pyproject.toml", ""),
        "pyproject.toml",
        errors,
    )
    pyproject_version = pyproject_toml.get("project", {}).get("version")
    if not isinstance(pyproject_version, str):
        errors.append(f"{sdist_name}: pyproject.toml is missing [project] version")

    python_cargo_toml = load_toml_text(
        text_files.get("packages/hwpx-python/Cargo.toml", ""),
        "packages/hwpx-python/Cargo.toml",
        errors,
    )
    python_cargo_version = package_version(python_cargo_toml)
    if python_cargo_version is None:
        errors.append(f"{sdist_name}: packages/hwpx-python/Cargo.toml is missing package version")

    init_py = text_files.get("python/hwpxkit/__init__.py", "")
    init_match = INIT_VERSION_RE.search(init_py)
    if not init_match:
        errors.append(f"{sdist_name}: hwpxkit __init__.py is missing __version__")
    init_version = init_match.group(1) if init_match else None

    versions = {
        "PKG-INFO": pkg_info_version.group(1) if pkg_info_version else None,
        "Cargo.toml": workspace_version,
        "pyproject.toml": pyproject_version,
        "packages/hwpx-python/Cargo.toml": python_cargo_version,
        "python/hwpxkit/__init__.py": init_version,
    }
    expected = workspace_version or next((version for version in versions.values() if version), None)
    if expected:
        for label, version in versions.items():
            if version is not None and version != expected:
                errors.append(
                    f"{sdist_name}: {label} version {version!r} "
                    f"does not match {expected!r}"
                )

    cargo_lock = load_toml_text(text_files.get("Cargo.lock", ""), "Cargo.lock", errors)
    lock_packages = cargo_lock.get("package", [])
    if not isinstance(lock_packages, list):
        return

    lock_versions: dict[str, str] = {}
    for package in lock_packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        version = package.get("version")
        if name in {"hwp-core", "hwpx-python"} and isinstance(version, str):
            lock_versions[name] = version

    for package_name in ("hwp-core", "hwpx-python"):
        version = lock_versions.get(package_name)
        if version is None:
            errors.append(f"{sdist_name}: Cargo.lock is missing workspace package {package_name}")
        elif expected and version != expected:
            errors.append(
                f"{sdist_name}: Cargo.lock package {package_name} version {version!r} "
                f"does not match {expected!r}"
            )


def validate_sdist(path: str) -> list[str]:
    errors: list[str] = []
    text_files: dict[str, str] = {}
    sdist_name = path_name(path)

    try:
        with tarfile.open(path, "r:gz") as tar:
            members = tar.getmembers()
            seen: set[str] = set()
            duplicate_names: set[str] = set()
            safe_members: list[tarfile.TarInfo] = []
            roots: set[str] = set()

            for member in members:
                name = member.name
                if name in seen:
                    duplicate_names.add(name)
                seen.add(name)

                reason = unsafe_path_reason(name)
                if reason:
                    errors.append(f"{sdist_name}: unsafe tar member path {name!r}: {reason}")
                    continue

                trimmed = name.rstrip("/")
                parts = trimmed.split("/")
                roots.add(parts[0])
                safe_members.append(member)

            if duplicate_names:
                errors.append(
                    f"{sdist_name}: duplicate tar member paths: "
                    f"{', '.join(sorted(duplicate_names))}"
                )

            if len(roots) != 1:
                errors.append(
                    f"{sdist_name}: expected exactly one top-level directory, "
                    f"found {', '.join(sorted(roots)) or 'none'}"
                )

            root = next(iter(roots), "")
            files: set[str] = set()
            for member in safe_members:
                rel = relative_name(member.name, root)
                if rel is None:
                    continue
                if not rel:
                    continue

                path_obj = PurePosixPath(rel)
                parts = set(path_obj.parts)
                if (
                    parts.intersection(FORBIDDEN_COMPONENTS)
                    or rel.lower().endswith(FORBIDDEN_SUFFIXES)
                ):
                    errors.append(f"{sdist_name}: contains generated artifact {rel}")

                if member.isfile():
                    files.add(rel)
                    if rel in REQUIRED_FILES or rel == "Cargo.lock":
                        text_files[rel] = load_member_text(
                            tar,
                            member,
                            sdist_name,
                            rel,
                            errors,
                        )

            missing = sorted(REQUIRED_FILES.difference(files))
            if missing:
                errors.append(
                    f"{sdist_name}: missing required source files: {', '.join(missing)}"
                )

            init_pyi = text_files.get("python/hwpxkit/__init__.pyi", "")
            if init_pyi and not STUB_WARNINGS_RE.search(init_pyi):
                errors.append(f"{sdist_name}: type stub is missing Document.warnings property")
            if init_pyi and not STUB_DIAGNOSTIC_REPORT_RE.search(init_pyi):
                errors.append(
                    f"{sdist_name}: type stub is missing Document.diagnostic_report method"
                )
            if init_pyi:
                summary_fields = annotated_class_fields(init_pyi, "DiagnosticSummary")
                missing_summary_fields = sorted(
                    DIAGNOSTIC_SUMMARY_FIELDS.difference(summary_fields)
                )
                if missing_summary_fields:
                    missing_labels = ", ".join(
                        f"DiagnosticSummary.{field}" for field in missing_summary_fields
                    )
                    errors.append(f"{sdist_name}: type stub is missing {missing_labels}")

            if not missing:
                check_versions(sdist_name, text_files, errors)
    except (tarfile.TarError, OSError) as exc:
        errors.append(f"{sdist_name}: failed to read sdist: {exc}")

    return errors


def path_name(path: str) -> str:
    return PurePosixPath(path).name


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sdists", nargs="+")
    args = parser.parse_args()

    errors: list[str] = []
    for sdist in args.sdists:
        errors.extend(validate_sdist(sdist))

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
