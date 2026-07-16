import dataclasses
import hashlib
import inspect
import os
import tempfile
import time
import unittest
from pathlib import Path
import sys
import subprocess

from tools.hwpx_loop.profile import baseline_v1
from tools.hwpx_loop.verifier import (
    CommandComparison,
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
    execute_command,
    validate_request,
)


BASE_SHA = "6d1371c704853e6b427296b98fc9da4d0c5e49c6"
CLIPPY_HEAD_SHA = "29f29641ef74829fc480bfbd53b53d09e3c4cde1"
FUZZ_HEAD_SHA = "6a0e394a812a4fb0b1e72dd4c2a5cd1f6892a917"


def fake_oci_execution(argv):
    profile = baseline_v1()
    if argv[1:3] == ("image", "inspect"):
        return Execution(0, ('["' + profile.oci_image + '"]\n').encode("ascii"), b"")
    if "toolchain" in argv and "list" in argv:
        return Execution(
            0,
            (profile.stable_toolchain + "\n" + profile.nightly_toolchain + "\n").encode("ascii"),
            b"",
        )
    if "which" in argv and "--toolchain" in argv:
        return Execution(
            0,
            (
                "/rustup-home/toolchains/"
                + profile.nightly_toolchain
                + "-x86_64-unknown-linux-gnu/bin/rustc\n"
            ).encode("ascii"),
            b"",
        )
    if "/tools/bin/cargo-fuzz" in argv:
        return Execution(0, b"cargo-fuzz 0.13.1\n", b"")
    if "rustc" in argv and "+1.97.0" in argv:
        return Execution(0, b"rustc 1.97.0\nrelease: 1.97.0\n", b"")
    if "rustc" in argv and "+nightly-2025-06-01" in argv:
        return Execution(
            0,
            b"rustc 1.89.0-nightly (4d08223c0 2025-05-31)\n"
            b"commit-hash: 4d08223c054cf5a56d9761ca925fd46ffebe7115\n"
            b"commit-date: 2025-05-31\nrelease: 1.89.0-nightly\n",
            b"",
        )
    workspace_mounts = [
        value for value in argv if value.endswith(",dst=/workspace,readonly")
    ]
    if workspace_mounts:
        checkout = workspace_mounts[0].split("src=", 1)[1].split(",dst=", 1)[0]
        return Execution(0, ("ran fake-oci run " + checkout).encode("utf-8"), b"")
    return Execution(0, b"cleanup", b"")


class CommandResultTest(unittest.TestCase):
    def test_is_immutable(self):
        result = CommandResult(command_id="clippy", exit_code=0, stdout="", stderr="")

        with self.assertRaises(dataclasses.FrozenInstanceError):
            result.exit_code = 1


