"""Hosted synthetic integrity guards; never read the retained model capture."""
import copy
import hashlib
import importlib.util
from pathlib import Path
import stat
import tempfile
import unittest
import warnings
import zipfile

SPEC = importlib.util.spec_from_file_location("retained_audit", Path(__file__).with_name("audit_retained.py"))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class RetainedAuditTests(unittest.TestCase):
    def producer(self):
        run = {"id": AUDIT.RUN, "head_sha": AUDIT.REVISION, "head_branch": "main",
               "event": "workflow_dispatch", "run_attempt": 1, "path": AUDIT.WORKFLOW,
               "status": "completed", "conclusion": "success",
               "repository": {"full_name": AUDIT.REPOSITORY, "id": 1}}
        artifact = {"id": AUDIT.ARTIFACT, "name": AUDIT.ARTIFACT_NAME, "size_in_bytes": AUDIT.ARCHIVE_BYTES,
                    "expired": False, "digest": "sha256:" + AUDIT.ARCHIVE_SHA,
                    "workflow_run": {"id": AUDIT.RUN, "head_sha": AUDIT.REVISION,
                                     "head_branch": "main", "repository_id": 1}}
        jobs = {"total_count": 2, "jobs": [
            {"name": name, "status": "completed", "conclusion": "success", "run_id": AUDIT.RUN,
             "head_sha": AUDIT.REVISION, "labels": [f"ptq1-numerics-{AUDIT.RUN}"]}
            for name in ("prepare", "numerics")]}
        return run, artifact, jobs

    def test_inexact_producer_artifact_and_job_reject(self):
        AUDIT.validate_producer(*self.producer())
        for target, field, changed in [
            (0, "id", 1), (0, "head_sha", "0" * 40), (0, "head_branch", "branch"),
            (0, "event", "pull_request"), (0, "run_attempt", 2), (0, "path", "different.yml"),
            (0, "conclusion", "failure"), (1, "id", 1), (1, "name", "different"),
            (1, "size_in_bytes", 1), (1, "expired", True), (1, "digest", "sha256:" + "0" * 64),
        ]:
            values = list(copy.deepcopy(self.producer()))
            values[target][field] = changed
            with self.assertRaises(ValueError):
                AUDIT.validate_producer(*values)
        for mutate in (
            lambda values: values[0]["repository"].update(full_name="different/repository"),
            lambda values: values[1]["workflow_run"].update(head_sha="0" * 40),
            lambda values: values[2]["jobs"][1].update(conclusion="failure"),
            lambda values: values[2]["jobs"][1].update(labels=["self-hosted"]),
        ):
            values = copy.deepcopy(self.producer())
            mutate(values)
            with self.assertRaises(ValueError):
                AUDIT.validate_producer(*values)

    def extract(self, entries, expected):
        with tempfile.TemporaryDirectory() as temporary:
            archive, output = Path(temporary) / "test.zip", Path(temporary) / "output"
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(archive, "w") as bundle:
                    for name, data, mode in entries:
                        member = zipfile.ZipInfo(name)
                        member.external_attr = mode << 16
                        bundle.writestr(member, data)
            return AUDIT.extract_checked(archive, output, expected)

    @staticmethod
    def record(name="safe.json", data=b"{}"):
        return {"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def test_regular_files_match_exact_manifest(self):
        expected = {name: self.record(name) for name in ("safe.json", "native/state.native-seq")}
        observed = self.extract([(name, b"{}", stat.S_IFREG | 0o644) for name in reversed(expected)], expected)
        self.assertEqual(observed, sorted(expected.values(), key=lambda record: record["path"]))

    def test_traversal_symlink_and_duplicate_members_reject(self):
        for name in ("../escape.json", "/escape.json", "a/../escape.json", "a//escape.json",
                     "C:/escape.json", "a\\escape.json"):
            with self.assertRaises(ValueError):
                self.extract([(name, b"{}", stat.S_IFREG)], {name: self.record(name)})
        with self.assertRaises(ValueError):
            self.extract([("safe.json", b"{}", stat.S_IFLNK | 0o777)], {"safe.json": self.record()})
        with self.assertRaises(ValueError):
            self.extract([("safe.json", b"{}", stat.S_IFREG)] * 2,
                         {"safe.json": self.record(), "other.json": self.record("other.json")})

    def test_missing_unexpected_extent_and_hash_mismatch_reject(self):
        with self.assertRaises(ValueError):
            self.extract([], {"safe.json": self.record()})
        with self.assertRaises(ValueError):
            self.extract([("other.json", b"{}", stat.S_IFREG)], {"safe.json": self.record()})
        for field, value in (("bytes", 3), ("sha256", "0" * 64)):
            record = self.record()
            record[field] = value
            with self.assertRaises(ValueError):
                self.extract([("safe.json", b"{}", stat.S_IFREG)], {"safe.json": record})

    def test_original_comparator_identity_rejects_before_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            comparator = Path(temporary) / "comparator.py"
            comparator.write_text("raise RuntimeError('must never execute')\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "original comparator source differs"):
                AUDIT.replay(Path(temporary) / "absent-capture", comparator)

    def test_redirect_strips_credentials_and_rejects_unapproved_hosts(self):
        request = AUDIT.urllib.request.Request("https://api.github.com/test",
                                              headers={"Authorization": "Bearer test-only"})
        redirect = AUDIT.ArtifactRedirect()
        accepted = redirect.redirect_request(request, None, 302, "Found", {},
                                              "https://test.blob.core.windows.net/evidence?signature=test-only")
        self.assertFalse(accepted.has_header("Authorization"))
        for target in ("http://test.blob.core.windows.net/evidence", "https://example.com/evidence",
                       "https://user:password@test.blob.core.windows.net/evidence"):
            with self.assertRaises(ValueError):
                redirect.redirect_request(request, None, 302, "Found", {}, target)


if __name__ == "__main__":
    unittest.main()
