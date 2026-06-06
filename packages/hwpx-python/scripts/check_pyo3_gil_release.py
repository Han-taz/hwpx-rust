#!/usr/bin/env python3
"""Check that CPU-bound PyO3 bindings release the GIL."""

from __future__ import annotations

import re
import sys
from pathlib import Path


REQUIRED_DETACH_FUNCTIONS = [
    "to_markdown",
    "to_html",
    "to_json",
    "get_text",
    "parse",
    "parse_file",
]

PYTHON_PARAM_RE = re.compile(r"(?:^|[,(]\s*)py\s*:\s*Python\s*<\s*'_\s*>")
PY_DETACH_RE = re.compile(r"\bpy\s*\.\s*detach\s*\(")


def find_function(source: str, function_name: str) -> tuple[str, str] | None:
    match = re.search(rf"\bfn\s+{re.escape(function_name)}\s*\(", source)
    if not match:
        return None

    brace_start = source.find("{", match.end())
    if brace_start == -1:
        return None

    signature = source[match.start() : brace_start]
    depth = 0
    for index in range(brace_start, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return signature, source[brace_start : index + 1]

    return None


def validate_source(source: str) -> list[str]:
    errors: list[str] = []
    for function_name in REQUIRED_DETACH_FUNCTIONS:
        function = find_function(source, function_name)
        if function is None:
            errors.append(f"missing function {function_name}")
            continue
        signature, body = function

        if not PYTHON_PARAM_RE.search(signature):
            errors.append(f"{function_name} does not accept py: Python<'_> for GIL release")

        if not PY_DETACH_RE.search(body):
            errors.append(f"{function_name} does not call py.detach around CPU-bound work")

    return errors


def main() -> int:
    source_path = Path("packages/hwpx-python/src/lib.rs")
    source = source_path.read_text()

    errors = validate_source(source)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
