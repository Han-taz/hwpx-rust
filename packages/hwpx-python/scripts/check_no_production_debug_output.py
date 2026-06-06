#!/usr/bin/env python3
"""Reject direct stdout/stderr debug output in production Rust source."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from check_no_production_panics import production_lines, rust_files


DEFAULT_SOURCE_ROOTS = [
    Path("crates/hwp-core/src"),
    Path("packages/hwpx-python/src"),
]
FORBIDDEN_DEBUG_OUTPUT_RE = re.compile(r"\b(?P<api>dbg|println|eprintln)\s*!\s*")


@dataclass(frozen=True)
class Finding:
    path: Path
    line_number: int
    api: str
    line: str

    def format(self) -> str:
        return (
            f"{self.path}:{self.line_number}: forbidden production debug output "
            f"{self.api}! macro: {self.line.strip()}"
        )


def strip_strings_and_line_comment(line: str) -> str:
    code: list[str] = []
    in_string = False
    in_char = False
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
            code.append(" ")
            continue
        if char == "\\":
            escaped = in_string or in_char
            code.append(" ")
            continue
        if char == '"' and not in_char:
            in_string = not in_string
            code.append(" ")
            continue
        if char == "'" and not in_string:
            in_char = not in_char
            code.append(" ")
            continue
        if not in_string and not in_char and line[index : index + 2] == "//":
            break
        code.append(" " if in_string or in_char else char)
    return "".join(code)


def find_forbidden_debug_output(path: Path) -> list[Finding]:
    source = path.read_text(encoding="utf-8")
    findings: list[Finding] = []
    for line_number, line in production_lines(source):
        code = strip_strings_and_line_comment(line)
        for match in FORBIDDEN_DEBUG_OUTPUT_RE.finditer(code):
            findings.append(
                Finding(
                    path=path,
                    line_number=line_number,
                    api=match.group("api"),
                    line=line,
                )
            )
    return findings


def validate_sources(source_roots: Iterable[Path] = DEFAULT_SOURCE_ROOTS) -> list[Finding]:
    findings: list[Finding] = []
    for path in rust_files(source_roots):
        findings.extend(find_forbidden_debug_output(path))
    return findings


def main() -> int:
    findings = validate_sources()
    if findings:
        for finding in findings:
            print(finding.format(), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
