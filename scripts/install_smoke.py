#!/usr/bin/env python3
"""Exercise a release archive as a new user on the current native platform."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

from release_assets import TARGETS, archive_name
from version_policy import package_version


def run(binary: str, args: list[str], env: dict[str, str], cwd: Path, input: str | None = None) -> str:
    result = subprocess.run(
        [binary, *args], input=input, text=True, capture_output=True, env=env, cwd=cwd, check=True
    )
    return result.stdout.strip()


def smoke(archive: Path, target: str) -> None:
    binary_name, extension = TARGETS[target]
    if not archive.name.endswith(extension):
        raise ValueError(f"wrong archive type for {target}: {archive}")
    checksum = Path(f"{archive}.sha256")
    expected, named_file = checksum.read_text().strip().split()
    if named_file != archive.name or hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
        raise ValueError(f"checksum mismatch: {archive}")

    with tempfile.TemporaryDirectory(prefix="artmark-install-") as directory:
        home = Path(directory)
        bin_dir = home / ".local" / "bin"
        bin_dir.mkdir(parents=True)
        installed = bin_dir / binary_name
        if extension == ".zip":
            with zipfile.ZipFile(archive) as package:
                installed.write_bytes(package.read(binary_name))
        else:
            with tarfile.open(archive, "r:gz") as package:
                source = package.extractfile(binary_name)
                if source is None:
                    raise ValueError(f"missing binary: {binary_name}")
                installed.write_bytes(source.read())
            installed.chmod(0o755)

        env = os.environ.copy()
        env.pop("ARTMARK_DB", None)
        if sys.platform == "win32":
            env.pop("HOME", None)
            env["USERPROFILE"] = str(home)
        else:
            env["HOME"] = str(home)
        env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
        if Path(shutil.which(binary_name, path=env["PATH"]) or "") != installed:
            raise ValueError(f"installed binary is not on PATH: {installed}")

        binary = str(installed)
        version = run(binary, ["--version"], env, home)
        added = json.loads(run(binary, ["add", "https://example.com/fresh-install", "--json"], env, home))
        card = json.dumps({"catalog": {"summary": "First install test", "search_text": "fresh install smoke test"}})
        run(binary, ["index", added["id"], "--json-input", "-", "--json"], env, home, card)
        found = json.loads(run(binary, ["search", "fresh", "--json"], env, home))
        if len(found["results"]) != 1 or found["results"][0]["id"] != added["id"]:
            raise ValueError("new install could not search its first catalog entry")
        if not (home / ".artmark" / "artmark.db").is_file():
            raise ValueError("new install did not create the database in the user's home directory")
        print(f"{version}: checksum, install, default database, index, and search passed on {target}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    version = package_version(Path("Cargo.toml").read_bytes())
    smoke(args.archive or Path("dist") / archive_name(version, args.target), args.target)


if __name__ == "__main__":
    main()
