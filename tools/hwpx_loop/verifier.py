import hashlib
import json
import os
import re
import subprocess
import time
from dataclasses import asdict, dataclass, is_dataclass
from pathlib import Path
from typing import Any, Callable, Dict, Optional, Sequence, Tuple, Union

from tools.hwpx_loop.profile import VerificationProfile, baseline_v1


@dataclass(frozen=True)
class CommandResult:
    command_id: str
    exit_code: Optional[int]
    stdout: str
    stderr: str
    timed_out: bool = False
    infrastructure_error: Optional[str] = None
    duration_seconds: float = 0.0


@dataclass(frozen=True)
class Execution:
    exit_code: Optional[int]
    stdout: str
    stderr: str
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


class InvalidRun(ValueError):
    pass


@dataclass(frozen=True)
class CommandComparison:
    command_id: str
    base: CommandResult
    head: CommandResult
    classification: str


@dataclass(frozen=True)
class VerificationReport:
    profile_id: str
    base_sha: str
    head_sha: str
    comparisons: Tuple[CommandComparison, ...]
    verdict: Verdict

    @staticmethod
    def _result_data(result: CommandResult) -> Dict[str, Any]:
        return {
            "exit_code": result.exit_code,
            "fingerprint": _result_fingerprint(result),
            "infrastructure_error": (
                normalize_output(result.infrastructure_error)
                if result.infrastructure_error is not None
                else None
            ),
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
                }
                for comparison in self.comparisons
            ],
            "exit_code": self.verdict.exit_code,
            "head_sha": self.head_sha,
            "profile_id": self.profile_id,
            "schema_version": "hwpx-loop-verifier-v1",
            "verdict": self.verdict.name,
        }

    def to_json(self) -> bytes:
        return canonical_json(self.to_data())


_TIMESTAMP = re.compile(
    r"\b\d{4}-\d{2}-\d{2}[T ][0-2]\d:[0-5]\d:[0-5]\d(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b"
)
_TEMP_PATH = re.compile(
    r"(?:/tmp/|/private/tmp/|/var/folders/)[^\s:]+"
    r"|[A-Za-z]:\\(?:Users\\[^\\\s]+\\AppData\\Local\\Temp|Temp)\\[^\s:]+",
    re.IGNORECASE,
)
_DURATION = re.compile(
    r"\b\d+(?:\.\d+)?\s*(?:ns|us|µs|ms|milliseconds?|s|sec(?:onds?)?|m|min(?:utes?)?)\b",
    re.IGNORECASE,
)
_SHA = re.compile(r"[0-9a-fA-F]{40}")
_PINNED_STABLE = re.compile(r"\d+\.\d+\.\d+")
_PINNED_NIGHTLY = re.compile(r"nightly-\d{4}-\d{2}-\d{2}")
_PINNED_VERSION = re.compile(r"\d+\.\d+\.\d+")


def normalize_output(value: str) -> str:
    normalized = _TIMESTAMP.sub("<TIMESTAMP>", value)
    normalized = _TEMP_PATH.sub("<TMP>", normalized)
    return _DURATION.sub("<DURATION>", normalized)


def fingerprint(value: Union[str, bytes]) -> str:
    if isinstance(value, str):
        payload = normalize_output(value).encode("utf-8")
    else:
        payload = value
    return hashlib.sha256(payload).hexdigest()


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
    return fingerprint(result.stdout + "\n" + result.stderr)


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


def overall_verdict(classifications: Any) -> Verdict:
    outcomes = set(classifications)
    if "inconclusive" in outcomes:
        return Verdict("infrastructure_blocked", 30)
    if "invalid_run" in outcomes:
        return Verdict("invalid_run", 40)
    if outcomes.intersection({"new_regression", "changed_failure"}):
        return Verdict("repair_required", 20)
    if "pre_existing_failure" in outcomes:
        return Verdict("objective_not_met", 21)
    return Verdict("candidate_pass", 0)


