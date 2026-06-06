#!/usr/bin/env python3
"""Validate packaged hwpx wheel contents."""

from __future__ import annotations

import argparse
import ast
import base64
import csv
import hashlib
import re
import sys
import zipfile
from email.parser import Parser
from pathlib import Path

DIAGNOSTIC_SUMMARY_FIELDS = {
    "total",
    "max_items",
    "truncated",
    "by_severity",
    "by_category",
}
REQUIRED_DIST_INFO_FILES = {"METADATA", "WHEEL", "RECORD"}
MAX_WHEEL_MEMBER_SIZE = 64 * 1024 * 1024
MAX_WHEEL_UNCOMPRESSED_SIZE = 256 * 1024 * 1024


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


def record_hash(data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode("ascii")
    return digest.rstrip("=")


def validate_record(
    wheel_name: str,
    member_data: dict[str, bytes],
    record_name: str,
) -> list[str]:
    errors: list[str] = []
    try:
        record_text = member_data.get(record_name, b"").decode("utf-8")
    except UnicodeDecodeError as exc:
        return [f"{wheel_name}: {record_name} has invalid UTF-8: {exc}"]

    rows = list(csv.reader(record_text.splitlines()))
    record_entries: dict[str, tuple[str, str]] = {}

    for row_number, row in enumerate(rows, start=1):
        if len(row) != 3:
            errors.append(f"{wheel_name}: RECORD row {row_number} has {len(row)} fields")
            continue
        member_name, hash_field, size_field = row
        if not member_name:
            errors.append(f"{wheel_name}: RECORD row {row_number} has empty path")
            continue
        if member_name in record_entries:
            errors.append(f"{wheel_name}: RECORD has duplicate entry {member_name!r}")
            continue
        record_entries[member_name] = (hash_field, size_field)

    member_names = set(member_data)
    recorded_names = set(record_entries)
    missing_entries = sorted(member_names.difference(recorded_names))
    if missing_entries:
        errors.append(
            f"{wheel_name}: RECORD is missing entries: {', '.join(missing_entries)}"
        )

    extra_entries = sorted(recorded_names.difference(member_names))
    if extra_entries:
        errors.append(
            f"{wheel_name}: RECORD references missing wheel members: {', '.join(extra_entries)}"
        )

    for member_name in sorted(member_names.intersection(recorded_names)):
        hash_field, size_field = record_entries[member_name]
        data = member_data[member_name]

        if member_name == record_name:
            if hash_field or size_field:
                errors.append(f"{wheel_name}: RECORD entry for itself must not include hash/size")
            continue

        expected_hash = f"sha256={record_hash(data)}"
        if hash_field != expected_hash:
            errors.append(f"{wheel_name}: RECORD hash mismatch for {member_name}")

        expected_size = str(len(data))
        if size_field != expected_size:
            errors.append(
                f"{wheel_name}: RECORD size mismatch for {member_name}: "
                f"{size_field!r} != {expected_size!r}"
            )

    return errors


def read_wheel_members(
    wheel_name: str,
    wheel: zipfile.ZipFile,
    infos: list[zipfile.ZipInfo],
    unsafe_names: set[str],
    errors: list[str],
) -> dict[str, bytes]:
    member_data: dict[str, bytes] = {}
    total_uncompressed = 0
    total_limit_reported = False

    for info in infos:
        name = info.filename
        if name in unsafe_names:
            continue

        total_uncompressed += info.file_size
        if info.file_size > MAX_WHEEL_MEMBER_SIZE:
            errors.append(f"{wheel_name}: {name} is too large")
            continue
        if total_uncompressed > MAX_WHEEL_UNCOMPRESSED_SIZE:
            if not total_limit_reported:
                errors.append(f"{wheel_name}: wheel uncompressed size is too large")
                total_limit_reported = True
            continue

        try:
            member_data[name] = wheel.read(info)
        except (RuntimeError, zipfile.BadZipFile, OSError) as exc:
            errors.append(f"{wheel_name}: failed to read wheel member {name!r}: {exc}")

    return member_data


def decode_wheel_text(
    wheel_name: str,
    member_name: str,
    data: bytes,
    errors: list[str],
) -> str:
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError as exc:
        errors.append(f"{wheel_name}: {member_name} has invalid UTF-8: {exc}")
        return ""


def validate_wheel_metadata(
    wheel_name: str,
    member_name: str,
    text: str,
) -> list[str]:
    errors: list[str] = []
    metadata = Parser().parsestr(text)

    wheel_version = metadata.get("Wheel-Version")
    if not wheel_version:
        errors.append(f"{wheel_name}: {member_name} is missing Wheel-Version")
    elif not wheel_version.startswith("1."):
        errors.append(
            f"{wheel_name}: {member_name} has unsupported Wheel-Version {wheel_version!r}"
        )

    root_is_purelib = metadata.get("Root-Is-Purelib")
    if root_is_purelib is None:
        errors.append(f"{wheel_name}: {member_name} is missing Root-Is-Purelib")
    elif root_is_purelib.lower() != "false":
        errors.append(
            f"{wheel_name}: {member_name} Root-Is-Purelib must be false "
            "for the native extension wheel"
        )

    tags = metadata.get_all("Tag", [])
    if not tags:
        errors.append(f"{wheel_name}: {member_name} is missing Tag")
    elif not any("-abi3-" in tag for tag in tags):
        errors.append(f"{wheel_name}: {member_name} Tag is not an abi3 wheel tag")

    return errors


def validate_wheel(path: Path) -> list[str]:
    errors: list[str] = []
    names: list[str] = []
    member_data: dict[str, bytes] = {}
    init_py = ""
    init_pyi = ""
    metadata = ""
    wheel_metadata = ""
    wheel_metadata_member_name: str | None = None

    try:
        with zipfile.ZipFile(path) as wheel:
            infos = wheel.infolist()
            names = [info.filename for info in infos]
            unsafe_names: set[str] = set()
            seen: set[str] = set()
            duplicate_names: set[str] = set()
            for name in names:
                if name in seen:
                    duplicate_names.add(name)
                seen.add(name)

                reason = unsafe_path_reason(name)
                if reason:
                    unsafe_names.add(name)
                    errors.append(f"{path.name}: unsafe wheel member path {name!r}: {reason}")

            if duplicate_names:
                errors.append(
                    f"{path.name}: duplicate wheel member paths: "
                    f"{', '.join(sorted(duplicate_names))}"
                )

            member_data = read_wheel_members(
                path.name,
                wheel,
                infos,
                unsafe_names,
                errors,
            )
            init_py = (
                decode_wheel_text(
                    path.name,
                    "hwpx/__init__.py",
                    member_data["hwpx/__init__.py"],
                    errors,
                )
                if "hwpx/__init__.py" in member_data
                else ""
            )
            init_pyi = (
                decode_wheel_text(
                    path.name,
                    "hwpx/__init__.pyi",
                    member_data["hwpx/__init__.pyi"],
                    errors,
                )
                if "hwpx/__init__.pyi" in member_data
                else ""
            )
            metadata_member_name = next(
                (name for name in names if name.endswith(".dist-info/METADATA")),
                None,
            )
            metadata = (
                decode_wheel_text(
                    path.name,
                    metadata_member_name,
                    member_data[metadata_member_name],
                    errors,
                )
                if metadata_member_name in member_data
                else ""
            )
            wheel_metadata_member_name = next(
                (name for name in names if name.endswith(".dist-info/WHEEL")),
                None,
            )
            wheel_metadata = (
                decode_wheel_text(
                    path.name,
                    wheel_metadata_member_name,
                    member_data[wheel_metadata_member_name],
                    errors,
                )
                if wheel_metadata_member_name in member_data
                else ""
            )
    except (zipfile.BadZipFile, OSError) as exc:
        errors.append(f"{path.name}: failed to read wheel: {exc}")
        return errors

    required = {"hwpx/__init__.py", "hwpx/__init__.pyi", "hwpx/py.typed"}
    missing = sorted(required.difference(names))
    if missing:
        errors.append(f"{path.name}: missing required package files: {', '.join(missing)}")

    dist_info_dirs = sorted(
        {
            name.split("/", 1)[0]
            for name in names
            if "/" in name and name.split("/", 1)[0].endswith(".dist-info")
        }
    )
    if len(dist_info_dirs) != 1:
        errors.append(
            f"{path.name}: expected exactly one dist-info directory, "
            f"found {', '.join(dist_info_dirs) or 'none'}"
        )
    else:
        dist_info_dir = dist_info_dirs[0]
        required_dist_info_files = {
            f"{dist_info_dir}/{filename}" for filename in REQUIRED_DIST_INFO_FILES
        }
        missing_dist_info_files = sorted(required_dist_info_files.difference(names))
        if missing_dist_info_files:
            errors.append(
                f"{path.name}: missing wheel dist-info files: "
                f"{', '.join(missing_dist_info_files)}"
            )
        else:
            record_name = f"{dist_info_dir}/RECORD"
            errors.extend(validate_record(path.name, member_data, record_name))
            if (
                wheel_metadata_member_name is not None
                and wheel_metadata_member_name in member_data
            ):
                errors.extend(
                    validate_wheel_metadata(
                        path.name,
                        wheel_metadata_member_name,
                        wheel_metadata,
                    )
                )

    forbidden = [
        name
        for name in names
        if "__pycache__/" in name
        or name.endswith(".pyc")
        or name.endswith(".pyo")
        or name.endswith("-darwin.so")
    ]
    if forbidden:
        errors.append(f"{path.name}: contains generated artifacts: {', '.join(forbidden)}")

    native_extensions = [
        name
        for name in names
        if name.startswith("hwpx/hwpx.") and name.endswith((".so", ".pyd"))
    ]
    if len(native_extensions) != 1:
        errors.append(
            f"{path.name}: expected exactly one native extension, found {len(native_extensions)}"
        )
    elif "abi3" not in native_extensions[0] and not native_extensions[0].endswith(".pyd"):
        errors.append(f"{path.name}: native extension is not tagged abi3: {native_extensions[0]}")

    if init_pyi and not re.search(
        r"(?m)^    @property\s*\n    def warnings\(self\)\s*->\s*[^:]+:",
        init_pyi,
    ):
        errors.append(f"{path.name}: type stub is missing Document.warnings property")

    if init_pyi and not re.search(
        r"(?m)^    def diagnostic_report\(self\)\s*->\s*[^:]+:",
        init_pyi,
    ):
        errors.append(f"{path.name}: type stub is missing Document.diagnostic_report method")

    if init_pyi:
        summary_fields = annotated_class_fields(init_pyi, "DiagnosticSummary")
        missing_summary_fields = sorted(DIAGNOSTIC_SUMMARY_FIELDS.difference(summary_fields))
        if missing_summary_fields:
            missing_labels = ", ".join(
                f"DiagnosticSummary.{field}" for field in missing_summary_fields
            )
            errors.append(f"{path.name}: type stub is missing {missing_labels}")

    metadata_version = re.search(r"^Version: (.+)$", metadata, re.MULTILINE)
    metadata_package_name = re.search(r"^Name: (.+)$", metadata, re.MULTILINE)
    init_version = re.search(r"^__version__ = [\"'](.+)[\"']$", init_py, re.MULTILINE)
    if not metadata_package_name:
        errors.append(f"{path.name}: missing wheel metadata name")
    elif metadata_package_name.group(1).lower() != "hwpx":
        errors.append(
            f"{path.name}: metadata name {metadata_package_name.group(1)!r} "
            "does not match 'hwpx'"
        )

    if not metadata_version:
        errors.append(f"{path.name}: missing wheel metadata version")
    elif not init_version:
        errors.append(f"{path.name}: missing hwpx.__version__")
    elif init_version.group(1) != metadata_version.group(1):
        errors.append(
            f"{path.name}: hwpx.__version__ {init_version.group(1)!r} "
            f"does not match metadata version {metadata_version.group(1)!r}"
        )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheels", nargs="+", type=Path)
    args = parser.parse_args()

    errors: list[str] = []
    for wheel in args.wheels:
        errors.extend(validate_wheel(wheel))

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
