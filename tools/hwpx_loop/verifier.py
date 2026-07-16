import hashlib
import json
import os
import re
import signal
import subprocess
import time
import uuid
from dataclasses import asdict, dataclass, field, is_dataclass
from pathlib import Path
from typing import Any, Callable, Dict, Mapping, Optional, Sequence, Tuple, Union

from tools.hwpx_loop.profile import VerificationProfile, baseline_v1


@dataclass(frozen=True)
class CommandResult:
    command_id: str
    exit_code: Optional[int]
    stdout: bytes
    stderr: bytes
    timed_out: bool = False
    infrastructure_error: Optional[str] = None
    duration_seconds: float = 0.0
    normalized_stdout: Optional[str] = None
    normalized_stderr: Optional[str] = None


@dataclass(frozen=True)
class Execution:
    exit_code: Optional[int]
    stdout: bytes
    stderr: bytes
    timed_out: bool = False
    infrastructure_error: Optional[str] = None
    duration_seconds: float = 0.0


@dataclass(frozen=True)
class Verdict:
    name: str
    exit_code: int


@dataclass(frozen=True)
class VerificationRequest:
    base_sha: str
    head_sha: str
    objective_command_ids: Tuple[str, ...] = ()


class InvalidRun(ValueError):
    pass


@dataclass(frozen=True)
class CommandComparison:
    command_id: str
    base: CommandResult
    head: CommandResult
    classification: str
    objective: bool = False


@dataclass(frozen=True)
class VerificationReport:
    profile_id: str
    base_sha: str
    head_sha: str
    comparisons: Tuple[CommandComparison, ...]
    verdict: Verdict
    effective_versions: Dict[str, Dict[str, str]] = field(default_factory=dict)

    @staticmethod
    def _result_data(result: CommandResult) -> Dict[str, Any]:
        normalized_stdout = result.normalized_stdout
        if normalized_stdout is None:
            normalized_stdout = normalize_output(result.stdout)
        normalized_stderr = result.normalized_stderr
        if normalized_stderr is None:
            normalized_stderr = normalize_output(result.stderr)
        return {
            "exit_code": result.exit_code,
            "fingerprint": _result_fingerprint(result),
            "infrastructure_error": (
                normalize_output(result.infrastructure_error)
                if result.infrastructure_error is not None
                else None
            ),
            "normalized_stderr": normalized_stderr,
            "normalized_stdout": normalized_stdout,
            "timed_out": result.timed_out,
        }

    def to_data(self) -> Dict[str, Any]:
        return {
            "base_sha": self.base_sha,
            "commands": [
                {
                    "base": self._result_data(comparison.base),
                    "classification": comparison.classification,
                    "command_id": comparison.command_id,
                    "head": self._result_data(comparison.head),
                    "objective": comparison.objective,
                }
                for comparison in self.comparisons
            ],
            "effective_versions": self.effective_versions,
            "exit_code": self.verdict.exit_code,
            "head_sha": self.head_sha,
            "profile_id": self.profile_id,
            "schema_version": "hwpx-loop-verifier-v1",
            "verdict": self.verdict.name,
        }

    def to_json(self) -> bytes:
        return canonical_json(self.to_data())

    def sha256(self) -> str:
        return hashlib.sha256(self.to_json()).hexdigest()


_TIMESTAMP = re.compile(
    r"(^|\n|\b(?:at|timestamp|time)(?:[:=]|\s)+)"
    r"\d{4}-\d{2}-\d{2}[T ][0-2]\d:[0-5]\d:[0-5]\d(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b",
    re.IGNORECASE | re.MULTILINE,
)
_DURATION = re.compile(
    r"(\b(?:finished in|elapsed(?: time)?[:=]?|duration[:=]?|took)\s+)"
    r"\d+(?:\.\d+)?\s*(?:ns|us|µs|ms|milliseconds?|s|sec(?:onds?)?|m|min(?:utes?)?)\b"
    r"(?:\s+\d+(?:\.\d+)?\s*(?:ns|us|µs|ms|milliseconds?|s|sec(?:onds?)?))?",
    re.IGNORECASE,
)
_SHA = re.compile(r"[0-9a-fA-F]{40}")


