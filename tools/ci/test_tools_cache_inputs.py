"""Check packaged-tool dependency/embedded inputs and real environment identities.

Source closure uses Cargo manifests and production literal include expressions,
without invoking Cargo or a compiler. PowerShell fixtures run only the action's
environment identity block; they neither build Rust nor execute a cached program.
"""

import fnmatch
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / ".github/actions/bonsai-tools/action.yml"
LITERAL = re.compile(r'include_(?:str|bytes)!\(\s*"([^"\\]+)"\s*\)')
CONCAT = re.compile(r'include_(?:str|bytes)!\(\s*concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),\s*"([^"\\]+)"\s*\)\s*\)')


def package_closure(root, initial):
    """Follow workspace and explicit local dependencies through every target table."""
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    result, pending = set(), [root / initial]
    while pending:
        package = pending.pop().resolve()
        if package in result:
            continue
        package.relative_to(root.resolve())
        result.add(package)
        manifest = tomllib.loads((package / "Cargo.toml").read_text(encoding="utf-8"))
        tables = [manifest, *manifest.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for name, declaration in table.get(kind, {}).items():
                    base = package
                    if not isinstance(declaration, dict):
                        continue
                    if declaration.get("workspace"):
                        declaration = workspace["dependencies"][name]
                        base = root
                    if isinstance(declaration, dict) and "path" in declaration:
                        pending.append(base / declaration["path"])
    return result


def embedded_files(package, source):
    """Resolve ordinary and manifest-relative production literal byte inclusions."""
    text = source.read_text(encoding="utf-8")
    # Repository test modules are separate files or follow this final marker.
    # Synthetic Rust snippets in tests are not production compile inclusions.
    text = text.split("#[cfg(test)]", 1)[0]
    ordinary, manifest_relative = LITERAL.findall(text), CONCAT.findall(text)
    if len(re.findall(r"\binclude_(?:str|bytes)!\s*\(", text)) != len(ordinary) + len(manifest_relative):
        raise ValueError(f"Extend cache closure controls for an unfamiliar include expression in {source}")
    paths = [(source.parent / value).resolve() for value in ordinary]
    paths += [(package / value.lstrip("/")).resolve() for value in manifest_relative]
    if source.name == "build.rs":
        paths += [(package / value).resolve() for value in re.findall(r'\.set_icon\("([^"\\]+)"\)', text)]
    return paths


def required_files(root):
    """Enumerate path-crate sources, manifests and their embedded production files."""
    result = {root / "Cargo.toml", root / "Cargo.lock", root / "rust-toolchain.toml"}
    result.update(path for path in (root / ".cargo").rglob("*") if path.is_file())
    for package in package_closure(root, "tools/bonsai"):
        result.add(package / "Cargo.toml")
        for source in package.rglob("*.rs"):
            result.add(source)
            if "tests" in source.relative_to(package).parts or source.name.endswith(("tests.rs", "_tests.rs")):
                continue
            result.update(embedded_files(package, source))
    return {path.resolve().relative_to(root.resolve()).as_posix() for path in result}


def patterns():
    action = ACTION.read_text(encoding="utf-8")
    match = re.search(r"TOOLS_SOURCE_ID: \$\{\{ hashFiles\((.*)\) \}\}", action)
    if not match:
        raise ValueError("Missing exact packaged-tool source hash.")
    return re.findall(r"'([^']+)'", match[1])


def uncovered(files, globs):
    return {path for path in files if not any(fnmatch.fnmatchcase(path, pattern) for pattern in globs)}


class ToolsCacheInputTests(unittest.TestCase):
    def test_real_path_dependency_and_embedded_input_closure_is_hashed(self):
        files = required_files(ROOT)
        self.assertEqual(uncovered(files, patterns()), set())
        for path in files:
            self.assertTrue((ROOT / path).is_file(), path)

    def test_missing_audio_or_embedded_input_is_detected(self):
        files = required_files(ROOT)
        # Remove coverage rather than editing repository inputs: each discovered
        # production input must independently require cache invalidation.
        for path in ("src/audio/src/lib.rs", "tools/canary-asr/model.lock.json",
                     "tools/canary-asr/tokenizer.lock.json", "tools/qwen-tts/model.lock.json",
                     "tools/hexagon/diagnostic.manifest", "src/chat-ui/assets/bonsai.ico", "LICENSE", "NOTICE"):
            with self.subTest(path=path):
                self.assertIn(path, files)
                reduced = [pattern for pattern in patterns() if not fnmatch.fnmatchcase(path, pattern)]
                self.assertIn(path, uncovered(files, reduced))

    def test_dependency_traversal_and_literal_paths_use_manifest_owners(self):
        with tempfile.TemporaryDirectory(prefix="bonsai-cache-closure-") as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text('[workspace.dependencies]\nshared = { path = "shared" }\n', encoding="utf-8")
            for name, manifest in (("tools", '[dependencies]\nshared.workspace = true\n[target.\'cfg(windows)\'.build-dependencies]\nleaf = { path = "../leaf" }\n'),
                                   ("shared", ""), ("leaf", "")):
                directory = root / name
                directory.mkdir()
                (directory / "Cargo.toml").write_text(manifest, encoding="utf-8")
            self.assertEqual(package_closure(root, "tools"), {root / "tools", root / "shared", root / "leaf"})
            source = root / "tools/build.rs"
            source.write_text('const A: &str = include_str!("../shared/data");\n'
                              'const B: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../leaf/data"));\n'
                              'r.set_icon("../leaf/icon.ico");\n', encoding="utf-8")
            self.assertEqual(set(embedded_files(source.parent, source)),
                             {root / "shared/data", root / "leaf/data", root / "leaf/icon.ico"})
            source.write_text('const A: &str = include_str!(unreviewed!());', encoding="utf-8")
            with self.assertRaises(ValueError):
                embedded_files(source.parent, source)

    def test_native_executable_caches_share_the_exercised_environment_filter(self):
        filters = []
        for name in ("bonsai-tools", "ci-tool"):
            action = (ROOT / f".github/actions/{name}/action.yml").read_text(encoding="utf-8")
            filters.append([line.strip() for line in action.splitlines() if "$buildInputs = @(" in line])
        self.assertEqual(len(filters[0]), 1)
        self.assertEqual(filters[0], filters[1])

    def test_both_cache_layers_and_receipt_bind_the_same_inputs(self):
        action = ACTION.read_text(encoding="utf-8")
        exact = action.split("    - name: Reuse only the exact packaged CLI", 1)[1].split("    - name:", 1)[0]
        incremental = action.split("    - name: Reuse Cargo downloads", 1)[1].split("    - uses:", 1)[0]
        for block in (exact, incremental):
            self.assertIn("${{ env.BONSAI_TOOLS_SOURCE_ID }}", block)
            self.assertIn("${{ env.BONSAI_TOOLS_CACHE_HOST }}", block)
        self.assertNotIn("restore-keys:", exact)
        self.assertIn("HostId = $env:BONSAI_TOOLS_CACHE_HOST", action)
        self.assertIn("$receipt.HostId -cne $env:BONSAI_TOOLS_CACHE_HOST", action)

    def environment_identities(self, scenarios):
        executable = shutil.which("pwsh")
        self.assertIsNotNone(executable, "PowerShell 7 is required for packaged-tool identity fixtures")
        action = ACTION.read_text(encoding="utf-8")
        block = action.split("      run: |\n", 1)[1].split("    - name:", 1)[0]
        block = block[block.index("        $buildInputs = "):block.index('        "BONSAI_TOOLS_SOURCE_ID=')]
        body = "\n".join(line[8:] for line in block.splitlines())
        with tempfile.TemporaryDirectory(prefix="bonsai-tools-identity-") as temporary:
            scratch = Path(temporary)
            cargo_home = scratch / "cargo"
            cargo_home.mkdir()
            environment = os.environ.copy()
            for name in list(environment):
                if name.upper().startswith(("RUST", "CARGO_")):
                    environment.pop(name)
            environment.update({"TOOLS_SOURCE_ID": "a" * 64, "RUNNER_ENVIRONMENT": "github-hosted",
                                "ImageOS": "fixture-os", "ImageVersion": "fixture-image",
                                "RUNNER_TEMP": str(scratch), "CARGO_HOME": str(cargo_home),
                                "GITHUB_ENV": str(scratch / "environment"), "FIXTURE_BODY": body,
                                "FIXTURE_SCENARIOS": json.dumps(scenarios)})
            wrapper = r"""
$results = @()
foreach ($scenario in ($env:FIXTURE_SCENARIOS | ConvertFrom-Json -AsHashtable)) {
    foreach ($key in $scenario.environment.Keys) { [Environment]::SetEnvironmentVariable($key, $scenario.environment[$key]) }
    foreach ($name in @('config', 'config.toml')) {
        $path = Join-Path $env:CARGO_HOME $name
        if ($scenario.ContainsKey($name)) { [IO.File]::WriteAllText($path, $scenario[$name]) }
        elseif (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
    }
    [IO.File]::WriteAllText($env:GITHUB_ENV, '')
    & ([scriptblock]::Create($env:FIXTURE_BODY)) | Out-Null
    $entry = [IO.File]::ReadAllLines($env:GITHUB_ENV) | Where-Object { $_.StartsWith('BONSAI_TOOLS_CACHE_HOST=') }
    $results += $entry
}
ConvertTo-Json -InputObject @($results) -Compress
"""
            result = subprocess.run([executable, "-NoProfile", "-NonInteractive", "-Command", wrapper],
                                    cwd=scratch, env=environment, capture_output=True, text=True,
                                    timeout=15, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)

    def test_compiler_profile_and_cargo_home_changes_invalidate_real_identity(self):
        for variable in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
                         "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP", "CARGO_INCREMENTAL", "CARGO_BUILD_RUSTFLAGS", "CARGO_PROFILE_RELEASE_LTO",
                         "CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER"):
            with self.subTest(variable=variable):
                values = self.environment_identities([{"environment": {}}, {"environment": {variable: "changed"}}])
                self.assertNotEqual(values[0], values[1])
        for filename in ("config", "config.toml"):
            with self.subTest(filename=filename):
                values = self.environment_identities([{"environment": {}}, {"environment": {}, filename: "[build]\njobs = 2\n"},
                                                      {"environment": {}, filename: "[build]\njobs = 4\n"}])
                self.assertEqual(len(set(values)), 3)

    def test_mixed_case_environment_names_have_the_same_build_identity(self):
        for canonical, mixed in (("RUSTFLAGS", "RustFlags"), ("CARGO_PROFILE_RELEASE_LTO", "Cargo_Profile_Release_Lto")):
            with self.subTest(canonical=canonical):
                values = self.environment_identities([
                    {"environment": {canonical: "changed"}},
                    {"environment": {canonical: None, mixed: "changed"}},
                    {"environment": {mixed: None}},
                ])
                self.assertEqual(values[0], values[1])
                self.assertNotEqual(values[1], values[2])

    def test_run_metadata_and_overridden_build_paths_do_not_partition_reuse(self):
        values = self.environment_identities([{"environment": {}}, {"environment": {"GITHUB_RUN_ID": "new-run",
                                               "CARGO_TARGET_DIR": "different-target", "CARGO_BUILD_FINGERPRINT": "mtime"}}])
        self.assertEqual(values[0], values[1])


if __name__ == "__main__":
    unittest.main()
