import hashlib
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from release_assets import TARGETS, package, verify  # noqa: E402


class ReleaseAssetTests(unittest.TestCase):
    def test_packages_and_verifies_all_platforms(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "dist"
            for target, (name, _) in TARGETS.items():
                binary = root / name
                binary.write_bytes(b"test binary")
                archive = package(binary, target, "0.0.1", dist)
                if archive.suffix == ".zip":
                    with zipfile.ZipFile(archive) as packed:
                        names = set(packed.namelist())
                else:
                    with tarfile.open(archive, "r:gz") as packed:
                        names = set(packed.getnames())
                self.assertEqual(names, {name, "LICENSE", "skills/artmark/SKILL.md"})
            verify(dist, "0.0.1")
            checksum = next(dist.glob("*.sha256"))
            checksum.write_text("incorrect\n")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                verify(dist, "0.0.1")

            checksum_archive = dist / checksum.name.removesuffix(".sha256")
            checksum.write_text(
                f"{hashlib.sha256(checksum_archive.read_bytes()).hexdigest()}  {checksum_archive.name}\n"
            )
            windows = next(dist.glob("*.zip"))
            with zipfile.ZipFile(windows, "a") as packed:
                packed.writestr("README.md", "unexpected")
            (dist / f"{windows.name}.sha256").write_text(
                f"{hashlib.sha256(windows.read_bytes()).hexdigest()}  {windows.name}\n"
            )
            with self.assertRaisesRegex(ValueError, "unexpected archive contents"):
                verify(dist, "0.0.1")


if __name__ == "__main__":
    unittest.main()
