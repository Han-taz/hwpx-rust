import argparse
import sys
from pathlib import Path
from typing import Any, Optional, Sequence, TextIO

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.hwpx_loop.profile import baseline_v1
from tools.hwpx_loop.verifier import (
    DifferentialVerifier,
    InvalidRun,
    VerificationRequest,
    canonical_json,
    normalize_output,
    validate_request,
)


class _Parser(argparse.ArgumentParser):
    def error(self, message: str) -> None:
        raise InvalidRun(message)


def _parser() -> argparse.ArgumentParser:
    parser = _Parser(prog="hwpx-loop", add_help=False)
    commands = parser.add_subparsers(dest="action", required=True)
    verify = commands.add_parser("verify", add_help=False)
    verify.add_argument("--base", required=True)
    verify.add_argument("--head", required=True)
    verify.add_argument("--artifacts", required=True)
    verify.add_argument("--repo", default=".")
    verify.add_argument("--objective", action="append", required=True)
    verify.add_argument("--oci-runtime", default="docker")
    return parser


def _error_payload(verdict: str, exit_code: int, error: str) -> bytes:
    return canonical_json(
        {
            "error": normalize_output(error),
            "exit_code": exit_code,
            "schema_version": "hwpx-loop-verifier-v1",
            "verdict": verdict,
        }
    )


def main(
    argv: Optional[Sequence[str]] = None,
    stdout: Optional[TextIO] = None,
    verifier_factory: Any = DifferentialVerifier,
) -> int:
    output = stdout if stdout is not None else sys.stdout
    try:
        arguments_list = list(argv) if argv is not None else sys.argv[1:]
        if "--help" in arguments_list or "-h" in arguments_list:
            output.write(
                canonical_json(
                    {
                        "action": "help",
                        "exit_code": 0,
                        "schema_version": "hwpx-loop-cli-v1",
                        "usage": "hwpx-loop verify --base SHA --head SHA --artifacts PATH --objective ID [--objective ID ...] [--repo PATH] [--oci-runtime PATH]",
                    }
                ).decode("utf-8")
            )
            return 0
        arguments = _parser().parse_args(arguments_list)
        request = VerificationRequest(
            arguments.base, arguments.head, tuple(arguments.objective)
        )
        profile = baseline_v1()
        validate_request(request, profile)
        verifier = verifier_factory(
            repo_dir=Path(arguments.repo),
            artifact_dir=Path(arguments.artifacts),
            oci_runtime=arguments.oci_runtime,
        )
        report = verifier.run(request, profile)
        output.write(report.to_json().decode("utf-8"))
        return report.verdict.exit_code
    except InvalidRun as error:
        output.write(_error_payload("invalid_run", 40, str(error)).decode("utf-8"))
        return 40
    except Exception as error:
        output.write(
            _error_payload("infrastructure_blocked", 30, str(error)).decode("utf-8")
        )
        return 30


if __name__ == "__main__":
    raise SystemExit(main())
