#!/usr/bin/env python3
"""Reject unresolved TODO-style markers in production Rust comments."""

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
FORBIDDEN_TODO_RE = re.compile(r"\b(?P<marker>TODO|FIXME|XXX|HACK)\b", re.IGNORECASE)


@dataclass(frozen=True)
class Finding:
    path: Path
    line_number: int
    marker: str
    line: str

    def format(self) -> str:
        return (
            f"{self.path}:{self.line_number}: forbidden production todo marker "
            f"{self.marker!r}: {self.line.strip()}"
        )


def raw_string_start(line: str, index: int) -> tuple[int, int] | None:
    prefix_len = 0
    if line.startswith("br", index):
        prefix_len = 2
    elif line.startswith("r", index):
        prefix_len = 1
    else:
        return None

    before = line[index - 1] if index > 0 else " "
    if before.isalnum() or before == "_":
        return None

    cursor = index + prefix_len
    hashes = 0
    while cursor < len(line) and line[cursor] == "#":
        hashes += 1
        cursor += 1

    if cursor < len(line) and line[cursor] == '"':
        return hashes, cursor + 1
    return None


def find_raw_string_end(line: str, index: int, hashes: int) -> int | None:
    delimiter = '"' + ("#" * hashes)
    close_index = line.find(delimiter, index)
    if close_index == -1:
        return None
    return close_index + len(delimiter)


def char_literal_end(line: str, index: int) -> int | None:
    cursor = index + 1
    escaped = False
    while cursor < len(line):
        char = line[cursor]
        if escaped:
            escaped = False
            cursor += 1
            continue
        if char == "\\":
            escaped = True
            cursor += 1
            continue
        if char == "'":
            return cursor + 1
        if char in " \t\r\n/":
            return None
        cursor += 1
    return None


def comment_fragments(lines: Iterable[tuple[int, str]]) -> Iterable[tuple[int, str]]:
    block_comment_depth = 0
    raw_string_hashes: int | None = None
    in_string = False
    escaped = False

    for line_number, line in lines:
        fragments: list[str] = []
        index = 0

        while index < len(line):
            if block_comment_depth > 0:
                if line.startswith("/*", index):
                    block_comment_depth += 1
                    index += 2
                    continue
                if line.startswith("*/", index):
                    block_comment_depth -= 1
                    index += 2
                    continue
                fragments.append(line[index])
                index += 1
                continue

            if raw_string_hashes is not None:
                end = find_raw_string_end(line, index, raw_string_hashes)
                if end is None:
                    break
                raw_string_hashes = None
                index = end
                continue

            if in_string:
                if escaped:
                    escaped = False
                    index += 1
                    continue
                if line[index] == "\\":
                    escaped = True
                    index += 1
                    continue
                if line[index] == '"':
                    in_string = False
                index += 1
                continue

            raw_start = raw_string_start(line, index)
            if raw_start is not None:
                hashes, content_index = raw_start
                end = find_raw_string_end(line, content_index, hashes)
                if end is None:
                    raw_string_hashes = hashes
                    break
                index = end
                continue

            if line.startswith("//", index):
                fragments.append(line[index + 2 :])
                break

            if line.startswith("/*", index):
                block_comment_depth = 1
                index += 2
                continue

            if line[index] == '"':
                in_string = True
                index += 1
                continue

            if line[index] == "'":
                end = char_literal_end(line, index)
                if end is not None:
                    index = end
                    continue

            index += 1

        if fragments:
            yield line_number, "".join(fragments)


def find_forbidden_todos(path: Path) -> list[Finding]:
    source = path.read_text(encoding="utf-8")
    findings: list[Finding] = []
    lines_by_number = dict(enumerate(source.splitlines(), start=1))
    for line_number, comment in comment_fragments(production_lines(source)):
        for match in FORBIDDEN_TODO_RE.finditer(comment):
            findings.append(
                Finding(
                    path=path,
                    line_number=line_number,
                    marker=match.group("marker").upper(),
                    line=lines_by_number[line_number],
                )
            )
    return findings


def validate_sources(source_roots: Iterable[Path] = DEFAULT_SOURCE_ROOTS) -> list[Finding]:
    findings: list[Finding] = []
    for path in rust_files(source_roots):
        findings.extend(find_forbidden_todos(path))
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
