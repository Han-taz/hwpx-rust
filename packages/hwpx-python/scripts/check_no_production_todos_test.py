#!/usr/bin/env python3
"""Tests for the production TODO/FIXME comment guard."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from check_no_production_todos import validate_sources


SCRIPT = Path(__file__).with_name("check_no_production_todos.py")


class ProductionTodoGuardTest(unittest.TestCase):
    def test_flags_line_and_block_comment_markers_in_production_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "lib.rs"
            source.write_text(
                "\n".join(
                    [
                        "pub fn parse() {",
                        "    // TODO: replace placeholder",
                        "    let _ = 1; /* FIXME: validate edge case */",
                        "}",
                    ]
                ),
                encoding="utf-8",
            )

            findings = validate_sources([source])

        self.assertEqual(["TODO", "FIXME"], [finding.marker for finding in findings])

    def test_ignores_string_literals_and_cfg_test_comments(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "lib.rs"
            source.write_text(
                "\n".join(
                    [
                        'pub const LABEL: &str = "TODO appears in user-visible text";',
                        "#[cfg(test)]",
                        "mod tests {",
                        "    // TODO: test fixture reminder is allowed",
                        "    fn sample() {}",
                        "}",
                    ]
                ),
                encoding="utf-8",
            )

            findings = validate_sources([source])

        self.assertEqual([], findings)

    def test_cli_fails_for_repo_layout_with_production_marker(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source_dir = root / "crates" / "hwp-core" / "src"
            source_dir.mkdir(parents=True)
            (source_dir / "lib.rs").write_text(
                "pub fn parse() {}\n// HACK: production workaround\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [sys.executable, str(SCRIPT)],
                cwd=root,
                text=True,
                capture_output=True,
                check=False,
            )

        self.assertNotEqual(0, result.returncode)
        self.assertIn("HACK", result.stderr)
        self.assertIn("forbidden production todo marker", result.stderr)


if __name__ == "__main__":
    unittest.main()