def validate_request(
    request: VerificationRequest, profile: VerificationProfile, dirty: bool
) -> None:
    if not _SHA.fullmatch(request.base_sha) or not _SHA.fullmatch(request.head_sha):
        raise InvalidRun("base and head must be full 40-character commit SHAs")
    if request.base_sha.lower() == request.head_sha.lower():
        raise InvalidRun("base and head must differ")
    if dirty:
        raise InvalidRun("repository must be clean")
    if not _PINNED_STABLE.fullmatch(profile.stable_toolchain):
        raise InvalidRun("stable toolchain must be an immutable version")
    if not _PINNED_NIGHTLY.fullmatch(profile.nightly_toolchain):
        raise InvalidRun("nightly toolchain must include a date")
    if not _PINNED_VERSION.fullmatch(profile.cargo_fuzz_version):
        raise InvalidRun("cargo-fuzz must be pinned to an exact version")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", profile.oci_digest):
        raise InvalidRun("OCI image must be pinned by SHA-256 digest")
    if profile.profile_id == "baseline-v1" and profile != baseline_v1():
        raise InvalidRun("baseline-v1 profile must match its exact pinned definition")

    command_ids = [command.command_id for command in profile.commands]
    if len(command_ids) != len(set(command_ids)):
        raise InvalidRun("command IDs must be unique")
    for command in profile.commands:
        if command.timeout_seconds <= 0:
            raise InvalidRun("command timeouts must be positive")
        for argument in command.argv:
            token = argument.lower()
            if "latest" in token:
                raise InvalidRun("command contains a mutable version token")
            if "stable" in token and profile.stable_toolchain not in token:
                raise InvalidRun("command contains a mutable stable toolchain")
            if "nightly" in token and profile.nightly_toolchain not in token:
                raise InvalidRun("command contains a mutable nightly toolchain")


Executor = Callable[[Sequence[str], Path, Dict[str, str], int], Execution]


def execute_command(
    argv: Sequence[str], cwd: Path, env: Dict[str, str], timeout: int
) -> Execution:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            list(argv),
            cwd=str(cwd),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
            check=False,
        )
        return Execution(
            completed.returncode,
            completed.stdout,
            completed.stderr,
            duration_seconds=time.monotonic() - started,
        )
    except subprocess.TimeoutExpired as error:
        stdout = (
            error.stdout.decode("utf-8", "replace")
            if isinstance(error.stdout, bytes)
            else error.stdout
        )
        stderr = (
            error.stderr.decode("utf-8", "replace")
            if isinstance(error.stderr, bytes)
            else error.stderr
        )
        return Execution(
            None,
            stdout or "",
            stderr or "",
            timed_out=True,
            duration_seconds=time.monotonic() - started,
        )
    except OSError as error:
        return Execution(
            None,
            "",
            "",
            infrastructure_error=str(error),
            duration_seconds=time.monotonic() - started,
        )


