from dataclasses import dataclass
from typing import Tuple


@dataclass(frozen=True)
class CommandSpec:
    command_id: str
    argv: Tuple[str, ...]
    timeout_seconds: int


@dataclass(frozen=True)
class VerificationProfile:
    profile_id: str
    stable_toolchain: str
    nightly_toolchain: str
    nightly_release: str
    nightly_commit_hash: str
    nightly_commit_date: str
    cargo_fuzz_version: str
    oci_digest: str
    oci_image: str
    setup_network: str
    command_network: str
    commands: Tuple[CommandSpec, ...]


def baseline_v1() -> VerificationProfile:
    stable = "1.97.0"
    nightly = "nightly-2025-06-01"
    digest = "sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073"
    return VerificationProfile(
        profile_id="baseline-v1",
        stable_toolchain=stable,
        nightly_toolchain=nightly,
        nightly_release="1.89.0-nightly",
        nightly_commit_hash="4d08223c054cf5a56d9761ca925fd46ffebe7115",
        nightly_commit_date="2025-05-31",
        cargo_fuzz_version="0.13.1",
        oci_digest=digest,
        oci_image="rust@" + digest,
        setup_network="bridge",
        command_network="none",
        commands=(
            CommandSpec(
                "stable-clippy",
                (
                    "cargo",
                    "+" + stable,
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
            CommandSpec(
                "fuzz-workspace-check",
                (
                    "cargo",
                    "+" + nightly,
                    "check",
                    "--manifest-path",
                    "fuzz/Cargo.toml",
                    "--locked",
                ),
                2700,
            ),
            CommandSpec("fuzz-list", ("cargo", "+" + nightly, "fuzz", "list"), 600),
            CommandSpec(
                "fuzz-build-parse-auto",
                ("cargo", "+" + nightly, "fuzz", "build", "parse_auto"),
                2700,
            ),
            CommandSpec(
                "fuzz-build-parse-hwpx",
                ("cargo", "+" + nightly, "fuzz", "build", "parse_hwpx"),
                2700,
            ),
        ),
    )
