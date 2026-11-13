"""Guard Foundation's producer DAG and complete package-check ownership offline."""

import json
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


def job_blocks(text):
    """Read the workflow's simple, two-space job mapping without a YAML dependency."""
    text = re.split(r"^jobs:\s*$", text, maxsplit=1, flags=re.MULTILINE)[1]
    matches = list(re.finditer(r"^  ([a-z][a-z-]*):\s*$", text, re.MULTILINE))
    return {
        match[1]: text[match.end():matches[index + 1].start() if index + 1 < len(matches) else len(text)]
        for index, match in enumerate(matches)
    }


def dependencies(block):
    match = re.search(r"^    needs: (.+)$", block, re.MULTILINE)
    if not match:
        return set()
    return set(match[1].strip("[]").replace(" ", "").split(","))


def acyclic(jobs):
    """Reject a missing dependency or cycle rather than leaving consumers queued."""
    remaining = {name: dependencies(block) for name, block in jobs.items()}
    done = set()
    while remaining:
        ready = {name for name, needs in remaining.items() if needs <= done}
        if not ready:
            return False
        done.update(ready)
        remaining = {name: needs for name, needs in remaining.items() if name not in ready}
    return True


class FoundationHandoffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.jobs = job_blocks((ROOT / ".github/workflows/foundation.yml").read_text(encoding="utf-8"))
        cls.config = json.loads((ROOT / ".github/ci/impact.json").read_text(encoding="utf-8"))

    def test_consumers_wait_for_checked_cli_and_aggregate_keeps_maintenance(self):
        for name in ("speech", "foundation-tests", "release-integration"):
            self.assertEqual(dependencies(self.jobs[name]), {"changes", "checked-tools"}, name)
        self.assertEqual(dependencies(self.jobs["checked-tools"]), {"changes"})
        self.assertEqual(dependencies(self.jobs["maintenance"]), {"changes"})
        self.assertEqual(dependencies(self.jobs["speech-tts"]), {"changes"})
        expected = set(self.config["workflows"]["foundation"]) | {"changes"}
        self.assertEqual(dependencies(self.jobs["foundation"]), expected)
        self.assertEqual(set(self.jobs), expected | {"foundation"})
        self.assertTrue(acyclic(self.jobs))

    def test_missing_producer_and_cycle_fail_the_dependency_control(self):
        jobs = dict(self.jobs)
        del jobs["checked-tools"]
        self.assertFalse(acyclic(jobs))
        jobs = dict(self.jobs)
        jobs["checked-tools"] = "\n    needs: speech\n"
        self.assertFalse(acyclic(jobs))

    def test_every_registered_package_keeps_one_owning_check_lane(self):
        registry = json.loads((ROOT / "config/rust-packages.json").read_text(encoding="utf-8"))
        owned = []
        for name in ("maintenance", "checked-tools"):
            match = re.search(r"^          packages: '(\[.*\])'$", self.jobs[name], re.MULTILINE)
            self.assertIsNotNone(match, name)
            packages = json.loads(match[1])
            group = self.config["groups"]["tools" if name == "checked-tools" else "maintenance"]
            registered = {entry["Name"] for entry in registry["Packages"]}
            expected = (
                registered - set(group.get("exclude_packages", []))
                if group["packages"] == ["*"] else set(group["packages"])
            )
            self.assertEqual(set(packages), expected, name)
            owned.extend(packages)
        self.assertEqual(len(owned), len(set(owned)))
        self.assertEqual(set(owned), {
            "bonsai-artifacts", "bonsai-audio", "bonsai-cpu-policy",
            "bonsai-windows-process", "bonsai-windows-credentials",
            "bonsai-ci", "bonsai-tools",
        })

    def test_cli_is_checked_before_current_run_publication(self):
        producer = self.jobs["checked-tools"]
        self.assertLess(producer.index("uses: $/.github/actions/check-rust"), producer.index("Stage the checked maintenance CLI"))
        self.assertIn("lane: tools", producer)
        self.assertIn("SchemaVersion = 2", producer)
        self.assertIn("Revision = $env:GITHUB_SHA", producer)
        self.assertIn("ProbeSha256 = (Get-FileHash", producer)
        self.assertIn("name: bonsai-tools-windows-arm64-${{ github.sha }}", producer)
        consumer = (ROOT / ".github/actions/use-bonsai-tools/action.yml").read_text(encoding="utf-8")
        self.assertIn("name: bonsai-tools-windows-arm64-${{ github.sha }}", consumer)
        self.assertNotIn("run-id:", consumer)
        self.assertIn("$receipt.Revision -cne $env:GITHUB_SHA", consumer)

    def test_router_and_gate_select_the_new_producer_independently(self):
        self.assertEqual(self.config["workflows"]["foundation"]["checked-tools"], "tools")
        self.assertEqual(self.config["workflows"]["foundation"]["maintenance"], "maintenance")
        self.assertNotIn("tools", self.config["groups"]["maintenance"].get("groups", []))
        self.assertIn("bonsai-tools", self.config["groups"]["maintenance"]["exclude_packages"])
        self.assertIn("tools", self.config["groups"]["rust_lanes"]["groups"])

    def test_speech_lanes_keep_independent_selection_and_required_results(self):
        for job, lane in (("speech", "asr"), ("speech-tts", "tts")):
            block = self.jobs[job]
            self.assertIn(f"contains(fromJSON(needs.changes.outputs.speech_lanes), '{lane}')", block)
            self.assertIn(f"lane: {lane}", block)
            self.assertIn(f"packages: '[\"bonsai-speech-{lane}\"]'", block)
            self.assertEqual(self.config["workflows"]["foundation"][job], f"speech_{lane}")
        self.assertIn("uses: $/.github/actions/use-bonsai-tools", self.jobs["speech"])
        self.assertNotIn("bonsai-tools", self.jobs["speech-tts"])
        self.assertNotIn("strategy:", self.jobs["speech"])

    def test_only_asr_needs_native_compiler_preparation(self):
        action = (ROOT / ".github/actions/check-rust/action.yml").read_text(encoding="utf-8")
        self.assertIn("if ($env:RUST_LANE -ceq 'asr')", action)
        self.assertIn("if: steps.packages.outputs.has-packages == 'true' && inputs.lane == 'asr'", action)
        self.assertIn("--full-tests --build", action)
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertRegex(release, r"uses: \$/\.github/actions/bonsai-tools\n\s+if: matrix.lane == 'asr'\n")
        self.assertIn("--all-targets", (ROOT / "tools/ci/src/checks/mod.rs").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
