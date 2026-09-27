import sys
import tempfile
import unittest
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
                package(binary, target, "0.0.1", dist)
            verify(dist, "0.0.1")
            checksum = next(dist.glob("*.sha256"))
            checksum.write_text("incorrect\n")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                verify(dist, "0.0.1")


if __name__ == "__main__":
    unittest.main()