class ExecutionTest(unittest.TestCase):
    def test_executor_preserves_non_utf8_output_as_raw_bytes(self):
        execution = execute_command(
            (sys.executable, "-c", "import os; os.write(1, b'\\xff')"),
            Path.cwd(),
            dict(os.environ),
            5,
        )

        self.assertEqual(execution.stdout, b"\xff")
        self.assertEqual(execution.stderr, b"")

    def test_timeout_terminates_the_process_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            marker = Path(temporary) / "descendant-ran"
            child = "import time, pathlib; time.sleep(.5); pathlib.Path(%r).write_text('bad')" % str(marker)
            parent = "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',%r]); time.sleep(5)" % child

            execution = execute_command(
                (sys.executable, "-c", parent), Path.cwd(), dict(os.environ), 0.1
            )
            time.sleep(0.6)

            self.assertTrue(execution.timed_out)
            self.assertFalse(marker.exists())


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
            (["improved", "pre_existing_failure"], ("candidate_pass", 0)),
            (["new_regression", "inconclusive"], ("infrastructure_blocked", 30)),
            (["invalid_run"], ("invalid_run", 40)),
            (["invalid_run", "inconclusive"], ("invalid_run", 40)),
        ]

        for classifications, expected in cases:
            with self.subTest(expected=expected):
                verdict = overall_verdict(classifications)
                self.assertEqual((verdict.name, verdict.exit_code), expected)

    def test_comparisons_mark_the_objective_explicitly(self):
        self.assertIn("objective", {field.name for field in dataclasses.fields(CommandComparison)})

    def test_objective_aware_verdicts_handle_mixed_results(self):
        passed = self.result(0)
        failed = self.result(1, "failure")

        def comparison(command_id, classification, objective):
            return CommandComparison(command_id, failed, passed, classification, objective)

        cases = [
            (
                [comparison("objective", "improved", True), comparison("other", "pre_existing_failure", False)],
                ("candidate_pass", 0),
            ),
            ([comparison("objective", "unchanged_pass", True)], ("objective_not_met", 21)),
            (
                [comparison("objective", "pre_existing_failure", True), comparison("other", "improved", False)],
                ("objective_not_met", 21),
            ),
            ([comparison("objective", "changed_failure", True)], ("repair_required", 20)),
            ([comparison("other", "inconclusive", False)], ("infrastructure_blocked", 30)),
            (
                [comparison("run", "invalid_run", False), comparison("other", "inconclusive", False)],
                ("invalid_run", 40),
            ),
        ]

        for comparisons, expected in cases:
            with self.subTest(expected=expected):
                verdict = overall_verdict(comparisons)
                self.assertEqual((verdict.name, verdict.exit_code), expected)

    def test_blocked_report_preserves_requested_objectives(self):
        request = VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA, ("stable-clippy",))
        verifier = DifferentialVerifier(Path.cwd(), Path.cwd().parent / "unused-artifacts")

        report = verifier._blocked_report(request, baseline_v1(), "blocked")

        objectives = {
            comparison.command_id: comparison.objective
            for comparison in report.comparisons
        }
        self.assertTrue(objectives["stable-clippy"])
        self.assertFalse(objectives["fuzz-list"])


class SerializationTest(unittest.TestCase):
    def test_normalizes_volatile_output_before_sha256(self):
        left = "2026-07-17T10:20:30Z /tmp/run-a finished in 1m 02s"
        right = "2025-01-01T00:00:00Z /tmp/run-b finished in 9m 59s"

        left_roots = {"/tmp/run-a": "<TEMP>"}
        right_roots = {"/tmp/run-b": "<TEMP>"}
        self.assertEqual(normalize_output(left, left_roots), normalize_output(right, right_roots))
        self.assertEqual(fingerprint(left, left_roots), fingerprint(right, right_roots))

    def test_canonical_json_is_utf8_sorted_compact_and_newline_terminated(self):
        value = {"z": "한글", "a": [2, 1]}
        expected = b'{"a":[2,1],"z":"\xed\x95\x9c\xea\xb8\x80"}\n'

        encoded = canonical_json(value)

        self.assertEqual(encoded, expected)
        self.assertEqual(hashlib.sha256(encoded).hexdigest(), fingerprint(encoded))

    def test_normalization_does_not_guess_unknown_temp_paths(self):
        value = "/tmp/user-owned 2026-07-17T10:20:30Z finished in 1.2s"

        self.assertEqual(
            normalize_output(value),
            "/tmp/user-owned 2026-07-17T10:20:30Z finished in <DURATION>",
        )

    def test_normalizes_raw_bytes_with_verifier_known_roots(self):
        value = b"/runs/a/checkout /runs/a/target /runs/a/tmp \xff"

        normalized = normalize_output(
            value,
            {
                "/runs/a/checkout": "<CHECKOUT>",
                "/runs/a/target": "<TARGET>",
                "/runs/a/tmp": "<TEMP>",
            },
        )

        self.assertEqual(normalized, "<CHECKOUT> <TARGET> <TEMP> \ufffd")

    def test_preserves_semantic_durations_outside_known_volatile_phrases(self):
        self.assertNotEqual(fingerprint("timeout 1s"), fingerprint("timeout 2s"))
        self.assertEqual(
            fingerprint("finished in 1.0s"), fingerprint("finished in 2.0s")
        )

    def test_preserves_semantic_dates_outside_volatile_timestamp_contexts(self):
        self.assertNotEqual(
            fingerprint("release cutoff 2026-07-17T10:20:30Z"),
            fingerprint("release cutoff 2027-07-17T10:20:30Z"),
        )
        self.assertEqual(
            fingerprint("timestamp: 2026-07-17T10:20:30Z"),
            fingerprint("timestamp: 2027-07-17T10:20:30Z"),
        )


