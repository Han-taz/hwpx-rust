import dataclasses
import hashlib
import tempfile
import unittest
from pathlib import Path

from tools.hwpx_loop.profile import baseline_v1
from tools.hwpx_loop.verifier import (
    CommandResult,
    DifferentialVerifier,
    Execution,
    InvalidRun,
    VerificationRequest,
    canonical_json,
    classify,
    fingerprint,
    normalize_output,
    overall_verdict,
    validate_request,
)


BASE_SHA = "6d1371c704853e6b427296b98fc9da4d0c5e49c6"
CLIPPY_HEAD_SHA = "29f29641ef74829fc480bfbd53b53d09e3c4cde1"
FUZZ_HEAD_SHA = "6a0e394a812a4fb0b1e72dd4c2a5cd1f6892a917"


class CommandResultTest(unittest.TestCase):
    def test_is_immutable(self):
        result = CommandResult(command_id="clippy", exit_code=0, stdout="", stderr="")

        with self.assertRaises(dataclasses.FrozenInstanceError):
            result.exit_code = 1


class ClassificationTest(unittest.TestCase):
    def result(self, exit_code, output="", **kwargs):
        return CommandResult("command", exit_code, output, "", **kwargs)

    def test_classifies_all_differential_outcomes(self):
        cases = [
            (self.result(0), self.result(0), "unchanged_pass"),
            (self.result(1, "same"), self.result(0), "improved"),
            (self.result(1, "same"), self.result(1, "same"), "pre_existing_failure"),
            (self.result(1, "old"), self.result(1, "new"), "changed_failure"),
            (self.result(1, "same"), self.result(2, "same"), "changed_failure"),
            (self.result(0), self.result(1), "new_regression"),
            (self.result(0), self.result(0, timed_out=True), "inconclusive"),
        ]

        for base, head, expected in cases:
            with self.subTest(expected=expected):
                self.assertEqual(classify(base, head), expected)

    def test_maps_outcomes_to_verdicts_with_inconclusive_precedence(self):
        cases = [
            (["unchanged_pass", "improved"], ("candidate_pass", 0)),
            (["unchanged_pass", "new_regression"], ("repair_required", 20)),
            (["improved", "pre_existing_failure"], ("objective_not_met", 21)),
            (["new_regression", "inconclusive"], ("infrastructure_blocked", 30)),
            (["invalid_run"], ("invalid_run", 40)),
        ]

        for classifications, expected in cases:
            with self.subTest(expected=expected):
                verdict = overall_verdict(classifications)
                self.assertEqual((verdict.name, verdict.exit_code), expected)


class SerializationTest(unittest.TestCase):
    def test_normalizes_volatile_output_before_sha256(self):
        left = "2026-07-17T10:20:30Z /tmp/run-a finished in 1m 02s"
        right = "2025-01-01T00:00:00Z /tmp/run-b finished in 9m 59s"

        self.assertEqual(normalize_output(left), normalize_output(right))
        self.assertEqual(fingerprint(left), fingerprint(right))

    def test_canonical_json_is_utf8_sorted_compact_and_newline_terminated(self):
        value = {"z": "한글", "a": [2, 1]}
        expected = b'{"a":[2,1],"z":"\xed\x95\x9c\xea\xb8\x80"}\n'

        encoded = canonical_json(value)

        self.assertEqual(encoded, expected)
        self.assertEqual(hashlib.sha256(encoded).hexdigest(), fingerprint(encoded))


