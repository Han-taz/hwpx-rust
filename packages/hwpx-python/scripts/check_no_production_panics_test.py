#!/usr/bin/env python3
"""Tests for the production panic API checker."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import check_no_production_panics  # noqa: E402


def write_rust_file(root: Path, relative_path: str, source: str) -> Path:
    path = root / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(source, encoding="utf-8")
    return path


class ProductionPanicCheckerTest(unittest.TestCase):
    def test_accepts_safe_production_code_and_test_unwraps(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "src/lib.rs",
                """
pub fn parse(input: &[u8]) -> Result<usize, String> {
    Ok(input.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse() {
        parse(b"abc").unwrap();
        assert_eq!(Some(1).expect("test data"), 1);
    }
}
""".lstrip(),
            )

            findings = check_no_production_panics.validate_sources([root])

            self.assertEqual(findings, [])

    def test_rejects_production_unwrap_expect_and_panic_macros(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "src/lib.rs",
                """
pub fn unwrap_value(value: Option<u8>) -> u8 {
    value.unwrap()
}

pub fn expect_value(value: Option<u8>) -> u8 {
    value.expect("value should exist")
}

pub fn panic_now() {
    panic!("external input should not reach production panic paths");
}
""".lstrip(),
            )

            findings = check_no_production_panics.validate_sources([root])

            formatted = "\n".join(finding.format() for finding in findings)
            self.assertIn(".unwrap(", formatted)
            self.assertIn(".expect(", formatted)
            self.assertIn("panic!", formatted)

    def test_cli_exits_nonzero_when_production_panic_api_is_present(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_rust_file(
                root,
                "crates/hwp-core/src/lib.rs",
                "pub fn parse(value: Option<u8>) -> u8 { value.unwrap() }\n",
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT_DIR / "check_no_production_panics.py"),
                ],
                cwd=root,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("forbidden production panic API", result.stderr)


if __name__ == "__main__":
    unittest.main()