class ProfileTest(unittest.TestCase):
    def test_baseline_v1_has_exact_ids_and_immutable_pins(self):
        profile = baseline_v1()

        self.assertEqual(profile.profile_id, "baseline-v1")
        self.assertEqual(profile.stable_toolchain, "1.97.0")
        self.assertEqual(profile.nightly_toolchain, "nightly-2025-06-01")
        self.assertEqual(profile.nightly_release, "1.89.0-nightly")
        self.assertEqual(
            profile.nightly_commit_hash,
            "4d08223c054cf5a56d9761ca925fd46ffebe7115",
        )
        self.assertEqual(profile.nightly_commit_date, "2025-05-31")
        self.assertEqual(profile.cargo_fuzz_version, "0.13.1")
        self.assertEqual(
            profile.oci_digest,
            "sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073",
        )
        self.assertEqual(profile.oci_image, "rust@" + profile.oci_digest)
        self.assertEqual(profile.setup_network, "bridge")
        self.assertEqual(profile.command_network, "none")
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
        self.assertEqual(
            [(command.command_id, command.argv, command.timeout_seconds) for command in profile.commands],
            [
                (
                    "stable-clippy",
                    (
                        "cargo",
                        "+1.97.0",
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--all-features",
                        "--locked",
                        "--",
                        "-D",
                        "warnings",
                    ),
                    2700,
                ),
                (
                    "fuzz-workspace-check",
                    (
                        "cargo",
                        "+nightly-2025-06-01",
                        "check",
                        "--manifest-path",
                        "fuzz/Cargo.toml",
                        "--locked",
                    ),
                    2700,
                ),
                (
                    "fuzz-list",
                    ("cargo", "+nightly-2025-06-01", "fuzz", "list"),
                    600,
                ),
                (
                    "fuzz-build-parse-auto",
                    (
                        "cargo",
                        "+nightly-2025-06-01",
                        "fuzz",
                        "build",
                        "parse_auto",
                    ),
                    2700,
                ),
                (
                    "fuzz-build-parse-hwpx",
                    (
                        "cargo",
                        "+nightly-2025-06-01",
                        "fuzz",
                        "build",
                        "parse_hwpx",
                    ),
                    2700,
                ),
            ],
        )

    def test_duplicate_and_timeout_validation_reasons_and_boundaries(self):
        profile = baseline_v1()
        request = VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA, ("stable-clippy",))

        duplicate = dataclasses.replace(
            profile, commands=profile.commands + (profile.commands[0],)
        )
        with self.assertRaisesRegex(InvalidRun, "^command IDs must be unique$"):
            validate_request(request, duplicate)

        for timeout in (0, True, 1.5, 3601):
            with self.subTest(timeout=timeout):
                changed = dataclasses.replace(
                    profile,
                    commands=(
                        dataclasses.replace(
                            profile.commands[0], timeout_seconds=timeout
                        ),
                    ),
                )
                with self.assertRaisesRegex(
                    InvalidRun,
                    "^command timeouts must be integer seconds from 1 through 3600$",
                ):
                    validate_request(request, changed)

        for timeout in (1, 3600):
            with self.subTest(timeout=timeout):
                changed = dataclasses.replace(
                    profile,
                    commands=(
                        dataclasses.replace(
                            profile.commands[0], timeout_seconds=timeout
                        ),
                    ),
                )
                with self.assertRaisesRegex(
                    InvalidRun,
                    "^only the exact registered baseline-v1 profile is accepted$",
                ):
                    validate_request(request, changed)

    def test_rejects_invalid_inputs_before_a_run(self):
        profile = baseline_v1()
        valid = VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA, ("stable-clippy",))
        invalid = [
            (VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA), profile, False),
            (VerificationRequest("main", CLIPPY_HEAD_SHA), profile, False),
            (VerificationRequest(BASE_SHA, BASE_SHA), profile, False),
            (VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA, ("missing",)), profile, False),
            (
                VerificationRequest(BASE_SHA, CLIPPY_HEAD_SHA, ("stable-clippy", "stable-clippy")),
                profile,
                False,
            ),
            (valid, dataclasses.replace(profile, stable_toolchain="stable"), False),
            (valid, dataclasses.replace(profile, nightly_toolchain="nightly"), False),
            (valid, dataclasses.replace(profile, cargo_fuzz_version="latest"), False),
            (valid, dataclasses.replace(profile, profile_id="renamed"), False),
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
            (
                valid,
                dataclasses.replace(
                    profile,
                    commands=(dataclasses.replace(profile.commands[0], timeout_seconds=True),),
                ),
                False,
            ),
            (
                valid,
                dataclasses.replace(
                    profile,
                    commands=(dataclasses.replace(profile.commands[0], timeout_seconds=3601),),
                ),
                False,
            ),
        ]

        for request, candidate_profile, dirty in invalid:
            with self.subTest(request=request, profile=candidate_profile, dirty=dirty):
                with self.assertRaises(InvalidRun):
                    validate_request(request, candidate_profile)

        validate_request(valid, profile)
        validate_request(
            VerificationRequest(BASE_SHA, FUZZ_HEAD_SHA, ("fuzz-build-parse-auto",)),
            profile,
        )

    def test_request_declares_objectives_and_runner_has_no_dirty_override(self):
        request_fields = {field.name for field in dataclasses.fields(VerificationRequest)}

        self.assertIn("objective_command_ids", request_fields)
        self.assertNotIn("dirty", inspect.signature(DifferentialVerifier.run).parameters)
        self.assertNotIn("dirty", inspect.signature(validate_request).parameters)