class DifferentialVerifier:
    def __init__(
        self,
        repo_dir: Path,
        artifact_dir: Path,
        executor: Executor = execute_command,
    ) -> None:
        self.repo_dir = Path(repo_dir).resolve()
        self.artifact_dir = Path(artifact_dir).resolve()
        self.executor = executor

    def run(
        self,
        request: VerificationRequest,
        profile: VerificationProfile,
        dirty: Optional[bool] = None,
    ) -> VerificationReport:
        # Validate all static inputs before invoking even repository inspection.
        validate_request(request, profile, False)
        if dirty is None:
            status = self.executor(
                ("git", "status", "--porcelain"), self.repo_dir, dict(os.environ), 60
            )
            if status.infrastructure_error is not None or status.exit_code != 0:
                return self._blocked_report(request, profile, "unable to inspect repository")
            dirty = bool(status.stdout.strip())
        validate_request(request, profile, dirty)

        self.artifact_dir.mkdir(parents=True, exist_ok=False)
        request_data = {
            "profile": asdict(profile),
            "request": asdict(request),
            "schema_version": "hwpx-loop-request-v1",
        }
        (self.artifact_dir / "request.json").write_bytes(canonical_json(request_data))

        checkouts: Dict[str, Path] = {}
        environments: Dict[str, Dict[str, str]] = {}
        for side, sha in (("base", request.base_sha), ("head", request.head_sha)):
            checkout = self.artifact_dir / "checkouts" / side
            checkout.parent.mkdir(parents=True, exist_ok=True)
            clone = self.executor(
                (
                    "git",
                    "clone",
                    "--no-checkout",
                    "--shared",
                    str(self.repo_dir),
                    str(checkout),
                ),
                self.repo_dir,
                dict(os.environ),
                300,
            )
            if clone.exit_code != 0 or clone.timed_out or clone.infrastructure_error is not None:
                return self._write_blocked_report(request, profile, "checkout clone failed")
            checkout.mkdir(parents=True, exist_ok=True)
            checked_out = self.executor(
                ("git", "checkout", "--detach", sha), checkout, dict(os.environ), 300
            )
            if (
                checked_out.exit_code != 0
                or checked_out.timed_out
                or checked_out.infrastructure_error is not None
            ):
                return self._write_blocked_report(request, profile, "checkout failed")

            target = self.artifact_dir / "targets" / side
            tool_root = self.artifact_dir / "tools" / side
            target.mkdir(parents=True, exist_ok=True)
            tool_root.mkdir(parents=True, exist_ok=True)
            environment = dict(os.environ)
            environment["CARGO_TARGET_DIR"] = str(target)
            environment["PATH"] = (
                str(tool_root / "bin") + os.pathsep + environment.get("PATH", "")
            )
            installed = self.executor(
                (
                    "cargo",
                    "+" + profile.nightly_toolchain,
                    "install",
                    "cargo-fuzz",
                    "--version",
                    profile.cargo_fuzz_version,
                    "--locked",
                    "--root",
                    str(tool_root),
                ),
                checkout,
                environment,
                2700,
            )
            if (
                installed.exit_code != 0
                or installed.timed_out
                or installed.infrastructure_error is not None
            ):
                return self._write_blocked_report(request, profile, "cargo-fuzz install failed")
            checkouts[side] = checkout
            environments[side] = environment

        results: Dict[str, Dict[str, CommandResult]] = {"base": {}, "head": {}}
        for side in ("base", "head"):
            log_dir = self.artifact_dir / "logs" / side
            log_dir.mkdir(parents=True, exist_ok=True)
            for command in profile.commands:
                execution = self.executor(
                    command.argv,
                    checkouts[side],
                    environments[side],
                    command.timeout_seconds,
                )
                result = CommandResult(
                    command.command_id,
                    execution.exit_code,
                    execution.stdout,
                    execution.stderr,
                    execution.timed_out,
                    execution.infrastructure_error,
                    execution.duration_seconds,
                )
                results[side][command.command_id] = result
                (log_dir / (command.command_id + ".stdout.log")).write_text(
                    execution.stdout, encoding="utf-8"
                )
                (log_dir / (command.command_id + ".stderr.log")).write_text(
                    execution.stderr, encoding="utf-8"
                )

        comparisons = tuple(
            CommandComparison(
                command.command_id,
                results["base"][command.command_id],
                results["head"][command.command_id],
                classify(
                    results["base"][command.command_id],
                    results["head"][command.command_id],
                ),
            )
            for command in profile.commands
        )
        report = VerificationReport(
            profile.profile_id,
            request.base_sha,
            request.head_sha,
            comparisons,
            overall_verdict(comparison.classification for comparison in comparisons),
        )
        (self.artifact_dir / "result.json").write_bytes(report.to_json())
        return report

    def _blocked_report(
        self, request: VerificationRequest, profile: VerificationProfile, reason: str
    ) -> VerificationReport:
        missing = CommandResult("", None, "", "", infrastructure_error=reason)
        comparisons = tuple(
            CommandComparison(command.command_id, missing, missing, "inconclusive")
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
        (self.artifact_dir / "result.json").write_bytes(report.to_json())
        return report