class ProfileTest(unittest.TestCase):
    def test_baseline_v1_has_exact_ids_and_immutable_pins(self):
        profile = baseline_v1()

        self.assertEqual(profile.profile_id, "baseline-v1")
        self.assertEqual(profile.stable_toolchain, "1.97.0")
        self.assertEqual(profile.nightly_toolchain, "nightly-2025-06-01")
        self.assertEqual(profile.cargo_fuzz_version, "0.13.1")
        self.assertEqual(
            profile.oci_digest,
            "sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073",
        )
        self.assertEqual(
            [command.command_id for command in profile.commands],
            [
                "stable-clippy",
                "fuzz-workspace-check",
                "fuzz-list",
                "fuzz-build-parse-auto",
                "fuzz-build-parse-hwpx",
            ],
        )

    def test_rejects_invalid_inputs_before_a_run(self):
        profile = baseline_v1()
        valid = VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA)
        invalid = [
            (VerificationRequest("main", CLIPPY_HEAD_SHA), profile, False),
            (VerificationRequest(BASE_SHA, BASE_SHA), profile, False),
            (valid, profile, True),
            (valid, dataclasses.replace(profile, stable_toolchain="stable"), False),
            (valid, dataclasses.replace(profile, nightly_toolchain="nightly"), False),
            (valid, dataclasses.replace(profile, cargo_fuzz_version="latest"), False),
            (valid, dataclasses.replace(profile, stable_toolchain="1.96.0"), False),
            (valid, dataclasses.replace(profile, oci_digest="sha256:" + "0" * 64), False),
            (
                valid,
                dataclasses.replace(
                    profile,
                    commands=(
                        dataclasses.replace(profile.commands[0], command_id="renamed"),
                    )
                    + profile.commands[1:],
                ),
                False,
            ),
            (
                valid,
                dataclasses.replace(profile, commands=profile.commands + (profile.commands[0],)),
                False,
            ),
            (
                valid,
                dataclasses.replace(
                    profile,
                    commands=(dataclasses.replace(profile.commands[0], timeout_seconds=0),),
                ),
                False,
            ),
        ]

        for request, candidate_profile, dirty in invalid:
            with self.subTest(request=request, profile=candidate_profile, dirty=dirty):
                with self.assertRaises(InvalidRun):
                    validate_request(request, candidate_profile, dirty)

        validate_request(valid, profile, False)
        validate_request(VerificationRequest(BASE_SHA, FUZZ_HEAD_SHA), profile, False)


class DifferentialVerifierTest(unittest.TestCase):
    def test_runs_once_per_isolated_checkout_and_preserves_raw_logs(self):
        calls = []

        def execute(argv, cwd, env, timeout):
            calls.append((tuple(argv), Path(cwd), dict(env), timeout))
            if argv[0] == "git":
                return Execution(0, "", "")
            if "install" in argv:
                return Execution(0, "installed", "")
            side = Path(cwd).name
            return Execution(1, "/tmp/" + side + " failed at 2026-07-17T10:20:30Z", "")

        profile = baseline_v1()
        profile = dataclasses.replace(
            profile, profile_id="test-v1", commands=(profile.commands[0],)
        )
        with tempfile.TemporaryDirectory() as temporary:
            artifact_dir = Path(temporary) / "artifacts"
            verifier = DifferentialVerifier(
                repo_dir=Path.cwd(), artifact_dir=artifact_dir, executor=execute
            )

            report = verifier.run(
                VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA), profile, dirty=False
            )

            command_calls = [call for call in calls if call[0] == profile.commands[0].argv]
            self.assertEqual(len(command_calls), 2)
            self.assertNotEqual(command_calls[0][1], command_calls[1][1])
            self.assertNotEqual(
                command_calls[0][2]["CARGO_TARGET_DIR"],
                command_calls[1][2]["CARGO_TARGET_DIR"],
            )
            self.assertEqual(report.comparisons[0].classification, "pre_existing_failure")
            self.assertEqual(report.verdict.name, "objective_not_met")
            self.assertEqual(
                (artifact_dir / "logs/base/stable-clippy.stdout.log").read_text(),
                "/tmp/base failed at 2026-07-17T10:20:30Z",
            )
            self.assertTrue((artifact_dir / "request.json").read_bytes().endswith(b"\n"))
            self.assertEqual((artifact_dir / "result.json").read_bytes(), report.to_json())


if __name__ == "__main__":
    unittest.main()