class GitIntegrationTest(unittest.TestCase):
    def make_repo(self, root):
        repo = Path(root) / "repo"
        repo.mkdir()

        def git(*arguments, input=None):
            return subprocess.run(
                ("git",) + arguments,
                cwd=str(repo),
                input=input,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=True,
            ).stdout.decode("ascii").strip()

        git("init", "-q")
        git("config", "user.name", "Verifier Test")
        git("config", "user.email", "verifier@example.invalid")
        (repo / "fixture.txt").write_text("base", encoding="utf-8")
        git("add", "fixture.txt")
        git("commit", "-q", "-m", "base")
        base = git("rev-parse", "HEAD")
        (repo / "fixture.txt").write_text("head", encoding="utf-8")
        git("commit", "-q", "-am", "head")
        head = git("rev-parse", "HEAD")
        blob = git("hash-object", "-w", "--stdin", input=b"not a commit")
        tree = git("rev-parse", "HEAD^{tree}")
        return repo, base, head, blob, tree

    def test_rejects_missing_blob_and_tree_objects_before_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, blob, tree = self.make_repo(temporary)
            for index, invalid_head in enumerate(("0" * 40, blob, tree)):
                with self.subTest(invalid_head=invalid_head):
                    artifacts = Path(temporary) / ("artifacts-" + str(index))
                    verifier = DifferentialVerifier(repo, artifacts)

                    with self.assertRaises(InvalidRun):
                        verifier.run(
                            VerificationRequest(base, invalid_head, ("stable-clippy",)),
                            baseline_v1(),
                        )

                    self.assertFalse(artifacts.exists())

    def test_rejects_actual_dirty_repository_before_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            (repo / "dirty.txt").write_text("dirty", encoding="utf-8")
            artifacts = Path(temporary) / "artifacts"

            with self.assertRaises(InvalidRun):
                DifferentialVerifier(repo, artifacts).run(
                    VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
                )

            self.assertFalse(artifacts.exists())

    def test_runs_every_command_once_per_side_in_the_pinned_oci_image(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"
            calls = []
            environments = []

            def executor(argv, cwd, env, timeout):
                calls.append(tuple(argv))
                environments.append((tuple(argv), dict(env)))
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                return fake_oci_execution(argv)

            report = DifferentialVerifier(
                repo, artifacts, executor=executor, oci_runtime="fake-oci"
            ).run(
                VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
            )

            image = baseline_v1().oci_image
            oci_runs = [call for call in calls if call[:2] == ("fake-oci", "run")]
            verification_runs = [
                call
                for call in oci_runs
                if any(command.argv == call[-len(command.argv) :] for command in baseline_v1().commands)
            ]
            self.assertEqual(len(verification_runs), len(baseline_v1().commands) * 2)
            for side in ("base", "head"):
                for command in baseline_v1().commands:
                    matches = [
                        call
                        for call in verification_runs
                        if call[-len(command.argv) :] == command.argv
                        and any(
                            ("/checkouts/" + side + ",dst=/workspace,readonly") in value
                            for value in call
                        )
                    ]
                    self.assertEqual(
                        len(matches),
                        1,
                        (side, command.command_id),
                    )
            self.assertTrue(all(image in call for call in oci_runs))
            self.assertTrue(all("--read-only" in call for call in oci_runs))
            self.assertTrue(all("--network" in call for call in oci_runs))
            self.assertTrue(all("--name" in call and "--cidfile" in call for call in oci_runs))
            names = [call[call.index("--name") + 1] for call in oci_runs]
            cidfiles = [call[call.index("--cidfile") + 1] for call in oci_runs]
            self.assertEqual(len(names), len(set(names)))
            self.assertEqual(len(cidfiles), len(set(cidfiles)))
            self.assertTrue(
                all(call[call.index("--network") + 1] == "none" for call in verification_runs)
            )
            install_runs = [call for call in oci_runs if "cargo-fuzz" in call and "install" in call]
            self.assertEqual(len(install_runs), 2)
            self.assertTrue(
                all(call[call.index("--network") + 1] == "bridge" for call in install_runs)
            )
            for destination in (
                "/target",
                "/cargo-home",
                "/rustup-home",
                "/tmp/hwpx-loop",
                "/tools",
            ):
                mounts = [
                    value
                    for call in verification_runs
                    for value in call
                    if value.endswith(",dst=" + destination)
                ]
                self.assertEqual(len({value.split(",dst=", 1)[0] for value in mounts}), 2)
            clones = [call for call in calls if call[:2] == ("git", "clone")]
            self.assertEqual(len(clones), 2)
            self.assertTrue(all("--no-local" in call and "--no-hardlinks" in call for call in clones))
            self.assertTrue(all("--shared" not in call for call in clones))
            git_environments = [env for call, env in environments if call[0] == "git"]
            self.assertTrue(
                all(
                    env.get("GIT_CONFIG_NOSYSTEM") == "1"
                    and env.get("GIT_CONFIG_GLOBAL") == os.devnull
                    and env.get("GIT_ATTR_NOSYSTEM") == "1"
                    for env in git_environments
                )
            )
            status_calls = [call for call in calls if call[:2] == ("git", "status")]
            self.assertTrue(all("--untracked-files=all" in call for call in status_calls))
            for side in ("base", "head"):
                alternates = artifacts / "checkouts" / side / ".git/objects/info/alternates"
                self.assertFalse(alternates.exists())
            self.assertEqual(report.verdict.name, "objective_not_met")
            self.assertIn("base", report.effective_versions)
            self.assertTrue((artifacts / "logs/base/clone.stdout.bin").exists())
            self.assertTrue((artifacts / "logs/base/cargo-fuzz-install.stdout.bin").exists())
            raw_command = artifacts / "logs/base/stable-clippy.stdout.bin"
            self.assertTrue(raw_command.read_bytes().startswith(b"ran fake-oci run"))
            self.assertIn("<CHECKOUT>", report.comparisons[0].base.normalized_stdout)

    def test_repeated_runs_have_identical_canonical_bytes_and_sha256(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                return fake_oci_execution(argv)

            outputs = []
            for index in range(2):
                artifacts = Path(temporary) / ("repeat-" + str(index))
                report = DifferentialVerifier(
                    repo, artifacts, executor=executor, oci_runtime="fake-oci"
                ).run(
                    VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
                )
                result = (artifacts / "result.json").read_bytes()
                outputs.append((result, report.sha256()))
                expected_sha256 = hashlib.sha256(result).hexdigest()
                self.assertEqual(report.sha256(), expected_sha256)
                self.assertEqual(
                    (artifacts / "result.sha256").read_text(encoding="ascii"),
                    expected_sha256 + "\n",
                )

            self.assertEqual(outputs[0], outputs[1])

    def test_rejects_checkout_mutation_immediately_after_setup_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"
            oci_runs = []

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                if argv[1:3] == ("image", "inspect"):
                    return fake_oci_execution(argv)
                if argv[1] == "run":
                    oci_runs.append(tuple(argv))
                if argv[1] == "run" and "install" in argv:
                    workspace_mount = next(
                        value for value in argv if value.endswith(",dst=/workspace,readonly")
                    )
                    checkout = workspace_mount.split("src=", 1)[1].split(",dst=", 1)[0]
                    (Path(checkout) / "mutated.txt").write_text("mutation", encoding="utf-8")
                    return Execution(1, b"failed", b"")
                return fake_oci_execution(argv)

            with self.assertRaises(InvalidRun):
                DifferentialVerifier(
                    repo, artifacts, executor=executor, oci_runtime="fake-oci"
                ).run(
                    VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
                )

            self.assertEqual(len(oci_runs), 1)
            self.assertFalse(any("rustc" in call for call in oci_runs))

    def test_blocks_when_effective_image_or_tool_version_is_not_pinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                if argv[1:3] == ("image", "inspect"):
                    return Execution(0, b'["rust@sha256:wrong"]\n', b"")
                return Execution(0, b"unexpected-version\n", b"")

            report = DifferentialVerifier(
                repo, artifacts, executor=executor, oci_runtime="fake-oci"
            ).run(
                VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
            )

            self.assertEqual(report.verdict.name, "infrastructure_blocked")
            self.assertFalse((artifacts / "logs/base/stable-clippy.stdout.bin").exists())

    def test_blocks_mismatched_verbose_nightly_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                if "rustc" in argv and "+nightly-2025-06-01" in argv:
                    return Execution(
                        0,
                        b"rustc 1.89.0-nightly\n"
                        b"commit-hash: 0000000000000000000000000000000000000000\n"
                        b"commit-date: 2025-06-01\n"
                        b"release: 1.89.0-nightly\n",
                        b"",
                    )
                return fake_oci_execution(argv)

            report = DifferentialVerifier(
                repo, artifacts, executor=executor, oci_runtime="fake-oci"
            ).run(
                VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
            )

            self.assertEqual(report.verdict.name, "infrastructure_blocked")
            self.assertFalse((artifacts / "logs/base/stable-clippy.stdout.bin").exists())

    def test_rejects_checkout_mutation_after_successful_verification_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"
            verification_runs = []

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                if argv[-len(baseline_v1().commands[0].argv) :] == baseline_v1().commands[0].argv:
                    verification_runs.append(tuple(argv))
                    workspace_mount = next(
                        value for value in argv if value.endswith(",dst=/workspace,readonly")
                    )
                    checkout = workspace_mount.split("src=", 1)[1].split(",dst=", 1)[0]
                    (Path(checkout) / "mutated.txt").write_text("mutation", encoding="utf-8")
                    return Execution(0, b"passed", b"")
                return fake_oci_execution(argv)

            with self.assertRaises(InvalidRun):
                DifferentialVerifier(
                    repo, artifacts, executor=executor, oci_runtime="fake-oci"
                ).run(
                    VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
                )

            self.assertEqual(len(verification_runs), 1)

    def test_timeout_cleans_up_daemon_container_and_captures_cleanup_logs(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head, _, _ = self.make_repo(temporary)
            artifacts = Path(temporary) / "artifacts"
            live = set()
            cleanup_actions = []

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                action = argv[1]
                if action == "run":
                    name = argv[argv.index("--name") + 1]
                    live.add(name)
                    return Execution(None, b"partial\xff", b"timeout", timed_out=True)
                name = argv[-1]
                cleanup_actions.append(action)
                if action == "stop":
                    return Execution(1, b"", b"stop failed")
                if action == "kill":
                    return Execution(0, b"killed", b"")
                if action == "wait":
                    return Execution(0, b"137\n", b"")
                if action == "inspect":
                    return Execution(0, b'{"State":{"Running":false}}\n', b"")
                if action == "rm":
                    live.discard(name)
                    return Execution(0, b"removed", b"")
                return Execution(1, b"", b"unexpected")

            report = DifferentialVerifier(
                repo, artifacts, executor=executor, oci_runtime="fake-oci"
            ).run(
                VerificationRequest(base, head, ("stable-clippy",)), baseline_v1()
            )

            self.assertEqual(report.verdict.name, "infrastructure_blocked")
            self.assertEqual(cleanup_actions, ["stop", "kill", "wait", "inspect", "rm"])
            self.assertEqual(live, set())
            log_dir = artifacts / "logs/base"
            for action in cleanup_actions:
                self.assertTrue(
                    (log_dir / ("stable-toolchain-install.cleanup-" + action + ".stdout.bin")).exists()
                )

    def test_detached_head_rejects_symbolic_ref_fatal_128(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifact_dir = Path(temporary) / "artifacts"
            artifact_dir.mkdir()
            sha = "a" * 40

            def executor(argv, cwd, env, timeout):
                if argv[:3] == ("git", "rev-parse", "HEAD"):
                    return Execution(0, (sha + "\n").encode("ascii"), b"")
                if argv[:2] == ("git", "status"):
                    return Execution(0, b"", b"")
                if argv[:3] == ("git", "symbolic-ref", "-q"):
                    return Execution(128, b"", b"fatal: corrupt repository\n")
                return Execution(1, b"", b"unexpected")

            verifier = DifferentialVerifier(
                Path(temporary), artifact_dir, executor=executor
            )

            with self.assertRaises(InvalidRun):
                verifier._assert_checkout("base", Path(temporary), sha, "boundary")

    def test_rejects_repository_and_artifact_path_containment_before_creation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo, base, head, _, _ = self.make_repo(temporary)
            outside = root / "outside"
            outside.mkdir()
            link_into_repo = outside / "artifact-link"
            link_into_repo.symlink_to(repo / "linked-artifacts")
            cases = (
                repo / "artifacts",
                repo / ".git" / "artifacts",
                root,
                link_into_repo,
            )

            def executor(argv, cwd, env, timeout):
                if argv[0] != "fake-oci":
                    return execute_command(argv, cwd, env, timeout)
                return fake_oci_execution(argv)

            for artifact_dir in cases:
                with self.subTest(artifact_dir=artifact_dir):
                    with self.assertRaises(InvalidRun):
                        DifferentialVerifier(
                            repo,
                            artifact_dir,
                            executor=executor,
                            oci_runtime="fake-oci",
                        ).run(
                            VerificationRequest(base, head, ("stable-clippy",)),
                            baseline_v1(),
                        )
                    self.assertFalse((artifact_dir / "request.json").exists())


@unittest.skipUnless(
    os.environ.get("HWPX_LOOP_REAL_OCI") == "1",
    "set HWPX_LOOP_REAL_OCI=1 to run the pinned Docker probe",
)
class RealOciIntegrationTest(unittest.TestCase):
    def test_exact_digest_restrictions_raw_bytes_and_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact_dir = root / "artifacts"
            artifact_dir.mkdir()
            checkout = root / "workspace"
            target = artifact_dir / "targets/probe"
            cargo_home = artifact_dir / "cargo-home/probe"
            rustup_home = artifact_dir / "rustup-home/probe"
            temp_dir = artifact_dir / "temp/probe"
            tools = artifact_dir / "tools/probe"
            for directory in (
                checkout,
                target,
                cargo_home,
                rustup_home,
                temp_dir,
                tools,
            ):
                directory.mkdir(parents=True)
            calls = []

            def executor(argv, cwd, env, timeout):
                calls.append(tuple(argv))
                return execute_command(argv, cwd, env, timeout)

            verifier = DifferentialVerifier(
                root / "repo",
                artifact_dir,
                executor=executor,
                oci_runtime="docker",
            )
            profile = baseline_v1()
            argv = verifier._oci_argv(
                profile,
                checkout,
                target,
                cargo_home,
                temp_dir,
                tools,
                "none",
                (
                    "sh",
                    "-c",
                    'test "$CARGO_HOME" = /cargo-home '
                    '&& test "$RUSTUP_HOME" = /rustup-home '
                    '&& test "$CARGO_TARGET_DIR" = /target '
                    '&& test "$TMPDIR" = /tmp/hwpx-loop '
                    '&& test "$HOME" = /tmp/hwpx-loop '
                    '&& test "$PATH" = '
                    "/tools/bin:/usr/local/cargo/bin:/usr/local/rustup/bin:"
                    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin "
                    '&& test -z "${HWPX_LOOP_SENTINEL+x}" '
                    "&& ! touch /workspace/forbidden 2>/dev/null "
                    "&& printf '\\377'",
                ),
            )

            os.environ["HWPX_LOOP_SENTINEL"] = "must-not-leak"
            try:
                execution = verifier._logged(
                    "probe", "real-oci-probe", argv, root, 120
                )
            finally:
                del os.environ["HWPX_LOOP_SENTINEL"]

            run = next(call for call in calls if call[:2] == ("docker", "run"))
            name = run[run.index("--name") + 1]
            self.assertEqual(execution.exit_code, 0)
            self.assertEqual(execution.stdout, b"\xff")
            self.assertIn(profile.oci_image, run)
            self.assertIn("--read-only", run)
            self.assertEqual(run[run.index("--network") + 1], "none")
            self.assertIn("CARGO_HOME=/cargo-home", run)
            self.assertIn("RUSTUP_HOME=/rustup-home", run)
            self.assertFalse((checkout / "forbidden").exists())
            self.assertEqual(
                (artifact_dir / "logs/probe/real-oci-probe.stdout.bin").read_bytes(),
                b"\xff",
            )
            self.assertTrue(
                (artifact_dir / "logs/probe/real-oci-probe.cleanup-rm.stdout.bin").exists()
            )
            inspected = execute_command(
                ("docker", "inspect", name), root, dict(os.environ), 30
            )
            self.assertNotEqual(inspected.exit_code, 0)


if __name__ == "__main__":
    unittest.main()
