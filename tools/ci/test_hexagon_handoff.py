"""Focused offline controls for Hexagon concurrency, build reuse and owner admission."""

from pathlib import Path
import re
import unittest

from test_foundation_handoff import acyclic, dependencies, job_blocks


ROOT = Path(__file__).resolve().parents[2]


def cache_blocks(job):
    """Inspect only the paths owned by each cache step, never artifact uploads."""
    steps = re.split(r"^      - ", job, flags=re.MULTILINE)[1:]
    return [step for step in steps if "uses: actions/cache@" in step]


class HexagonHandoffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = (ROOT / ".github/workflows/hexagon-qualification.yml").read_text(encoding="utf-8")
        cls.jobs = job_blocks(cls.workflow)

    def test_independent_controls_run_concurrently_and_native_intake_keeps_all_prerequisites(self):
        for name in ("checked-policy", "layout-controls", "ranking-controls", "public-native-abi"):
            self.assertEqual(dependencies(self.jobs[name]), set(), name)
        self.assertEqual(dependencies(self.jobs["controls"]), {"checked-policy", "layout-controls", "ranking-controls"})
        self.assertEqual(dependencies(self.jobs["artifact-binder"]), {"checked-policy"})
        self.assertTrue(acyclic(self.jobs))
        for name in ("session-probe", "inventory", "dispatch"):
            self.assertEqual(dependencies(self.jobs[name]), {"controls"}, name)

    def test_missing_policy_or_cycle_cannot_admit_the_dependency_graph(self):
        jobs = dict(self.jobs)
        del jobs["checked-policy"]
        self.assertFalse(acyclic(jobs))
        jobs = dict(self.jobs)
        jobs["checked-policy"] = "\n    needs: controls\n"
        self.assertFalse(acyclic(jobs))

    def test_exact_policy_tests_run_before_current_run_cli_publication(self):
        producer = self.jobs["checked-policy"]
        self.assertEqual(self.workflow.count("uses: $/.github/actions/bonsai-tools\n"), 1)
        self.assertIn("--package bonsai-tools --all-features --target-dir $env:BONSAI_TOOLS_TARGET --lib hexagon_", producer)
        self.assertNotRegex(producer, r"if:.*cache-hit")
        self.assertLess(producer.index("--lib hexagon_"), producer.index("Stage the current source-bound policy executable"))
        for receipt in ("SchemaVersion = 2", "Revision = $env:GITHUB_SHA", "ProbeSha256 = (Get-FileHash"):
            self.assertIn(receipt, producer)
        self.assertIn("name: bonsai-tools-windows-arm64-${{ github.sha }}", producer)
        for name in ("controls", "artifact-binder", "session-probe", "inventory", "dispatch"):
            self.assertIn("uses: $/.github/actions/use-bonsai-tools", self.jobs[name], name)

    def test_all_layout_and_ranking_checks_remain_selected(self):
        layout = self.jobs["layout-controls"]
        for script in ("test_pq2_bridge.py", "pq2_bridge.py", "verify_packed_fixture.py", "verify_operation_traffic.py", "verify_operation_budget.py", "verify_resident_traffic.py"):
            self.assertIn("python -B tools/hexagon/" + script, layout)
        self.assertIn("inputs.reference_initialization && !inputs.run_session_probe", layout)
        ranking = self.jobs["ranking-controls"]
        for command in ("hardware::ranking::tests", "--all-targets -- -D warnings", "-- --check"):
            self.assertIn(command, ranking)
        self.assertNotRegex(ranking, r"if:.*cache-hit")

    def test_cache_paths_exclude_fresh_bundles_and_evidence(self):
        self.assertIn("CARGO_UNSTABLE_CHECKSUM_FRESHNESS: 'true'", self.workflow)
        self.assertIn("CARGO_BUILD_FINGERPRINT: content", self.workflow)
        for name in ("checked-policy", "ranking-controls", "controls", "artifact-binder"):
            caches = cache_blocks(self.jobs[name])
            self.assertEqual(len(caches), 1, name)
            paths = caches[0].split("path: |", 1)[1].split("          key:", 1)[0]
            self.assertNotRegex(paths, r"(?i)(?:bundle|prepared|evidence|receipt|checkedtools)")
            self.assertNotIn("GITHUB_WORKSPACE", paths)
        policy_cache = cache_blocks(self.jobs["checked-policy"])[0]
        self.assertIn("${{ env.BONSAI_TOOLS_TARGET }}", policy_cache)
        self.assertIn("BONSAI_TOOLS_SOURCE_ID", self.jobs["checked-policy"])
        self.assertIn("BONSAI_TOOLS_CACHE_HOST", self.jobs["checked-policy"])
        self.assertIn("needs.checked-policy.outputs.target-cache-key", self.jobs["controls"])
        self.assertIn("needs.checked-policy.outputs.image-identity == steps.rust.outputs.image-identity", self.jobs["controls"])
        self.assertIn("needs.checked-policy.outputs.compiler-identity == steps.rust.outputs.compiler-identity", self.jobs["controls"])
        for name in ("ranking-controls", "controls", "artifact-binder"):
            self.assertIn("steps.rust.outputs.image-identity", self.jobs[name])
            self.assertIn("steps.rust.outputs.compiler-identity", self.jobs[name])

    def test_native_builds_and_artifacts_remain_fresh_and_source_bound(self):
        controls = self.jobs["controls"]
        self.assertIn("operation = 'build-dispatch'", controls)
        self.assertIn("BONSAI_TOOLS_TARGET=$(Join-Path $env:RUNNER_TEMP 'SwiftBonsai-PackagedTools')", controls)
        binder = self.jobs["artifact-binder"]
        self.assertIn("operation = 'build-packed-artifact'", binder)
        self.assertIn("buildTargetDirectory = (Join-Path $env:RUNNER_TEMP 'SwiftBonsai-Hexagon-Artifact-Target')", binder)
        self.assertIn("outputDirectory = (Join-Path $env:RUNNER_TEMP 'SwiftBonsai-Hexagon-Artifact')", binder)
        for artifact in ("hexagon-dispatch-fixture", "hexagon-hosted-controls", "hexagon-native-probe", "hexagon-projection-probe", "hexagon-packed-artifact", "hexagon-packed-artifact-controls", "hexagon-windows-abi"):
            self.assertIn("name: " + artifact + "-${{ github.run_id }}-${{ github.run_attempt }}", self.workflow)
        for name in ("controls", "artifact-binder"):
            self.assertIn("if: always()", self.jobs[name])
            self.assertNotRegex(self.jobs[name], r"if:.*cache-hit")

    def test_owner_workloads_retain_single_budget_mutex_and_same_run_admission(self):
        for name in ("session-probe", "inventory", "dispatch"):
            owner = self.jobs[name]
            for field in ("timeout-minutes: 5", "group: swift-lora-snapdragon", "cancel-in-progress: false", "BONSAI_HEXAGON_JOB_STARTED_UTC", "WholeJobMaximumSeconds = 300"):
                self.assertIn(field, owner, (name, field))
        for name in ("session-probe", "dispatch"):
            for identity in ("$build.RunnerRevision -cne $env:GITHUB_SHA", "$build.RunId -cne $env:GITHUB_RUN_ID", "$build.RunAttempt -cne $env:GITHUB_RUN_ATTEMPT"):
                self.assertIn(identity, self.jobs[name])


if __name__ == "__main__":
    unittest.main()
