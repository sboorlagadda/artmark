import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from version_policy import bump_type, check_pr  # noqa: E402


class VersionPolicyTests(unittest.TestCase):
    def test_one_step_bumps(self):
        self.assertEqual(bump_type("0.0.1", "0.0.2"), "patch")
        self.assertEqual(bump_type("0.0.1", "0.1.0"), "minor")
        self.assertEqual(bump_type("0.0.1", "1.0.0"), "major")

    def test_skips_and_unchanged_versions_fail(self):
        for head in ("0.0.1", "0.0.3", "0.1.1", "1.0.1", "0.0.0"):
            with self.subTest(head=head), self.assertRaises(ValueError):
                bump_type("0.0.1", head)

    def test_pr_requires_synced_lockfile(self):
        base = b'[package]\nname = "artmark"\nversion = "0.0.1"\n'
        head = b'[package]\nname = "artmark"\nversion = "0.0.2"\n'
        stale_lock = b'[[package]]\nname = "artmark"\nversion = "0.0.1"\n'
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            check_pr(base, head, stale_lock)


if __name__ == "__main__":
    unittest.main()
