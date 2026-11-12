"""Exercise the shared PowerShell regression step with inert Cargo/guard fixtures."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / ".github/actions/native-regressions/action.yml"


def action_body():
    lines = ACTION.read_text(encoding="utf-8-sig").splitlines()
    start = lines.index("      run: |") + 1
    return "\n".join(line[8:] for line in lines[start:])


class NativeRegressionsTests(unittest.TestCase):
    def invoke(self, selections, *, cargo_exit=0, guard_exit=0, **overrides):
        executable = shutil.which("pwsh")
        self.assertIsNotNone(executable, "PowerShell 7 is required for native control fixtures")
        with tempfile.TemporaryDirectory(prefix="bonsai-native-control-") as temporary:
            scratch = Path(temporary)
            cargo = scratch / "cargo.ps1"
            guard = scratch / "guard.ps1"
            invocations = scratch / "cargo.jsonl"
            receipts = scratch / "guard.jsonl"
            cargo.write_text(
                "ConvertTo-Json -InputObject @($args) -Compress | Add-Content -LiteralPath $env:FIXTURE_INVOCATIONS\n"
                "'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'\n"
                "$global:LASTEXITCODE = [int]$env:FIXTURE_CARGO_EXIT\n",
                encoding="utf-8",
            )
            guard.write_text(
                "$request = $args[2] | ConvertFrom-Json\n"
                "$args[2] | Add-Content -LiteralPath $env:FIXTURE_RECEIPTS\n"
                "$global:LASTEXITCODE = if ($request.exitCode -ne 0) { 1 } else { [int]$env:FIXTURE_GUARD_EXIT }\n",
                encoding="utf-8",
            )
            environment = os.environ.copy()
            environment.update({
                "REGRESSION_PACKAGE": "bonsai-tools",
                "REGRESSION_TARGET_KIND": "lib",
                "REGRESSION_TARGET_NAME": "",
                "REGRESSION_SELECTIONS": json.dumps(selections),
                "REGRESSION_TARGET_DIRECTORY": str(scratch / "target"),
                "REGRESSION_TOOLCHAIN": "nightly-2026-11-12-aarch64-pc-windows-msvc",
                "GITHUB_WORKSPACE": str(scratch / "source"),
                "BONSAI_CI_PATH": str(guard),
                "FIXTURE_CARGO": str(cargo),
                "FIXTURE_INVOCATIONS": str(invocations),
                "FIXTURE_RECEIPTS": str(receipts),
                "FIXTURE_CARGO_EXIT": str(cargo_exit),
                "FIXTURE_GUARD_EXIT": str(guard_exit),
                "FIXTURE_BODY": action_body(),
            })
            environment.update(overrides)
            wrapper = (
                "function Get-Command { param($Name, $CommandType, $ErrorAction) "
                "[pscustomobject]@{ Source=$env:FIXTURE_CARGO } }; "
                "try { & ([scriptblock]::Create($env:FIXTURE_BODY)); exit 0 } "
                "catch { [Console]::Error.WriteLine($_); exit 1 }"
            )
            result = subprocess.run(
                [executable, "-NoProfile", "-NonInteractive", "-Command", wrapper],
                cwd=scratch, env=environment, capture_output=True, text=True,
                timeout=15, check=False,
            )
            calls = [json.loads(line) for line in invocations.read_text(encoding="utf-8-sig").splitlines()] if invocations.exists() else []
            proofs = [json.loads(line) for line in receipts.read_text(encoding="utf-8-sig").splitlines()] if receipts.exists() else []
            return result, calls, proofs

    def test_exact_and_grouped_controls_preserve_arguments_and_typed_counts(self):
        result, calls, proofs = self.invoke([
            {"name": "module::exact_case", "exact": True, "expected-passed": 1},
            {"name": "owner_shutdown_", "exact": False, "expected-passed": None},
        ])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0][:4], ["+nightly-2026-11-12-aarch64-pc-windows-msvc", "test", "--locked", "--release"])
        self.assertEqual(calls[0][-4:], ["--lib", "module::exact_case", "--", "--exact"])
        self.assertEqual(calls[1][-2:], ["--lib", "owner_shutdown_"])
        self.assertEqual([proof["expectedPassed"] for proof in proofs], [1, None])
        self.assertTrue(all(proof["operation"] == "test-output" for proof in proofs))

    def test_named_binary_target_is_forwarded_without_enabling_ignored_tests(self):
        result, calls, _ = self.invoke(
            [{"name": "owned_broker_", "exact": False, "expected-passed": None}],
            REGRESSION_PACKAGE="bonsai-chat", REGRESSION_TARGET_KIND="bin", REGRESSION_TARGET_NAME="bonsai-chat",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls[0][-3:], ["--bin", "bonsai-chat", "owned_broker_"])
        self.assertNotIn("--ignored", calls[0])

    def test_complete_selection_is_validated_before_any_control_starts(self):
        valid = {"name": "valid_case", "exact": True, "expected-passed": 1}
        for invalid in [
            {"name": "--ignored", "exact": True, "expected-passed": 1},
            {"name": "other", "exact": "true", "expected-passed": 1},
            {"name": "other", "exact": True, "expected-passed": None},
            {"name": "other", "exact": False, "expected-passed": 0},
            valid,
        ]:
            with self.subTest(invalid=invalid):
                result, calls, proofs = self.invoke([valid, invalid])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((calls, proofs), ([], []))

    def test_cargo_failure_reaches_the_guard_and_stops_later_controls(self):
        result, calls, proofs = self.invoke([
            {"name": "first", "exact": True, "expected-passed": 1},
            {"name": "second", "exact": True, "expected-passed": 1},
        ], cargo_exit=9)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1)
        self.assertEqual(proofs[0]["exitCode"], 9)

    def test_guard_failure_stops_the_action(self):
        result, _, proofs = self.invoke([
            {"name": "one", "exact": True, "expected-passed": 1},
        ], guard_exit=7)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(proofs), 1)


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(NativeRegressionsTests)
    if suite.countTestCases() != 5:
        raise RuntimeError("The five shared native-control fixtures must remain selected")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    raise SystemExit(0 if result.wasSuccessful() and result.testsRun == 5 else 1)
