#!/usr/bin/env python3
"""Tests for the PyO3 GIL-release checker."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import check_pyo3_gil_release  # noqa: E402


def source_with_function(function_name: str, signature: str, body: str) -> str:
    return f"""
impl Document {{
    fn {function_name}({signature}) -> String {{
        {body}
    }}
}}
"""


def source_with_all_required(
    signature: str = "&self, py: Python<'_>",
    body: str = "py.detach(|| String::new())",
) -> str:
    return "\n".join(
        source_with_function(function_name, signature, body)
        for function_name in check_pyo3_gil_release.REQUIRED_DETACH_FUNCTIONS
    )


class PyO3GilReleaseCheckerTest(unittest.TestCase):
    def test_validates_required_functions_call_py_detach(self) -> None:
        errors = check_pyo3_gil_release.validate_source(source_with_all_required())

        self.assertEqual(errors, [])

    def test_rejects_detach_called_on_non_python_binding_name(self) -> None:
        errors = check_pyo3_gil_release.validate_source(
            source_with_all_required(body="worker.detach(|| String::new())")
        )

        self.assertIn(
            "to_markdown does not call py.detach around CPU-bound work",
            errors,
        )

    def test_rejects_python_parameter_with_wrong_name(self) -> None:
        errors = check_pyo3_gil_release.validate_source(
            source_with_all_required(signature="&self, not_py: Python<'_>")
        )

        self.assertIn(
            "to_markdown does not accept py: Python<'_> for GIL release",
            errors,
        )


if __name__ == "__main__":
    unittest.main()
