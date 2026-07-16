import io
import json
import tempfile
import unittest
from pathlib import Path

from tools.hwpx_loop import cli
from tools.hwpx_loop.verifier import VerificationReport, Verdict


BASE_SHA = "6d1371c704853e6b427296b98fc9da4d0c5e49c6"
HEAD_SHA = "29f29641ef74829fc480bfbd53b53d09e3c4cde1"


class CliTest(unittest.TestCase):
    def test_verify_emits_only_canonical_json_and_uses_verdict_exit_code(self):
        reports = [
            ("candidate_pass", 0),
            ("repair_required", 20),
            ("objective_not_met", 21),
            ("infrastructure_blocked", 30),
        ]
        for name, exit_code in reports:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                output = io.StringIO()
                artifact_dir = Path(temporary) / "run"
                report = VerificationReport(
                    "baseline-v1", BASE_SHA, HEAD_SHA, (), Verdict(name, exit_code)
                )

                class FakeVerifier:
                    def __init__(self, repo_dir, artifact_dir, executor=None):
                        self.artifact_dir = artifact_dir

                    def run(self, request, profile):
                        self.request = request
                        return report

                actual = cli.main(
                    [
                        "verify",
                        "--base",
                        BASE_SHA,
                        "--head",
                        HEAD_SHA,
                        "--artifacts",
                        str(artifact_dir),
                    ],
                    stdout=output,
                    verifier_factory=FakeVerifier,
                )

                self.assertEqual(actual, exit_code)
                self.assertEqual(output.getvalue().encode("utf-8"), report.to_json())

    def test_invalid_request_is_json_and_exit_40_without_execution(self):
        output = io.StringIO()
        constructed = []

        def verifier_factory(*args, **kwargs):
            constructed.append((args, kwargs))
            self.fail("invalid input must not construct the runner")

        exit_code = cli.main(
            ["verify", "--base", "main", "--head", HEAD_SHA, "--artifacts", "run"],
            stdout=output,
            verifier_factory=verifier_factory,
        )

        payload = json.loads(output.getvalue())
        self.assertEqual(exit_code, 40)
        self.assertEqual(payload["verdict"], "invalid_run")
        self.assertEqual(payload["exit_code"], 40)
        self.assertEqual(constructed, [])
        self.assertTrue(output.getvalue().endswith("\n"))


if __name__ == "__main__":
    unittest.main()
