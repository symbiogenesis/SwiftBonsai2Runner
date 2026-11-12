"""One bounded, model-free static release check. Never retries a workload."""

import ctypes
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
import xml.etree.ElementTree as ET
import zipfile


class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]


def power():
    status = Power()
    try:
        query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
        query.argtypes = [ctypes.POINTER(Power)]
        query.restype = ctypes.c_int
        known = query(ctypes.byref(status)) != 0
    except (OSError, AttributeError):
        known = False
    return {"observedUtc": datetime.now(timezone.utc).isoformat(),
            "source": "GetSystemPowerStatus",
            "acConnected": bool(status.ac) if known and status.ac in (0, 1) else None,
            "batterySaver": bool(status.saver) if known and status.saver in (0, 1) else None,
            "chargePercent": status.charge if known and status.charge <= 100 else None}


def main():
    started = time.monotonic()
    here = Path(__file__).resolve().parent
    summary_path = here / "summary.json"
    if summary_path.exists():
        raise RuntimeError("This workload is already closed; no retry or clock restart.")
    root = here.parents[2]
    plan = json.loads((here / "workload-plan.json").read_text(encoding="utf-8"))
    raw = here / "raw"
    raw.mkdir(exist_ok=False)
    receipt = {"schemaVersion": 1, "kind": "windows-release-static-validation",
               "startedUtc": datetime.now(timezone.utc).isoformat(),
               "maximumAggregateSeconds": plan["maximumAggregateSeconds"],
               "powerBefore": power(), "checks": [],
               "comparisonArms": [], "performanceQualified": False,
               "hostedBuildAndMsiAcceptance": "pending-owner-source-publication",
               "releasePublished": False, "gitPushes": 0, "ciRunsDispatched": 0}
    scratch = Path(tempfile.mkdtemp(prefix="SwiftBonsai-ReleaseStatic-"))
    git = Path(r"C:\Program Files\Microsoft Visual Studio\18\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe")

    def check(name, command):
        remaining = plan["maximumAggregateSeconds"] - 30 - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError("Original aggregate allowance exhausted; stop and retain.")
        result = subprocess.run([str(item) for item in command], cwd=root,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=min(30, remaining), check=False)
        (raw / f"{name}.log").write_bytes(result.stdout)
        receipt["checks"].append({"name": name, "exitCode": result.returncode,
                                   "passed": result.returncode == 0})
        return result.stdout.decode("utf-8", errors="replace").strip()

    try:
        receipt["sourceRevision"] = check("source-revision", [git, "rev-parse", "HEAD"])
        archive = plan["actionlint"]
        with urllib.request.urlopen(archive["archive"], timeout=20) as response:
            data = response.read(8 * 1024 * 1024 + 1)
        digest = hashlib.sha256(data).hexdigest()
        if digest != archive["sha256"]:
            raise RuntimeError("The actionlint archive differs from its official digest.")
        receipt["actionlintArchiveSha256"] = digest
        with zipfile.ZipFile(io.BytesIO(data)) as bundle:
            (scratch / "actionlint.exe").write_bytes(bundle.read("actionlint.exe"))
        check("actionlint", [scratch / "actionlint.exe", "-shellcheck=", "-pyflakes=",
                             root / ".github/workflows/release.yml"])

        installer_xml = root / "installer/windows/Package.wxs"
        ET.parse(installer_xml)
        json.loads((root / "installer/windows/toolchain.json").read_text(encoding="utf-8"))
        receipt["checks"].append({"name": "installer-xml-and-toolchain-json", "passed": True})

        workflow = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        lines = workflow.splitlines()
        blocks = scratch / "blocks"
        blocks.mkdir()
        for index, line in enumerate(lines):
            match = re.fullmatch(r"(\s*)run: \|", line)
            if not match:
                continue
            indent = len(match[1])
            body = []
            for following in lines[index + 1:]:
                if following.strip() and len(following) - len(following.lstrip()) <= indent:
                    break
                body.append(following[indent + 2:])
            (blocks / f"run-{index}.ps1").write_text("\n".join(body), encoding="utf-8")
        parser = scratch / "parse.ps1"
        parser.write_text(r'''param([string]$Root, [string]$Blocks)
$ErrorActionPreference = 'Stop'
$scripts = @(Get-ChildItem -LiteralPath (Join-Path $Root 'scripts/release') -Filter '*.ps1') +
    @(Get-ChildItem -LiteralPath (Join-Path $Root '.github/release') -Filter '*.ps1') +
    @(Get-ChildItem -LiteralPath $Blocks -Filter '*.ps1')
foreach ($file in $scripts) {
    $tokens = $null
    $issues = $null
    [void][Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$issues)
    if ($issues.Count -gt 0) { $issues | Out-String | Write-Output; throw "Invalid PowerShell: $($file.Name)" }
}
Write-Output "Parsed $($scripts.Count) release scripts and workflow run blocks."
''', encoding="utf-8")
        check("powershell-parser", ["powershell.exe", "-NoProfile", "-NonInteractive",
                                    "-File", parser, "-Root", root, "-Blocks", blocks])
        rustfmt = Path(os.environ["USERPROFILE"]) / ".rustup/toolchains/nightly-2026-11-09-aarch64-pc-windows-msvc/bin/rustfmt.exe"
        check("launcher-rustfmt", [rustfmt, "--check", "--edition", "2024", "--config",
                                   "skip_children=true", "tools/bonsai/src/cli.rs",
                                   "tools/bonsai/src/main.rs", "tools/bonsai/src/lib.rs"])
        rumdl = Path(os.environ["USERPROFILE"]) / ".cargo/bin/rumdl.exe"
        docs = ["README.md", "PLAN.md", "docs/README.md", "docs/chat-packages.md",
                "docs/releases.md", "docs/releases/v0.1.0.md"]
        check("markdown-check", [rumdl, "check", "--no-cache", *docs])
        check("markdown-format", [rumdl, "fmt", "--check", "--no-cache", *docs])
        check("diff-check", [git, "diff", "--check", "0621600b", "HEAD"])
        paths = [root / ".github/workflows/release.yml", installer_xml]
        paths.extend((root / ".github/release").glob("*.ps1"))
        paths.extend((root / "scripts/release").glob("*.ps1"))
        paths.extend(root / path for path in docs)
        paths.extend(root / f"tools/bonsai/src/{name}.rs" for name in ("cli", "main", "lib"))
        receipt["sourceSha256"] = {str(path.relative_to(root)).replace("\\", "/"):
                                   hashlib.sha256(path.read_bytes()).hexdigest()
                                   for path in sorted(paths)}
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
    finally:
        receipt["powerAfter"] = power()
        receipt["continuousPowerStabilityEstablished"] = False
        receipt["allStaticChecksPassed"] = ("failure" not in receipt and
                                              all(item["passed"] for item in receipt["checks"]))
        receipt["retainedScratchDirectory"] = str(scratch)
        receipt["closed"] = True
        receipt["elapsedSecondsThroughRetention"] = round(time.monotonic() - started, 6)
        receipt["withinAllowance"] = receipt["elapsedSecondsThroughRetention"] < plan["maximumAggregateSeconds"]
        summary_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({key: receipt[key] for key in ("allStaticChecksPassed", "elapsedSecondsThroughRetention", "powerBefore", "powerAfter")}))
    return 0 if receipt["allStaticChecksPassed"] else 1


if __name__ == "__main__":
    sys.exit(main())
