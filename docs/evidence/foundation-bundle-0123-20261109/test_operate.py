"""Offline run-identity regression; importing the operator never calls main."""

import importlib.util
import json
from pathlib import Path
import unittest


DIRECTORY = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("foundation_operator_fixture", DIRECTORY / "operate.py")
OPERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OPERATOR)
NONCE = "prism0123-20261109-a7cdee96"


class RunIdentityTests(unittest.TestCase):
    def setUp(self):
        # Minimal exact fields transcribed from raw/26-discover-foundation-bundle.json,
        # whose retained SHA-256 is 1bca71e1a726f3691bc3ecd47afdd2ff31eaf2185a31f10d33d31c2a25d66f53.
        self.observed = json.loads((DIRECTORY / "fixtures/observed-bundle-run.json").read_text(encoding="utf-8"))

    def matches(self, record, **expected):
        return OPERATOR.run_identity_matches(record, OPERATOR.WORKFLOW, "workflow_dispatch", NONCE, **expected)

    def test_observed_dynamic_run_name_binds_without_display_label_authority(self):
        self.assertTrue(self.matches(self.observed))
        for name in ("Foundation bundle", "Future display wording", None):
            with self.subTest(name=name):
                changed = dict(self.observed, name=name)
                self.assertTrue(self.matches(changed))

    def test_source_workflow_event_branch_nonce_and_attempt_mismatches_fail(self):
        for field, value in (("head_sha", "0" * 40), ("head_branch", "other"),
                             ("event", "push"), ("path", ".github/workflows/foundation.yml"),
                             ("path", None), ("display_title", "Foundation bundle / another-operation"),
                             ("run_attempt", 2), ("run_attempt", True)):
            with self.subTest(field=field, value=value):
                changed = dict(self.observed)
                changed[field] = value
                self.assertFalse(self.matches(changed))

    def test_run_and_workflow_ids_require_positive_integers(self):
        for field in ("id", "workflow_id"):
            for value in (0, -1, True, "42712889670", None):
                with self.subTest(field=field, value=value):
                    changed = dict(self.observed)
                    changed[field] = value
                    self.assertFalse(self.matches(changed))

    def test_reobservation_cannot_change_bound_run_or_workflow_id(self):
        self.assertTrue(self.matches(self.observed, expected_run_id=42712889670,
                                     expected_workflow_id=412647161))
        self.assertFalse(self.matches(self.observed, expected_run_id=42712889671))
        self.assertFalse(self.matches(self.observed, expected_workflow_id=412647162))

    def test_bundle_requires_explicit_nonce_and_matching_workflow_event(self):
        self.assertFalse(OPERATOR.run_identity_matches(self.observed, OPERATOR.WORKFLOW,
                                                       "workflow_dispatch", None))
        self.assertFalse(OPERATOR.run_identity_matches(self.observed, OPERATOR.WORKFLOW,
                                                       "push", NONCE))

    def test_automatic_foundation_still_binds_exact_source_push_identity(self):
        automatic = dict(self.observed, id=42712826648, name="Foundation", display_title="source push",
                         event="push", path=".github/workflows/foundation.yml")
        self.assertTrue(OPERATOR.run_identity_matches(automatic, OPERATOR.FULL_WORKFLOW, "push"))
        self.assertFalse(OPERATOR.run_identity_matches(automatic, OPERATOR.FULL_WORKFLOW, "workflow_dispatch"))


if __name__ == "__main__":
    unittest.main()
