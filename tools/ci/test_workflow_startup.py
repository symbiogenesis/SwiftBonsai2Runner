"""Focused fixtures for action-lock startup failures; no network or models."""

from pathlib import Path
import tempfile
import unittest

from check_workflow_startup import check


CHECKOUT = "actions/checkout@" + "1" * 40
CACHE = "actions/cache@" + "2" * 40


class WorkflowStartupTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="bonsai-workflow-startup-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.write(".github/workflows/main.yml", f"jobs:\n  check:\n    steps:\n      - uses: {CHECKOUT}\n      - uses: $/.github/actions/build\n")
        self.write(".github/actions/build/action.yml", "runs:\n  using: composite\n  steps:\n    - uses: $/.github/actions/cache\n")
        self.write(".github/actions/cache/action.yaml", f"runs:\n  using: composite\n  steps:\n    - uses: {CACHE}\n")
        self.write_lock()

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def write_lock(self, checkout_key=CHECKOUT, include_cache=True):
        keys = [checkout_key] + ([CACHE] if include_cache else [])
        lines = ["version: 'v0.0.3'", "workflows:", "    '.github/workflows/main.yml':"]
        lines += [f"        - '{key}'" for key in keys]
        lines.append("dependencies:")
        for key in keys:
            sha = "1" * 40 if key == checkout_key else "2" * 40
            lines += [f"    '{key}':", "        ref: 'v1.0.0'", f"        commit: 'sha1-{sha}'", "        owner_id: 1", "        repo_id: 2"]
        self.write(".github/workflows/actions.lock", "\n".join(lines) + "\n")

    def assert_failure(self, phrase):
        errors = check(self.root)
        self.assertTrue(any(phrase in error for error in errors), errors)

    def test_exact_sha_keys_and_nested_composite_pass(self):
        self.assertEqual(check(self.root), [])

    def test_tag_key_with_identical_commit_does_not_cover_sha_uses(self):
        self.write_lock(checkout_key="actions/checkout@v1.0.0")
        self.assert_failure("lockfile missing exact uses key " + CHECKOUT)

    def test_nested_composite_dependency_requires_workflow_coverage(self):
        self.write_lock(include_cache=False)
        self.assert_failure("lockfile missing exact uses key " + CACHE)

    def test_mutable_inline_reference_fails_even_with_lock_metadata(self):
        self.write(".github/actions/cache/action.yaml", "runs:\n  using: composite\n  steps:\n    - uses: actions/cache@v1\n")
        self.assert_failure("full 40-character commit SHA")

    def test_lock_commit_must_agree_with_literal_sha(self):
        path = self.root / ".github/workflows/actions.lock"
        path.write_text(path.read_text().replace("sha1-" + "1" * 40, "sha1-" + "9" * 40), encoding="utf-8")
        self.assert_failure("commit metadata disagrees")

    def test_missing_and_stale_workflows_fail(self):
        self.write(".github/workflows/additional.yaml", f"jobs:\n  check:\n    steps:\n      - uses: {CHECKOUT}\n")
        self.assert_failure("missing workflow .github/workflows/additional.yaml")
        (self.root / ".github/workflows/main.yml").unlink()
        self.assert_failure("stale workflow .github/workflows/main.yml")

    def test_missing_local_action_and_cycle_fail(self):
        path = self.root / ".github/actions/cache/action.yaml"
        path.unlink()
        self.assert_failure("same-repository reference must identify exactly one")
        self.write(".github/actions/cache/action.yaml", "runs:\n  using: composite\n  steps:\n    - uses: $/.github/actions/build\n")
        self.assert_failure("dependency cycle")

    def test_block_script_contents_are_not_action_references(self):
        with (self.root / ".github/workflows/main.yml").open("a", encoding="utf-8") as output:
            output.write("      - run: |\n          uses: actions/not-an-action@main\n      - run: >-\n          uses: actions/also-not-an-action@main\n")
        self.assertEqual(check(self.root), [])

    def test_quoted_uses_and_comments_are_checked(self):
        self.write(".github/actions/cache/action.yaml", f"runs:\n  using: composite\n  steps:\n    - 'uses': '{CACHE}' # pinned\n")
        self.assertEqual(check(self.root), [])
        self.write(".github/actions/cache/action.yaml", "runs:\n  using: composite\n  steps:\n    - \"uses\": \"actions/cache@main\"\n")
        self.assert_failure("full 40-character commit SHA")

    def test_flow_style_and_multiline_uses_fail_closed(self):
        self.write(".github/actions/cache/action.yaml", f"runs:\n  using: composite\n  steps:\n    - {{uses: {CACHE}}}\n")
        self.assert_failure("write uses as a block mapping")
        self.write(".github/actions/cache/action.yaml", "runs:\n  using: composite\n  steps:\n    - uses: >-\n        actions/cache@main\n")
        self.assert_failure("unsupported uses syntax")

    def test_malformed_or_duplicate_lock_metadata_fail_closed(self):
        path = self.root / ".github/workflows/actions.lock"
        original = path.read_text(encoding="utf-8")
        path.write_text(original.replace("version: 'v0.0.3'", "version: 'future'"), encoding="utf-8")
        self.assert_failure("expected complete generated lockfile schema")
        path.write_text(original.replace("        repo_id: 2", "        repo_id: 2\n        repo_id: 3", 1), encoding="utf-8")
        self.assert_failure("duplicate dependency field repo_id")

    def test_missing_lockfile_fails(self):
        (self.root / ".github/workflows/actions.lock").unlink()
        self.assert_failure("actions.lock")


if __name__ == "__main__":
    unittest.main()
