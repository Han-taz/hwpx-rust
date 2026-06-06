#!/usr/bin/env python3
"""Reject panic-prone APIs in production Rust source."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


DEFAULT_SOURCE_ROOTS = [
    Path("crates/hwp-core/src"),
    Path("packages/hwpx-python/src"),
]
FORBIDDEN_API_RE = re.compile(
    r"(?P<api>\.(?:unwrap|expect)\s*\(|\b(?:panic|todo|unimplemented|unreachable)!\s*)"
)


@dataclass(frozen=True)
class Finding:
    path: Path
    line_number: int
    api: str
    line: str

    def format(self) -> str:
        return (
            f"{self.path}:{self.line_number}: forbidden production panic API "
            f"{self.api.strip()!r}: {self.line.strip()}"
        )


def brace_delta(line: str) -> int:
    return line.count("{") - line.count("}")


def production_lines(source: str) -> Iterable[tuple[int, str]]:
    skip_cfg_test_depth: int | None = None
    pending_cfg_test = False

    for line_number, line in enumerate(source.splitlines(), start=1):
        stripped = line.strip()

        if skip_cfg_test_depth is not None:
            skip_cfg_test_depth += brace_delta(line)
            if skip_cfg_test_depth <= 0:
                skip_cfg_test_depth = None
            continue

        if stripped == "#[cfg(test)]":
            pending_cfg_test = True
            continue

        if pending_cfg_test:
            if not stripped or stripped.startswith("#["):
                continue

            depth = brace_delta(line)
            if depth > 0:
                skip_cfg_test_depth = depth
            pending_cfg_test = False
            continue

        yield line_number, line


def rust_files(source_roots: Iterable[Path]) -> list[Path]:
    files: list[Path] = []
    for root in source_roots:
        if root.is_file() and root.suffix == ".rs":
            files.append(root)
        elif root.is_dir():
            files.extend(path for path in root.rglob("*.rs") if path.is_file())
    return sorted(files)


def find_forbidden_apis(path: Path) -> list[Finding]:
    source = path.read_text(encoding="utf-8")
    findings: list[Finding] = []
    for line_number, line in production_lines(source):
        for match in FORBIDDEN_API_RE.finditer(line):
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
        findings.extend(find_forbidden_apis(path))
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
