import io
import json
import os
import tempfile
import unittest
from pathlib import Path
import subprocess
import sys

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
                    def __init__(
                        self, repo_dir, artifact_dir, executor=None, oci_runtime="docker"
                    ):
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
                        "--objective",
                        "stable-clippy",
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


class ProcessCliTest(unittest.TestCase):
    def make_repo(self, root):
        repo = Path(root) / "repo"
        repo.mkdir()

        def git(*arguments):
            return subprocess.run(
                ("git",) + arguments,
                cwd=str(repo),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=True,
            ).stdout.decode("ascii").strip()

        git("init", "-q")
        git("config", "user.name", "CLI Test")
        git("config", "user.email", "cli@example.invalid")
        (repo / "fixture.txt").write_text("base", encoding="utf-8")
        git("add", "fixture.txt")
        git("commit", "-q", "-m", "base")
        base = git("rev-parse", "HEAD")
        (repo / "fixture.txt").write_text("head", encoding="utf-8")
        git("commit", "-q", "-am", "head")
        return repo, base, git("rev-parse", "HEAD")

    def make_runtime(self, root):
        runtime = Path(root) / "fake-oci"
        image = (
            "rust@sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073"
        )
        runtime.write_text(
            "#!/usr/bin/env python3\n"
            "import sys\n"
            "if sys.argv[1:3] == ['image', 'inspect']:\n"
            "    print('[\\\"" + image + "\\\"]')\n"
            "elif 'toolchain' in sys.argv and 'list' in sys.argv:\n"
            "    print('1.97.0\\nnightly-2025-06-01')\n"
            "elif 'which' in sys.argv and '--toolchain' in sys.argv:\n"
            "    print('/rustup-home/toolchains/nightly-2025-06-01-x86_64-unknown-linux-gnu/bin/rustc')\n"
            "elif '/tools/bin/cargo-fuzz' in sys.argv:\n"
            "    print('cargo-fuzz 0.13.1')\n"
            "elif 'rustc' in sys.argv and '+1.97.0' in sys.argv:\n"
            "    print('rustc 1.97.0\\nrelease: 1.97.0')\n"
            "elif 'rustc' in sys.argv and '+nightly-2025-06-01' in sys.argv:\n"
            "    print('rustc 1.89.0-nightly (4d08223c0 2025-05-31)\\ncommit-hash: 4d08223c054cf5a56d9761ca925fd46ffebe7115\\ncommit-date: 2025-05-31\\nrelease: 1.89.0-nightly')\n"
            "else:\n"
            "    sys.stdout.buffer.write(('ran ' + ' '.join(sys.argv[1:])).encode())\n",
            encoding="utf-8",
        )
        runtime.chmod(0o755)
        return runtime

    def test_direct_help_from_arbitrary_cwd_is_json_only(self):
        script = Path(__file__).with_name("cli.py").resolve()
        with tempfile.TemporaryDirectory() as temporary:
            completed = subprocess.run(
                (sys.executable, str(script), "--help"),
                cwd=temporary,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

        payload = json.loads(completed.stdout.decode("utf-8"))
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(payload["exit_code"], 0)
        self.assertEqual(payload["action"], "help")
        self.assertIn("--objective ID", payload["usage"])
        self.assertNotIn("[--objective ID]", payload["usage"])
        self.assertEqual(completed.stderr, b"")
        self.assertTrue(completed.stdout.endswith(b"\n"))

    def test_direct_verify_uses_fake_oci_runtime_from_arbitrary_cwd(self):
        script = Path(__file__).with_name("cli.py").resolve()
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head = self.make_repo(temporary)
            runtime = self.make_runtime(temporary)
            artifacts = Path(temporary) / "artifacts"
            completed = subprocess.run(
                (
                    sys.executable,
                    str(script),
                    "verify",
                    "--repo",
                    str(repo),
                    "--base",
                    base,
                    "--head",
                    head,
                    "--objective",
                    "stable-clippy",
                    "--artifacts",
                    str(artifacts),
                    "--oci-runtime",
                    str(runtime),
                ),
                cwd=temporary,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            payload = json.loads(completed.stdout.decode("utf-8"))
            self.assertEqual(completed.returncode, 21)
            self.assertEqual(payload["verdict"], "objective_not_met")
            self.assertEqual(completed.stderr, b"")
            self.assertEqual((artifacts / "result.json").read_bytes(), completed.stdout)

    def test_module_help_and_malformed_arguments_are_json_only(self):
        workspace = Path(__file__).resolve().parents[2]
        cases = [
            ((sys.executable, "-m", "tools.hwpx_loop.cli", "--help"), 0, "help"),
            ((sys.executable, "-m", "tools.hwpx_loop.cli", "verify"), 40, "invalid_run"),
        ]
        for argv, exit_code, outcome in cases:
            with self.subTest(outcome=outcome):
                completed = subprocess.run(
                    argv,
                    cwd=str(workspace),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                payload = json.loads(completed.stdout.decode("utf-8"))
                self.assertEqual(completed.returncode, exit_code)
                self.assertEqual(payload.get("action", payload.get("verdict")), outcome)
                self.assertEqual(completed.stderr, b"")

    def test_missing_oci_runtime_is_canonical_infrastructure_json(self):
        script = Path(__file__).with_name("cli.py").resolve()
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head = self.make_repo(temporary)
            completed = subprocess.run(
                (
                    sys.executable,
                    str(script),
                    "verify",
                    "--repo",
                    str(repo),
                    "--base",
                    base,
                    "--head",
                    head,
                    "--objective",
                    "stable-clippy",
                    "--artifacts",
                    str(Path(temporary) / "artifacts"),
                    "--oci-runtime",
                    str(Path(temporary) / "missing-runtime"),
                ),
                cwd=temporary,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

        payload = json.loads(completed.stdout.decode("utf-8"))
        self.assertEqual(completed.returncode, 30)
        self.assertEqual(payload["verdict"], "infrastructure_blocked")
        self.assertEqual(completed.stderr, b"")

    def test_invalid_git_object_and_dirty_repo_exit_40_before_artifacts(self):
        script = Path(__file__).with_name("cli.py").resolve()
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head = self.make_repo(temporary)
            tree = subprocess.run(
                ("git", "rev-parse", "HEAD^{tree}"),
                cwd=str(repo),
                stdout=subprocess.PIPE,
                check=True,
            ).stdout.decode("ascii").strip()
            cases = [(tree, False), (head, True)]
            for index, (candidate_head, dirty) in enumerate(cases):
                with self.subTest(dirty=dirty):
                    marker = repo / "dirty.txt"
                    if dirty:
                        marker.write_text("dirty", encoding="utf-8")
                    artifacts = Path(temporary) / ("invalid-" + str(index))
                    completed = subprocess.run(
                        (
                            sys.executable,
                            str(script),
                            "verify",
                            "--repo",
                            str(repo),
                            "--base",
                            base,
                            "--head",
                            candidate_head,
                            "--objective",
                            "stable-clippy",
                            "--artifacts",
                            str(artifacts),
                        ),
                        cwd=temporary,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        check=False,
                    )
                    payload = json.loads(completed.stdout.decode("utf-8"))
                    self.assertEqual(completed.returncode, 40)
                    self.assertEqual(payload["verdict"], "invalid_run")
                    self.assertEqual(completed.stderr, b"")
                    self.assertFalse(artifacts.exists())
                    if marker.exists():
                        marker.unlink()

    def test_module_from_arbitrary_cwd_with_pythonpath_covers_process_outcomes(self):
        workspace = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            repo, base, head = self.make_repo(temporary)
            environment = dict(os.environ)
            environment["PYTHONPATH"] = str(workspace)
            runtime_argv = (
                sys.executable,
                "-m",
                "tools.hwpx_loop.cli",
                "verify",
                "--repo",
                str(repo),
                "--base",
                base,
                "--head",
                head,
                "--objective",
                "stable-clippy",
                "--artifacts",
                str(Path(temporary) / "runtime-artifacts"),
                "--oci-runtime",
                str(Path(temporary) / "missing-runtime"),
            )
            cases = (
                ((sys.executable, "-m", "tools.hwpx_loop.cli", "--help"), 0, "help"),
                ((sys.executable, "-m", "tools.hwpx_loop.cli", "verify"), 40, "invalid_run"),
                (runtime_argv, 30, "infrastructure_blocked"),
            )
            for argv, exit_code, outcome in cases:
                with self.subTest(outcome=outcome):
                    completed = subprocess.run(
                        argv,
                        cwd=temporary,
                        env=environment,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        check=False,
                    )
                    payload = json.loads(completed.stdout.decode("utf-8"))
                    self.assertEqual(completed.returncode, exit_code)
                    self.assertEqual(
                        payload.get("action", payload.get("verdict")), outcome
                    )
                    self.assertEqual(completed.stderr, b"")


if __name__ == "__main__":
    unittest.main()
