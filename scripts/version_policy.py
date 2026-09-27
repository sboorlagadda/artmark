#!/usr/bin/env python3
"""Validate Artmark's one-version-bump-per-PR release policy."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path


VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
RELEASE_LABELS = {"semver:patch", "semver:minor", "semver:major"}


def parse_version(raw: str) -> tuple[int, int, int]:
    match = VERSION.fullmatch(raw)
    if match is None:
        raise ValueError(f"expected a plain MAJOR.MINOR.PATCH version, got {raw!r}")
    return tuple(int(part) for part in match.groups())


def package_version(manifest: bytes) -> str:
    data = tomllib.loads(manifest.decode("utf-8"))
    if data.get("package", {}).get("name") != "artmark":
        raise ValueError("Cargo.toml must describe the artmark package")
    version = data["package"]["version"]
    parse_version(version)
    return version


def validate_lockfile(lockfile: bytes, expected: str) -> None:
    data = tomllib.loads(lockfile.decode("utf-8"))
    matches = [p for p in data.get("package", []) if p.get("name") == "artmark"]
    if len(matches) != 1 or matches[0].get("version") != expected:
        raise ValueError(f"Cargo.lock must contain artmark {expected}; run cargo check")


def bump_type(base: str, head: str) -> str:
    major, minor, patch = parse_version(base)
    actual = parse_version(head)
    next_versions = {
        "patch": (major, minor, patch + 1),
        "minor": (major, minor + 1, 0),
        "major": (major + 1, 0, 0),
    }
    for name, expected in next_versions.items():
        if actual == expected:
            return name
    allowed = ", ".join(
        f"{name}={'.'.join(map(str, version))}" for name, version in next_versions.items()
    )
    raise ValueError(f"PR version {head} must be exactly one bump from {base}: {allowed}")


def parse_pr_labels(raw: str) -> list[str]:
    try:
        labels = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ValueError("PR_LABELS_JSON must contain GitHub's PR label array") from error
    if not isinstance(labels, list) or any(
        not isinstance(label, dict) or not isinstance(label.get("name"), str)
        for label in labels
    ):
        raise ValueError("PR_LABELS_JSON must contain GitHub's PR label array")
    return [label["name"] for label in labels]


def check_bump_label(labels: list[str], change: str) -> None:
    selected = [label for label in labels if label in RELEASE_LABELS]
    expected = f"semver:{change}"
    if selected != [expected]:
        raise ValueError(
            f"PR must have exactly one release label matching its {change} bump: "
            f"{expected}; found {selected or 'none'}"
        )


def check_pr(
    base_manifest: bytes, head_manifest: bytes, head_lockfile: bytes, labels: list[str]
) -> str:
    base = package_version(base_manifest)
    head = package_version(head_manifest)
    validate_lockfile(head_lockfile, head)
    change = bump_type(base, head)
    check_bump_label(labels, change)
    return change


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("current", help="print the current CLI version")
    check = subcommands.add_parser("check-pr", help="validate the PR version against its base")
    check.add_argument("--base-sha", required=True)
    args = parser.parse_args()

    head_manifest = Path("Cargo.toml").read_bytes()
    head_version = package_version(head_manifest)
    validate_lockfile(Path("Cargo.lock").read_bytes(), head_version)
    if args.command == "current":
        print(head_version)
        return 0

    if not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", args.base_sha):
        raise ValueError("base SHA must be a Git commit hash")
    base_manifest = subprocess.check_output(
        ["git", "show", f"{args.base_sha}:Cargo.toml"]
    )
    labels = parse_pr_labels(os.environ.get("PR_LABELS_JSON", ""))
    change = check_pr(base_manifest, head_manifest, Path("Cargo.lock").read_bytes(), labels)
    print(
        f"Valid {change} bump with semver:{change} label: "
        f"{package_version(base_manifest)} -> {head_version}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"version policy: {error}", file=sys.stderr)
        sys.exit(1)
