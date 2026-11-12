"""Focused real-index fixtures for the offline workflow pre-commit guard."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from check_staged_workflows import check_staged


CHECKOUT = "actions/checkout@" + "1" * 40
CACHE = "actions/cache@" + "2" * 40
WORKFLOW = ".github/workflows/main.yml"
ACTION = ".github/actions/build/action.yml"
LOCK = ".github/workflows/actions.lock"


class StagedWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="bonsai staged workflows ")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.git_executable = shutil.which("git")
        self.assertIsNotNone(self.git_executable, "Git is required for staged-index regression fixtures")
        self.git("init", "--quiet")
        self.write(WORKFLOW, f"jobs:\n  check:\n    steps:\n      - uses: {CHECKOUT}\n      - uses: $/.github/actions/build\n")
        self.write(ACTION, f"runs:\n  using: composite\n  steps:\n    - uses: {CACHE}\n")
        self.write_lock()
        self.git("add", "--", ".github")
        self.git(
            "-c", "user.name=Workflow Fixture", "-c", "user.email=fixture@example.invalid",
            "-c", "core.hooksPath=" + str(self.root / "no-hooks"),
            "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "fixture baseline",
        )

    def git(self, *arguments, data=None):
        return subprocess.run(
            [self.git_executable, *arguments], cwd=self.root, input=data,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=10,
        ).stdout

    def write(self, path, content):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def write_lock(self, workflow=WORKFLOW, checkout_key=CHECKOUT):
        lines = ["version: 'v0.0.3'", "workflows:", f"    '{workflow}':"]
        lines += [f"        - '{key}'" for key in (checkout_key, CACHE)]
        lines.append("dependencies:")
        for key, sha in ((checkout_key, "1" * 40), (CACHE, "2" * 40)):
            lines += [f"    '{key}':", "        ref: 'v1.0.0'", f"        commit: 'sha1-{sha}'", "        owner_id: 1", "        repo_id: 2"]
        self.write(LOCK, "\n".join(lines) + "\n")

    def stage_change(self, path=WORKFLOW):
        with (self.root / path).open("a", encoding="utf-8") as output:
            output.write("# staged edit\n")
        self.git("add", "--", path)

    def check(self):
        return check_staged(self.root, self.git_executable)

    def assert_failure(self, phrase):
        selected, errors = self.check()
        self.assertTrue(selected)
        self.assertTrue(any(phrase in error for error in errors), errors)

    def test_unrelated_changes_skip_even_with_unstaged_broken_workflow(self):
        self.write("readme.txt", "ordinary edit\n")
        self.git("add", "--", "readme.txt")
        self.write(WORKFLOW, "uses: actions/checkout@main\n")
        self.assertEqual(self.check(), (False, []))

    def test_valid_staged_snapshot_ignores_unstaged_bad_lock(self):
        self.stage_change()
        self.write(LOCK, "invalid working-tree lock\n")
        self.assertEqual(self.check(), (True, []))

    def test_unstaged_repair_cannot_hide_bad_staged_lock(self):
        self.write_lock(checkout_key="actions/checkout@v1.0.0")
        self.git("add", "--", LOCK)
        self.write_lock()
        self.assert_failure("lockfile missing exact uses key " + CHECKOUT)

    def test_staged_composite_change_requires_matching_closure(self):
        self.write(ACTION, "runs:\n  using: composite\n  steps:\n    - uses: actions/upload-artifact@" + "3" * 40 + "\n")
        self.git("add", "--", ACTION)
        self.assert_failure("lockfile missing exact uses key actions/upload-artifact@")

    def test_deleted_lock_fails_even_if_worktree_restored(self):
        self.git("rm", "--cached", "--", LOCK)
        self.assert_failure("actions.lock")

    def test_deleted_workflow_requires_pruning_lock(self):
        self.git("rm", "--", WORKFLOW)
        self.assert_failure("no workflows found")

    def test_renamed_workflow_requires_matching_lock_names(self):
        renamed = ".github/workflows/workflow with spaces.yml"
        self.git("mv", "--", WORKFLOW, renamed)
        self.assert_failure("missing workflow " + renamed)
        self.write_lock(workflow=renamed)
        self.git("add", "--", LOCK)
        self.assertEqual(self.check(), (True, []))

    def test_local_composite_outside_github_is_checked(self):
        outside = "ci/build/action.yaml"
        content = (self.root / ACTION).read_text(encoding="utf-8")
        self.write(outside, content)
        self.git("rm", "--", ACTION)
        self.write(WORKFLOW, f"jobs:\n  check:\n    steps:\n      - uses: {CHECKOUT}\n      - uses: $/ci/build\n")
        self.git("add", "--", outside, WORKFLOW)
        self.assertEqual(self.check(), (True, []))

    def test_guard_changes_select_existing_index_policy(self):
        self.write(".githooks/pre-commit", "#!/bin/sh\nexit 0\n")
        self.git("add", "--", ".githooks/pre-commit")
        self.assertEqual(self.check(), (True, []))

    def test_deleted_composite_cannot_use_worktree_copy(self):
        self.git("rm", "--cached", "--", ACTION)
        self.assert_failure("same-repository reference must identify exactly one")

    def test_unmerged_manifest_fails_closed(self):
        oid = self.git("rev-parse", ":" + ACTION).strip().decode("ascii")
        self.git("rm", "--cached", "--", ACTION)
        conflict = "".join(f"100644 {oid} {stage}\t{ACTION}\0" for stage in (1, 2, 3))
        self.git("update-index", "-z", "--index-info", data=conflict.encode("utf-8"))
        self.assert_failure("unmerged staged workflow input")

    def test_symlink_manifest_fails_closed(self):
        oid = self.git("hash-object", "-w", "--stdin", data=b"elsewhere.yml\n").strip().decode("ascii")
        self.git("update-index", "--add", "--cacheinfo", "120000", oid, ACTION)
        self.assert_failure("must be a regular file")

    def test_snapshot_preserves_index_and_worktree(self):
        self.stage_change()
        self.write(LOCK, "unstaged content retained\n")
        before = self.git("ls-files", "--stage", "-z")
        self.assertEqual(self.check(), (True, []))
        self.assertEqual(self.git("ls-files", "--stage", "-z"), before)
        self.assertEqual((self.root / LOCK).read_text(encoding="utf-8"), "unstaged content retained\n")

    def test_git_failure_blocks_instead_of_skipping(self):
        with patch("check_staged_workflows.Git.run", side_effect=OSError("fixture Git unavailable")):
            self.assert_failure("fixture Git unavailable")

    def test_non_repository_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="bonsai nonrepo ") as directory:
            selected, errors = check_staged(Path(directory), self.git_executable)
        self.assertTrue(selected)
        self.assertTrue(any("git diff failed" in error for error in errors), errors)

    def test_new_repository_with_staged_workflow_is_checked(self):
        with tempfile.TemporaryDirectory(prefix="bonsai initial index ") as directory:
            root = Path(directory)
            subprocess.run([self.git_executable, "init", "--quiet", str(root)], check=True, timeout=10)
            (root / ".github/workflows").mkdir(parents=True)
            (root / WORKFLOW).write_text(f"jobs:\n  check:\n    steps:\n      - uses: {CHECKOUT}\n", encoding="utf-8")
            subprocess.run([self.git_executable, "add", "--", WORKFLOW], cwd=root, check=True, timeout=10)
            selected, errors = check_staged(root, self.git_executable)
        self.assertTrue(selected)
        self.assertTrue(any("actions.lock" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
