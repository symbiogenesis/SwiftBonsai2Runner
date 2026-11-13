"""Exercise job-local toolchain reuse with inert rustup and the actual action bodies."""

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / ".github/actions/rust-toolchain/action.yml"
TOOLCHAIN = "nightly-2026-11-12-aarch64-pc-windows-msvc"


def step_blocks():
    text = ACTION.read_text(encoding="utf-8")
    starts = list(re.finditer(r"^    - name: (.+)$", text, re.MULTILINE))
    return {
        match[1]: text[match.end():starts[index + 1].start() if index + 1 < len(starts) else len(text)]
        for index, match in enumerate(starts)
    }


def body(block):
    script = block.split("      run: |\n", 1)[1]
    return "\n".join(line[8:] for line in script.splitlines())


class RustToolchainTests(unittest.TestCase):
    def invoke(self, phases):
        executable = shutil.which("pwsh")
        self.assertIsNotNone(executable, "PowerShell 7 is required for toolchain control fixtures")
        blocks = step_blocks()
        with tempfile.TemporaryDirectory(prefix="bonsai-toolchain-control-") as temporary:
            scratch = Path(temporary)
            environment = os.environ.copy()
            environment.pop("BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY", None)
            environment.update({
                "PINNED_TOOLCHAIN": TOOLCHAIN,
                "REQUIRED_COMPONENTS": "rustfmt,clippy",
                "EXPECTED_HOST": "",
                "RUNNER_ENVIRONMENT": "github-hosted",
                "ImageOS": "fixture-os",
                "ImageVersion": "fixture-image",
                "GITHUB_OUTPUT": str(scratch / "output"),
                "GITHUB_ENV": str(scratch / "environment"),
                "FIXTURE_HOME": str(scratch / "rustup"),
                "FIXTURE_HOST": "aarch64-pc-windows-msvc",
                "FIXTURE_COMPONENTS": "clippy,rustfmt",
                "FIXTURE_SYSROOT": "",
                "FIXTURE_PATHS": body(blocks["Identify the exact toolchain directory and runner image"]),
                "FIXTURE_VERIFY": body(blocks["Verify the compiler and all required components"]),
                "FIXTURE_PHASES": json.dumps(phases),
            })
            wrapper = r"""
function rustup {
    if ($args[0] -ceq 'show' -and $args[1] -ceq 'home') { return $env:FIXTURE_HOME }
    if ($args[0] -ceq 'run' -and $args[-1] -ceq '-vV') { return @('rustc fixture', "host: $env:FIXTURE_HOST") }
    if ($args[0] -ceq 'run' -and $args[-1] -ceq 'sysroot') {
        if ($env:FIXTURE_SYSROOT) { return $env:FIXTURE_SYSROOT }
        return Join-Path (Join-Path $env:FIXTURE_HOME 'toolchains') $env:PINNED_TOOLCHAIN
    }
    if ($args[0] -ceq 'component') { return @($env:FIXTURE_COMPONENTS -split ',' | Where-Object { $_ }) }
    throw "Unexpected rustup fixture invocation: $args"
}
function Read-Fields($path) {
    $fields = @{}
    foreach ($line in [IO.File]::ReadAllLines($path)) {
        if ($line) { $parts = $line -split '=', 2; $fields[$parts[0]] = $parts[1] }
    }
    return $fields
}
$results = @()
$selected = @{}
foreach ($phase in ($env:FIXTURE_PHASES | ConvertFrom-Json -AsHashtable)) {
    foreach ($key in $phase.environment.Keys) { [Environment]::SetEnvironmentVariable($key, $phase.environment[$key]) }
    [IO.File]::WriteAllText($env:GITHUB_OUTPUT, '')
    [IO.File]::WriteAllText($env:GITHUB_ENV, '')
    $failure = $null
    try {
        if ($phase.name -ceq 'paths') {
            & ([scriptblock]::Create($env:FIXTURE_PATHS)) | Out-Null
        } else {
            $env:REQUIRED_COMPONENTS = $selected.components
            $env:EXPECTED_HOST = $selected.host
            $env:TOOLCHAIN_PATH = $selected.'toolchain-path'
            $env:TOOLCHAIN_REUSE_KEY = $selected.'reuse-key'
            & ([scriptblock]::Create($env:FIXTURE_VERIFY)) | Out-Null
        }
    } catch { $failure = $_.ToString() }
    $outputs = Read-Fields $env:GITHUB_OUTPUT
    $published = Read-Fields $env:GITHUB_ENV
    foreach ($key in $published.Keys) { [Environment]::SetEnvironmentVariable($key, $published[$key]) }
    if ($phase.name -ceq 'paths' -and -not $failure) { $selected = $outputs }
    $results += @{ Outputs=$outputs; Published=$published; Failure=$failure }
}
ConvertTo-Json -InputObject @($results) -Depth 8 -Compress
"""
            result = subprocess.run(
                [executable, "-NoProfile", "-NonInteractive", "-Command", wrapper],
                cwd=scratch, env=environment, capture_output=True, text=True,
                timeout=15, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)

    @staticmethod
    def phase(name, **environment):
        return {"name": name, "environment": environment}

    def test_repeat_request_reuses_only_after_successful_verification(self):
        results = self.invoke([
            self.phase("paths"), self.phase("paths"), self.phase("verify"),
            self.phase("paths", REQUIRED_COMPONENTS="clippy rustfmt rustfmt", EXPECTED_HOST=""),
            self.phase("verify"),
        ])
        self.assertEqual([result["Outputs"].get("reuse-verified") for result in results],
                         ["false", "false", None, "true", None])
        self.assertTrue(all(result["Failure"] is None for result in results))
        self.assertEqual(results[2]["Published"], results[4]["Published"])
        self.assertIn("compiler-identity", results[4]["Outputs"])

    def test_minimal_then_full_components_cannot_skip_installation(self):
        results = self.invoke([
            self.phase("paths", REQUIRED_COMPONENTS=""), self.phase("verify"),
            self.phase("paths", REQUIRED_COMPONENTS="rustfmt,clippy", EXPECTED_HOST=""),
        ])
        self.assertEqual(results[-1]["Outputs"]["reuse-verified"], "false")
        self.assertNotEqual(results[0]["Outputs"]["reuse-key"], results[-1]["Outputs"]["reuse-key"])

    def test_changed_image_or_toolchain_cannot_reuse_the_previous_installation(self):
        for changes in ({"ImageVersion": "new-image"},
                        {"PINNED_TOOLCHAIN": "nightly-2026-11-11-aarch64-pc-windows-msvc"}):
            with self.subTest(changes=changes):
                results = self.invoke([
                    self.phase("paths"), self.phase("verify"),
                    self.phase("paths", EXPECTED_HOST="", REQUIRED_COMPONENTS="clippy,rustfmt", **changes),
                ])
                self.assertEqual(results[-1]["Outputs"]["reuse-verified"], "false")

    def test_reused_installation_still_rejects_wrong_host_sysroot_and_missing_component(self):
        for changes in ({"FIXTURE_HOST": "x86_64-pc-windows-msvc"},
                        {"FIXTURE_SYSROOT": str(ROOT)},
                        {"FIXTURE_COMPONENTS": "rustfmt"}):
            with self.subTest(changes=changes):
                results = self.invoke([
                    self.phase("paths"), self.phase("verify"),
                    self.phase("paths", REQUIRED_COMPONENTS="clippy,rustfmt", EXPECTED_HOST=""),
                    self.phase("verify", **changes),
                ])
                self.assertEqual(results[2]["Outputs"]["reuse-verified"], "true")
                self.assertIsNotNone(results[-1]["Failure"])
                self.assertEqual(results[-1]["Published"], {})

    def test_action_gates_only_transfer_and_installation(self):
        blocks = step_blocks()
        self.assertIn("steps.paths.outputs.reuse-verified != 'true'", blocks["Reuse only the exact toolchain directory"])
        self.assertIn("steps.paths.outputs.reuse-verified != 'true'", blocks["Install the pinned compiler on a cache miss"])
        self.assertNotIn("      if:", blocks["Verify the compiler and all required components"])
        self.assertNotIn("BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY", blocks["Reuse only the exact toolchain directory"])


if __name__ == "__main__":
    unittest.main()
