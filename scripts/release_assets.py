#!/usr/bin/env python3
"""Package and verify the four native artmark release archives."""

from __future__ import annotations

import argparse
import hashlib
import sys
import tarfile
import zipfile
from pathlib import Path

from version_policy import package_version, validate_lockfile


TARGETS = {
    "x86_64-unknown-linux-gnu": ("artmark", ".tar.gz"),
    "x86_64-apple-darwin": ("artmark", ".tar.gz"),
    "aarch64-apple-darwin": ("artmark", ".tar.gz"),
    "x86_64-pc-windows-msvc": ("artmark.exe", ".zip"),
}
EXTRAS = (
    Path("README.md"),
    Path("SECURITY.md"),
    Path("CHANGELOG.md"),
    Path("assets/logo.png"),
    Path("assets/logo-symbol.svg"),
    Path("assets/logo-wordmark.svg"),
    Path("assets/logo-monochrome.svg"),
    Path("assets/hero-image.png"),
    Path("assets/social-image.png"),
    Path("assets/social-preview.png"),
    Path("LICENSE"),
    Path("CONTRIBUTING.md"),
    Path("AGENTS.md"),
    Path("docs/design.md"),
    Path("docs/brand-kit.md"),
    Path("skills/artmark/SKILL.md"),
)


def archive_name(version: str, target: str) -> str:
    if target not in TARGETS:
        raise ValueError(f"unsupported release target: {target}")
    return f"artmark-v{version}-{target}{TARGETS[target][1]}"


def package(binary: Path, target: str, version: str, dist: Path) -> Path:
    expected_binary, extension = TARGETS[target]
    if not binary.is_file() or binary.name != expected_binary:
        raise ValueError(f"expected built binary named {expected_binary}: {binary}")
    missing = [str(path) for path in EXTRAS if not path.is_file()]
    if missing:
        raise ValueError(f"release files missing: {', '.join(missing)}")
    dist.mkdir(parents=True, exist_ok=True)
    archive = dist / archive_name(version, target)
    entries = ((binary, expected_binary), *((path, path.as_posix()) for path in EXTRAS))
    if extension == ".zip":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for source, name in entries:
                output.write(source, arcname=name)
    else:
        with tarfile.open(archive, "w:gz") as output:
            for source, name in entries:
                output.add(source, arcname=name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (dist / f"{archive.name}.sha256").write_text(f"{digest}  {archive.name}\n")
    return archive


def verify(dist: Path, version: str) -> None:
    expected_files: set[str] = set()
    for target, (binary, extension) in TARGETS.items():
        archive = dist / archive_name(version, target)
        checksum = dist / f"{archive.name}.sha256"
        expected_files.update((archive.name, checksum.name))
        if not archive.is_file() or not checksum.is_file():
            raise ValueError(f"missing archive or checksum for {target}")
        expected = hashlib.sha256(archive.read_bytes()).hexdigest()
        if checksum.read_text() != f"{expected}  {archive.name}\n":
            raise ValueError(f"checksum mismatch for {archive.name}")
        if extension == ".zip":
            with zipfile.ZipFile(archive) as packed:
                names = set(packed.namelist())
        else:
            with tarfile.open(archive, "r:gz") as packed:
                names = set(packed.getnames())
        required = {binary, *(path.as_posix() for path in EXTRAS)}
        if not required.issubset(names):
            raise ValueError(f"archive contents incomplete: {archive.name}")
    actual_files = {path.name for path in dist.iterdir() if path.is_file()}
    if actual_files != expected_files:
        raise ValueError(f"unexpected release files: {sorted(actual_files ^ expected_files)}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    pack = subcommands.add_parser("pack")
    pack.add_argument("--binary", type=Path, required=True)
    pack.add_argument("--target", choices=TARGETS, required=True)
    pack.add_argument("--dist", type=Path, default=Path("dist"))
    check = subcommands.add_parser("verify")
    check.add_argument("--dist", type=Path, default=Path("dist"))
    args = parser.parse_args()

    version = package_version(Path("Cargo.toml").read_bytes())
    validate_lockfile(Path("Cargo.lock").read_bytes(), version)
    if args.command == "pack":
        print(package(args.binary, args.target, version, args.dist))
    else:
        verify(args.dist, version)
        print(f"Verified {len(TARGETS)} artmark {version} release archives")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"release assets: {error}", file=sys.stderr)
        sys.exit(1)