def normalize_output(
    value: Union[str, bytes], roots: Optional[Mapping[str, str]] = None
) -> str:
    normalized = value.decode("utf-8", "replace") if isinstance(value, bytes) else value
    for root, replacement in sorted(
        (roots or {}).items(), key=lambda item: len(item[0]), reverse=True
    ):
        normalized = normalized.replace(root, replacement)
    normalized = _TIMESTAMP.sub(r"\1<TIMESTAMP>", normalized)
    return _DURATION.sub(r"\1<DURATION>", normalized)


def fingerprint(
    value: Union[str, bytes], roots: Optional[Mapping[str, str]] = None
) -> str:
    return hashlib.sha256(normalize_output(value, roots).encode("utf-8")).hexdigest()


def canonical_json(value: Any) -> bytes:
    if is_dataclass(value):
        value = asdict(value)
    text = json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )
    return (text + "\n").encode("utf-8")


def _result_fingerprint(result: CommandResult) -> str:
    stdout = result.normalized_stdout
    if stdout is None:
        stdout = normalize_output(result.stdout)
    stderr = result.normalized_stderr
    if stderr is None:
        stderr = normalize_output(result.stderr)
    return fingerprint(stdout + "\n" + stderr)


def classify(base: CommandResult, head: CommandResult) -> str:
    if (
        base.timed_out
        or head.timed_out
        or base.infrastructure_error is not None
        or head.infrastructure_error is not None
        or base.exit_code is None
        or head.exit_code is None
    ):
        return "inconclusive"
    if base.exit_code == 0 and head.exit_code == 0:
        return "unchanged_pass"
    if base.exit_code != 0 and head.exit_code == 0:
        return "improved"
    if base.exit_code == 0 and head.exit_code != 0:
        return "new_regression"
    if base.exit_code != head.exit_code:
        return "changed_failure"
    if _result_fingerprint(base) == _result_fingerprint(head):
        return "pre_existing_failure"
    return "changed_failure"


def overall_verdict(values: Any) -> Verdict:
    items = list(values)
    comparisons = [item for item in items if isinstance(item, CommandComparison)]
    outcomes = {
        item.classification if isinstance(item, CommandComparison) else item
        for item in items
    }
    if "invalid_run" in outcomes:
        return Verdict("invalid_run", 40)
    if "inconclusive" in outcomes:
        return Verdict("infrastructure_blocked", 30)
    if outcomes.intersection({"new_regression", "changed_failure"}):
        return Verdict("repair_required", 20)
    objectives = [comparison for comparison in comparisons if comparison.objective]
    if objectives and any(item.classification != "improved" for item in objectives):
        return Verdict("objective_not_met", 21)
    if "pre_existing_failure" in outcomes and "improved" not in outcomes:
        return Verdict("objective_not_met", 21)
    return Verdict("candidate_pass", 0)


def validate_request(request: VerificationRequest, profile: VerificationProfile) -> None:
    if not _SHA.fullmatch(request.base_sha) or not _SHA.fullmatch(request.head_sha):
        raise InvalidRun("base and head must be full 40-character commit SHAs")
    if request.base_sha.lower() == request.head_sha.lower():
        raise InvalidRun("base and head must differ")
    command_ids = [command.command_id for command in profile.commands]
    if len(command_ids) != len(set(command_ids)):
        raise InvalidRun("command IDs must be unique")
    if len(request.objective_command_ids) != len(set(request.objective_command_ids)):
        raise InvalidRun("objective command IDs must be unique")
    if not request.objective_command_ids:
        raise InvalidRun("at least one objective command ID is required")
    if not set(request.objective_command_ids).issubset(command_ids):
        raise InvalidRun("objective command IDs must be registered by the profile")
    for command in profile.commands:
        if type(command.timeout_seconds) is not int or not 1 <= command.timeout_seconds <= 3600:
            raise InvalidRun("command timeouts must be integer seconds from 1 through 3600")
    if profile != baseline_v1():
        raise InvalidRun("only the exact registered baseline-v1 profile is accepted")


