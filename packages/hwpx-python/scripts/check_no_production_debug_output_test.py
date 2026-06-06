#!/usr/bin/env python3
"""Tests for the production debug output checker."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import check_no_production_debug_output  # noqa: E402


def write_rust_file(root: Path, relative_path: str, source: str) -> Path:
    path = root / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(source, encoding="utf-8")
    return path


class ProductionDebugOutputCheckerTest(unittest.TestCase):
    def test_accepts_comments_strings_and_test_debug_output(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "src/lib.rs",
                r'''
pub fn parse() -> &'static str {
    // println!("doc example only");
    "eprintln!(\"not a macro call\")"
}

#[cfg(test)]
mod tests {
    #[test]
    fn prints_in_test() {
        println!("test-only output");
        dbg!(1);
    }
}
'''.lstrip(),
            )

            findings = check_no_production_debug_output.validate_sources([root])

            self.assertEqual(findings, [])

    def test_rejects_production_print_and_debug_macros(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "src/lib.rs",
                """
pub fn parse() {
    println!("debug");
    eprintln!("debug");
    dbg!(1);
}
""".lstrip(),
            )

            findings = check_no_production_debug_output.validate_sources([root])

            formatted = "\n".join(finding.format() for finding in findings)
            self.assertIn("println! macro", formatted)
            self.assertIn("eprintln! macro", formatted)
            self.assertIn("dbg! macro", formatted)

    def test_cli_exits_nonzero_when_production_debug_output_is_present(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "crates/hwp-core/src/lib.rs",
                'pub fn parse() { eprintln!("debug"); }\n',
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT_DIR / "check_no_production_debug_output.py"),
                ],
                cwd=root,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("forbidden production debug output", result.stderr)


if __name__ == "__main__":
    unittest.main()
