import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from version_policy import bump_type, check_pr, parse_pr_labels  # noqa: E402


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
            check_pr(base, head, stale_lock, ["semver:patch"])

    def test_pr_label_must_match_bump_and_be_unique(self):
        base = b'[package]\nname = "artmark"\nversion = "0.0.1"\n'
        head = b'[package]\nname = "artmark"\nversion = "0.0.2"\n'
        lock = b'[[package]]\nname = "artmark"\nversion = "0.0.2"\n'
        self.assertEqual(check_pr(base, head, lock, ["docs", "semver:patch"]), "patch")
        for labels in ([], ["semver:minor"], ["semver:patch", "semver:major"]):
            with self.subTest(labels=labels), self.assertRaisesRegex(ValueError, "exactly one"):
                check_pr(base, head, lock, labels)

    def test_parse_pr_labels(self):
        self.assertEqual(
            parse_pr_labels('[{"name":"docs"},{"name":"semver:patch"}]'),
            ["docs", "semver:patch"],
        )
        for raw in ("", "{}", '[{"name":3}]'):
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "PR_LABELS_JSON"):
                parse_pr_labels(raw)


if __name__ == "__main__":
    unittest.main()