Executor = Callable[[Sequence[str], Path, Dict[str, str], float], Execution]


def execute_command(
    argv: Sequence[str], cwd: Path, env: Dict[str, str], timeout: float
) -> Execution:
    started = time.monotonic()
    try:
        process = subprocess.Popen(
            list(argv),
            cwd=str(cwd),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        stdout, stderr = process.communicate(timeout=timeout)
        return Execution(
            process.returncode,
            stdout,
            stderr,
            duration_seconds=time.monotonic() - started,
        )
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        else:
            process.terminate()
        try:
            stdout, stderr = process.communicate(timeout=2)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                process.kill()
            stdout, stderr = process.communicate()
        return Execution(
            None,
            stdout,
            stderr,
            timed_out=True,
            duration_seconds=time.monotonic() - started,
        )
    except OSError as error:
        return Execution(
            None,
            b"",
            b"",
            infrastructure_error=str(error),
            duration_seconds=time.monotonic() - started,
        )


class DifferentialVerifier:
    def __init__(
        self,
        repo_dir: Path,
        artifact_dir: Path,
        executor: Executor = execute_command,
        oci_runtime: str = "docker",
    ) -> None:
        self.repo_dir = Path(repo_dir).resolve()
        self.artifact_dir = Path(artifact_dir).resolve()
        self.executor = executor
        self.oci_runtime = oci_runtime

    @staticmethod
    def _host_env() -> Dict[str, str]:
        names = ("PATH", "HOME", "DOCKER_HOST", "XDG_RUNTIME_DIR")
        environment = {name: os.environ[name] for name in names if name in os.environ}
        environment.update(
            {
                "GIT_ATTR_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_OPTIONAL_LOCKS": "0",
                "GIT_TERMINAL_PROMPT": "0",
                "LC_ALL": "C",
            }
        )
        return environment

    @staticmethod
    def _succeeded(execution: Execution) -> bool:
        return (
            execution.exit_code == 0
            and not execution.timed_out
            and execution.infrastructure_error is None
        )

    @staticmethod
    def _contains(parent: Path, child: Path) -> bool:
        try:
            child.relative_to(parent)
            return True
        except ValueError:
            return False

    def _validate_path_boundaries(self) -> None:
        if self._contains(self.repo_dir, self.artifact_dir):
            raise InvalidRun("artifact directory must not be inside the repository or .git")
        if self._contains(self.artifact_dir, self.repo_dir):
            raise InvalidRun("repository must not be inside the artifact directory")

    def run(
        self, request: VerificationRequest, profile: VerificationProfile
    ) -> VerificationReport:
        validate_request(request, profile)
        self._validate_path_boundaries()
        status = self.executor(
            (
                "git",
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ),
            self.repo_dir,
            self._host_env(),
            60,
        )
        if not self._succeeded(status):
            return self._blocked_report(request, profile, "unable to inspect repository")
        if status.stdout.strip():
            raise InvalidRun("repository must be clean")
        for label, sha in (("base", request.base_sha), ("head", request.head_sha)):
            resolved = self.executor(
                ("git", "rev-parse", "--verify", sha + "^{commit}"),
                self.repo_dir,
                self._host_env(),
                60,
            )
            if (
                not self._succeeded(resolved)
                or normalize_output(resolved.stdout).strip().lower() != sha.lower()
            ):
                raise InvalidRun(label + " SHA must resolve exactly to a commit object")

        self.artifact_dir.mkdir(parents=True, exist_ok=False)
        (self.artifact_dir / "request.json").write_bytes(
            canonical_json(
                {
                    "profile": asdict(profile),
                    "request": asdict(request),
                    "schema_version": "hwpx-loop-request-v1",
                }
            )
        )

        checkouts: Dict[str, Path] = {}
        roots: Dict[str, Dict[str, str]] = {}
        versions: Dict[str, Dict[str, str]] = {}
        for side, sha in (("base", request.base_sha), ("head", request.head_sha)):
            checkout = self.artifact_dir / "checkouts" / side
            checkout.parent.mkdir(parents=True, exist_ok=True)
            clone = self._logged(
                side,
                "clone",
                (
                    "git",
                    "clone",
                    "--no-local",
                    "--no-hardlinks",
                    "--no-checkout",
                    str(self.repo_dir),
                    str(checkout),
                ),
                self.repo_dir,
                300,
            )
            if not self._succeeded(clone):
                return self._write_blocked_report(request, profile, "checkout clone failed")
            checked_out = self._logged(
                side,
                "checkout",
                ("git", "checkout", "--detach", sha),
                checkout,
                300,
            )
            if not self._succeeded(checked_out):
                return self._write_blocked_report(request, profile, "checkout failed")

            target = self.artifact_dir / "targets" / side
            cargo_home = self.artifact_dir / "cargo-home" / side
            rustup_home = self.artifact_dir / "rustup-home" / side
            temporary = self.artifact_dir / "temp" / side
            tools = self.artifact_dir / "tools" / side
            for directory in (target, cargo_home, rustup_home, temporary, tools):
                directory.mkdir(parents=True, exist_ok=True)
            roots[side] = {
                str(checkout): "<CHECKOUT>",
                str(target): "<TARGET>",
                str(cargo_home): "<CARGO_HOME>",
                str(rustup_home): "<RUSTUP_HOME>",
                str(temporary): "<TEMP>",
                str(tools): "<TOOLS>",
            }
            self._assert_checkout(side, checkout, sha, "setup-before")

            setup_commands = (
                (
                    "stable-toolchain-install",
                    (
                        "rustup",
                        "toolchain",
                        "install",
                        profile.stable_toolchain,
                        "--profile",
                        "minimal",
                        "--component",
                        "clippy",
                        "--no-self-update",
                    ),
                ),
                (
                    "nightly-toolchain-install",
                    (
                        "rustup",
                        "toolchain",
                        "install",
                        profile.nightly_toolchain,
                        "--profile",
                        "minimal",
                        "--no-self-update",
                    ),
                ),
                (
                    "cargo-fuzz-install",
                    (
                        "cargo",
                        "+" + profile.nightly_toolchain,
                        "install",
                        "cargo-fuzz",
                        "--version",
                        profile.cargo_fuzz_version,
                        "--locked",
                        "--root",
                        "/tools",
                    ),
                ),
                (
                    "workspace-fetch",
                    ("cargo", "+" + profile.stable_toolchain, "fetch", "--locked"),
                ),
                (
                    "fuzz-fetch",
                    (
                        "cargo",
                        "+" + profile.nightly_toolchain,
                        "fetch",
                        "--manifest-path",
                        "fuzz/Cargo.toml",
                        "--locked",
                    ),
                ),
            )
            for setup_name, setup_command in setup_commands:
                self._assert_checkout(
                    side, checkout, sha, setup_name + "-before"
                )
                setup = self._logged(
                    side,
                    setup_name,
                    self._oci_argv(
                        profile,
                        checkout,
                        target,
                        cargo_home,
                        temporary,
                        tools,
                        profile.setup_network,
                        setup_command,
                    ),
                    self.repo_dir,
                    2700,
                )
                self._assert_checkout(side, checkout, sha, setup_name + "-after")
                if not self._succeeded(setup):
                    return self._write_blocked_report(
                        request, profile, setup_name + " failed"
                    )

            probes = {
                "image": (
                    self.oci_runtime,
                    "image",
                    "inspect",
                    "--format",
                    "{{json .RepoDigests}}",
                    profile.oci_image,
                ),
                "toolchains": self._oci_argv(
                    profile,
                    checkout,
                    target,
                    cargo_home,
                    temporary,
                    tools,
                    profile.command_network,
                    ("rustup", "toolchain", "list"),
                ),
                "nightly_toolchain": self._oci_argv(
                    profile,
                    checkout,
                    target,
                    cargo_home,
                    temporary,
                    tools,
                    profile.command_network,
                    (
                        "rustup",
                        "which",
                        "--toolchain",
                        profile.nightly_toolchain,
                        "rustc",
                    ),
                ),
                "stable_rust": self._oci_argv(
                    profile,
                    checkout,
                    target,
                    cargo_home,
                    temporary,
                    tools,
                    profile.command_network,
                    ("rustc", "+" + profile.stable_toolchain, "--version", "--verbose"),
                ),
                "nightly_rust": self._oci_argv(
                    profile,
                    checkout,
                    target,
                    cargo_home,
                    temporary,
                    tools,
                    profile.command_network,
                    ("rustc", "+" + profile.nightly_toolchain, "--version", "--verbose"),
                ),
                "cargo_fuzz": self._oci_argv(
                    profile,
                    checkout,
                    target,
                    cargo_home,
                    temporary,
                    tools,
                    profile.command_network,
                    ("/tools/bin/cargo-fuzz", "--version"),
                ),
            }
            versions[side] = {}
            for name, argv in probes.items():
                self._assert_checkout(side, checkout, sha, name + "-version-before")
                probe = self._logged(
                    side, name + "-version", argv, self.repo_dir, 300
                )
                self._assert_checkout(side, checkout, sha, name + "-version-after")
                if not self._succeeded(probe):
                    return self._write_blocked_report(
                        request, profile, name + " version probe failed"
                    )
                effective = normalize_output(probe.stdout, roots[side]).strip()
                versions[side][name] = effective
                if not self._effective_version_matches(name, effective, profile):
                    return self._write_blocked_report(
                        request, profile, name + " effective version mismatch"
                    )
            self._assert_checkout(side, checkout, sha, "setup-after")
            checkouts[side] = checkout

        results: Dict[str, Dict[str, CommandResult]] = {"base": {}, "head": {}}
        for side, sha in (("base", request.base_sha), ("head", request.head_sha)):
            checkout = checkouts[side]
            target = self.artifact_dir / "targets" / side
            cargo_home = self.artifact_dir / "cargo-home" / side
            temporary = self.artifact_dir / "temp" / side
            tools = self.artifact_dir / "tools" / side
            for command in profile.commands:
                self._assert_checkout(
                    side, checkout, sha, command.command_id + "-before"
                )
                execution = self._logged(
                    side,
                    command.command_id,
                    self._oci_argv(
                        profile,
                        checkout,
                        target,
                        cargo_home,
                        temporary,
                        tools,
                        profile.command_network,
                        command.argv,
                    ),
                    self.repo_dir,
                    command.timeout_seconds,
                )
                results[side][command.command_id] = CommandResult(
                    command.command_id,
                    execution.exit_code,
                    execution.stdout,
                    execution.stderr,
                    execution.timed_out,
                    execution.infrastructure_error,
                    execution.duration_seconds,
                    normalize_output(execution.stdout, roots[side]),
                    normalize_output(execution.stderr, roots[side]),
                )
                self._assert_checkout(
                    side, checkout, sha, command.command_id + "-after"
                )

        objective_ids = set(request.objective_command_ids)
        comparisons = tuple(
            CommandComparison(
                command.command_id,
                results["base"][command.command_id],
                results["head"][command.command_id],
                classify(
                    results["base"][command.command_id],
                    results["head"][command.command_id],
                ),
                command.command_id in objective_ids,
            )
            for command in profile.commands
        )
        report = VerificationReport(
            profile.profile_id,
            request.base_sha,
            request.head_sha,
            comparisons,
            overall_verdict(comparisons),
            versions,
        )
        self._write_report(report)
        return report

    @staticmethod
    def _effective_version_matches(
        name: str, output: str, profile: VerificationProfile
    ) -> bool:
        if name == "image":
            try:
                return profile.oci_image in json.loads(output)
            except (TypeError, ValueError):
                return False
        if name == "toolchains":
            lines = output.splitlines()
            stable = re.compile(r"^" + re.escape(profile.stable_toolchain) + r"(?:-|\s|$)")
            nightly = re.compile(r"^" + re.escape(profile.nightly_toolchain) + r"(?:-|\s|$)")
            return any(stable.search(line) for line in lines) and any(
                nightly.search(line) for line in lines
            )
        if name == "nightly_toolchain":
            path = output.replace("\\", "/").strip()
            return bool(
                re.search(
                    r"/toolchains/"
                    + re.escape(profile.nightly_toolchain)
                    + r"(?:-[^/\s]+)?/bin/rustc(?:\.exe)?$",
                    path,
                )
            )
        if name == "stable_rust":
            return bool(
                re.search(
                    r"(?:^rustc |^release:\s*)" + re.escape(profile.stable_toolchain) + r"(?:\s|$)",
                    output,
                    re.MULTILINE,
                )
            )
        if name == "nightly_rust":
            fields = {}
            for line in output.splitlines():
                if ":" in line:
                    key, value = line.split(":", 1)
                    fields[key.strip()] = value.strip()
            return (
                fields.get("release") == profile.nightly_release
                and fields.get("commit-hash") == profile.nightly_commit_hash
                and fields.get("commit-date") == profile.nightly_commit_date
            )
        if name == "cargo_fuzz":
            return bool(
                re.search(
                    r"^cargo-fuzz\s+" + re.escape(profile.cargo_fuzz_version) + r"(?:\s|$)",
                    output,
                    re.MULTILINE,
                )
            )
        return False

    def _write_report(self, report: VerificationReport) -> None:
        (self.artifact_dir / "result.json").write_bytes(report.to_json())
        (self.artifact_dir / "result.sha256").write_text(
            report.sha256() + "\n", encoding="ascii"
        )

    def _logged(
        self,
        side: str,
        name: str,
        argv: Sequence[str],
        cwd: Path,
        timeout: float,
    ) -> Execution:
        container_name = None
        cidfile = None
        if tuple(argv[:2]) == (self.oci_runtime, "run"):
            container_name = "hwpx-loop-" + side + "-" + uuid.uuid4().hex
            cid_dir = self.artifact_dir / "temp" / side / "containers"
            cid_dir.mkdir(parents=True, exist_ok=True)
            cidfile = cid_dir / (container_name + ".cid")
            argv = tuple(argv[:2]) + (
                "--name",
                container_name,
                "--cidfile",
                str(cidfile),
            ) + tuple(argv[2:])
        execution = self.executor(argv, cwd, self._host_env(), timeout)
        log_dir = self.artifact_dir / "logs" / side
        log_dir.mkdir(parents=True, exist_ok=True)
        (log_dir / (name + ".stdout.bin")).write_bytes(execution.stdout)
        (log_dir / (name + ".stderr.bin")).write_bytes(execution.stderr)
        if container_name is not None:
            cleanup_failed = self._cleanup_container(
                side, name, container_name, execution, cwd
            )
            if cidfile is not None:
                try:
                    cidfile.unlink()
                except FileNotFoundError:
                    pass
            if cleanup_failed and execution.infrastructure_error is None:
                execution = Execution(
                    execution.exit_code,
                    execution.stdout,
                    execution.stderr,
                    execution.timed_out,
                    "OCI container cleanup failed",
                    execution.duration_seconds,
                )
        return execution

    def _cleanup_container(
        self,
        side: str,
        log_name: str,
        container_name: str,
        execution: Execution,
        cwd: Path,
    ) -> bool:
        cleanup_failed = False
        if execution.timed_out or execution.infrastructure_error is not None:
            stopped = self._cleanup_step(
                side,
                log_name,
                "stop",
                (self.oci_runtime, "stop", "--time", "2", container_name),
                cwd,
            )
            if not self._succeeded(stopped):
                killed = self._cleanup_step(
                    side,
                    log_name,
                    "kill",
                    (self.oci_runtime, "kill", container_name),
                    cwd,
                )
                cleanup_failed = cleanup_failed or not self._succeeded(killed)
        self._cleanup_step(
            side,
            log_name,
            "wait",
            (self.oci_runtime, "wait", container_name),
            cwd,
        )
        self._cleanup_step(
            side,
            log_name,
            "inspect",
            (self.oci_runtime, "inspect", container_name),
            cwd,
        )
        removed = self._cleanup_step(
            side,
            log_name,
            "rm",
            (self.oci_runtime, "rm", "-f", container_name),
            cwd,
        )
        return cleanup_failed or not self._succeeded(removed)

    def _cleanup_step(
        self,
        side: str,
        log_name: str,
        action: str,
        argv: Sequence[str],
        cwd: Path,
    ) -> Execution:
        result = self.executor(argv, cwd, self._host_env(), 60)
        log_dir = self.artifact_dir / "logs" / side
        log_dir.mkdir(parents=True, exist_ok=True)
        prefix = log_name + ".cleanup-" + action
        (log_dir / (prefix + ".stdout.bin")).write_bytes(result.stdout)
        (log_dir / (prefix + ".stderr.bin")).write_bytes(result.stderr)
        return result

    def _assert_checkout(self, side: str, checkout: Path, sha: str, phase: str) -> None:
        head = self._logged(
            side, phase + "-head", ("git", "rev-parse", "HEAD"), checkout, 60
        )
        clean = self._logged(
            side,
            phase + "-clean",
            (
                "git",
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ),
            checkout,
            60,
        )
        detached = self._logged(
            side,
            phase + "-detached",
            ("git", "symbolic-ref", "-q", "HEAD"),
            checkout,
            60,
        )
        if (
            not self._succeeded(head)
            or normalize_output(head.stdout).strip().lower() != sha.lower()
            or not self._succeeded(clean)
            or bool(clean.stdout.strip())
            or detached.exit_code != 1
            or detached.timed_out
            or detached.infrastructure_error is not None
            or bool(detached.stdout)
            or bool(detached.stderr)
        ):
            raise InvalidRun("checkout mutation or identity mismatch detected at " + phase)

    def _oci_argv(
        self,
        profile: VerificationProfile,
        checkout: Path,
        target: Path,
        cargo_home: Path,
        temporary: Path,
        tools: Path,
        network: str,
        command: Sequence[str],
    ) -> Tuple[str, ...]:
        rustup_home = cargo_home.parent.parent / "rustup-home" / cargo_home.name
        return (
            self.oci_runtime,
            "run",
            "--read-only",
            "--network",
            network,
            "--workdir",
            "/workspace",
            "--env",
            "CARGO_HOME=/cargo-home",
            "--env",
            "CARGO_TARGET_DIR=/target",
            "--env",
            "RUSTUP_HOME=/rustup-home",
            "--env",
            "TMPDIR=/tmp/hwpx-loop",
            "--env",
            "HOME=/tmp/hwpx-loop",
            "--env",
            "PATH=/tools/bin:/usr/local/cargo/bin:/usr/local/rustup/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            "--mount",
            "type=bind,src=" + str(checkout) + ",dst=/workspace,readonly",
            "--mount",
            "type=bind,src=" + str(target) + ",dst=/target",
            "--mount",
            "type=bind,src=" + str(cargo_home) + ",dst=/cargo-home",
            "--mount",
            "type=bind,src=" + str(rustup_home) + ",dst=/rustup-home",
            "--mount",
            "type=bind,src=" + str(temporary) + ",dst=/tmp/hwpx-loop",
            "--mount",
            "type=bind,src=" + str(tools) + ",dst=/tools",
            profile.oci_image,
        ) + tuple(command)

    def _blocked_report(
        self, request: VerificationRequest, profile: VerificationProfile, reason: str
    ) -> VerificationReport:
        missing = CommandResult("", None, b"", b"", infrastructure_error=reason)
        objective_ids = set(request.objective_command_ids)
        comparisons = tuple(
            CommandComparison(
                command.command_id,
                missing,
                missing,
                "inconclusive",
                command.command_id in objective_ids,
            )
            for command in profile.commands
        )
        return VerificationReport(
            profile.profile_id,
            request.base_sha,
            request.head_sha,
            comparisons,
            Verdict("infrastructure_blocked", 30),
        )

    def _write_blocked_report(
        self, request: VerificationRequest, profile: VerificationProfile, reason: str
    ) -> VerificationReport:
        report = self._blocked_report(request, profile, reason)
        self._write_report(report)
        return report
