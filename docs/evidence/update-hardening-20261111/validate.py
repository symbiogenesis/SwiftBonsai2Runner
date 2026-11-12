"""One fixed, model-free local check packet; never retries or starts CI."""

import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent
RAW = EVIDENCE / "raw"
GIT = Path.home() / "AppData/Local/Microsoft/WinGet/Packages/Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe/cmd/git.exe"
CARGO = Path.home() / ".cargo/bin/cargo.exe"
RUMDL = Path.home() / ".cargo/bin/rumdl.exe"
SYSTEM = Path(os.environ["SystemRoot"]) / "System32"
TARGET = Path(os.environ["TEMP"]) / "Bonsai-UpdateCheck-7d0gpwmw/target"


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def power():
    class Status(ctypes.Structure):
        _fields_ = [
            ("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
            ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
            ("life", ctypes.c_uint32), ("full", ctypes.c_uint32),
        ]

    result = {"observedUtc": utc(), "source": "GetSystemPowerStatus",
              "acConnected": None, "batterySaver": None, "chargePercent": None}
    status = Status()
    try:
        if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
            result.update(acConnected={0: False, 1: True}.get(status.ac),
                          batterySaver={0: False, 1: True}.get(status.saver),
                          chargePercent=status.charge if status.charge <= 100 else None)
    except (AttributeError, OSError) as error:
        result["error"] = str(error)
    return result


def main():
    if (EVIDENCE / "summary.json").exists():
        raise SystemExit("This workload's clock is closed; do not rerun it.")
    started = time.monotonic()
    receipt = {
        "schemaVersion": 1, "kind": "updater-hardening-offline-source-validation",
        "startedUtc": utc(), "maximumAggregateSeconds": 300,
        "powerBefore": power(), "checks": [], "comparisonArms": [],
        "continuousPowerStabilityEstablished": False, "performanceQualified": False,
        "completeSuitesRun": False, "msiExecuted": False, "releaseApiRequests": False,
        "gitPushes": 0, "ciRunsDispatched": 0, "existingInstalledExecutablesReplaced": False,
        "powershellRuntimeChecks": "pending: PowerShell 7 unavailable locally",
        "buildCache": str(TARGET),
    }
    RAW.mkdir(exist_ok=True)
    env = os.environ.copy()
    env["PATH"] = str(GIT.parent) + os.pathsep + str(CARGO.parent) + os.pathsep + env["PATH"]
    env["CARGO_BUILD_JOBS"] = "4"
    env["CARGO_TARGET_DIR"] = str(TARGET)

    def run(name, argv, maximum):
        allowance = min(maximum, 300 - (time.monotonic() - started) - 25)
        if allowance < 2:
            item = {"name": name, "skipped": "cannot fit original allowance", "passed": False}
            receipt["checks"].append(item)
            return False
        began = time.monotonic()
        item = {"name": name, "command": [str(part) for part in argv], "timedOut": False}
        with (RAW / (name + ".log")).open("wb") as log:
            child = subprocess.Popen(item["command"], cwd=ROOT, env=env,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
            try:
                child.wait(timeout=allowance)
            except subprocess.TimeoutExpired:
                item["timedOut"] = True
                stopped = subprocess.run([str(SYSTEM / "taskkill.exe"), "/PID", str(child.pid), "/T", "/F"],
                                         stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                         timeout=5, check=False)
                item["treeStopExitCode"] = stopped.returncode
                child.wait(timeout=5)
        item.update(exitCode=child.returncode, elapsedSeconds=round(time.monotonic() - began, 3),
                    passed=child.returncode == 0 and not item["timedOut"])
        receipt["checks"].append(item)
        print(json.dumps(item), flush=True)
        return item["passed"]

    try:
        run("source-revision", [GIT, "rev-parse", "HEAD"], 3)
        receipt["sourceRevision"] = (RAW / "source-revision.log").read_text().strip()
        run("source-status", [GIT, "status", "--short"], 3)
        run("rust-format", [CARGO, "fmt", "--all"], 10)
        run("rust-format-check", [CARGO, "fmt", "--all", "--check"], 10)
        source_files = [ROOT / "tools/bonsai/src/app_update.rs", ROOT / "tools/bonsai/src/chat_launch.rs",
                        ROOT / "src/chat-ui/src/window/updates.rs", ROOT / ".github/workflows/release.yml",
                        ROOT / "scripts/Start-Chat.ps1"]
        source_files += list((ROOT / "tools/bonsai/src/app_update").rglob("*.rs"))
        source_files += list((ROOT / "scripts/release").glob("*.ps1"))
        source_files += list((ROOT / ".github/release").glob("*.ps1"))
        receipt["sourceSha256"] = {
            path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(source_files)
        }
        parser = RAW / "parse-sources.ps1"
        parser.write_text(r'''param([string]$Root)
$ErrorActionPreference = 'Stop'
$files = @((Join-Path $Root 'scripts/Start-Chat.ps1')) +
    @((Get-ChildItem -LiteralPath (Join-Path $Root 'scripts/release') -Filter '*.ps1' -File).FullName) +
    @((Get-ChildItem -LiteralPath (Join-Path $Root '.github/release') -Filter '*.ps1' -File).FullName)
foreach ($file in $files) {
    $tokens = $null
    $issues = $null
    [void][Management.Automation.Language.Parser]::ParseFile($file, [ref]$tokens, [ref]$issues)
    if ($issues.Count -gt 0) { throw "PowerShell parse failed: $file`n$issues" }
}
Write-Output "Parsed $($files.Count) sources with Windows PowerShell AST; PS7 runtime behavior remains pending."
''', encoding="utf-8")
        run("powershell-ast", [SYSTEM / "WindowsPowerShell/v1.0/powershell.exe", "-NoLogo", "-NoProfile",
                               "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", parser, "-Root", ROOT], 10)
        docs = ["PLAN.md", "docs/releases.md", "docs/ui-architecture.md"]
        run("markdown-check", [RUMDL, "check", *docs], 5)
        run("markdown-format", [RUMDL, "fmt", "--check", *docs], 5)
        run("whitespace", [GIT, "diff", "--check", "60beb0f6"], 3)
        cargo = [CARGO, "test", "--offline", "--locked", "--release", "--package", "bonsai-tools", "--lib"]
        core_ok = run("updater-fixtures", [*cargo, "app_update::"], 100)
        if core_ok:
            run("acknowledged-handoff", [*cargo, "chat_launch::tests::installer_handoff_requires_acknowledged_update_exit_and_ignores_ordinary_close", "--", "--exact"], 15)
        run("native-ui-update-fixtures", [CARGO, "test", "--offline", "--locked", "--release", "--package", "bonsai-chat", "window::updates::tests::"], 80)
        run("native-ui-check", [CARGO, "check", "--offline", "--locked", "--release", "--package", "bonsai-chat"], 50)
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
        print(receipt["failure"], flush=True)
    finally:
        receipt["powerAfter"] = power()
        receipt["closed"] = True
        receipt["allSelectedChecksPassed"] = "failure" not in receipt and all(item["passed"] for item in receipt["checks"])
        receipt["elapsedSecondsThroughRetention"] = round(time.monotonic() - started, 3)
        receipt["withinAllowance"] = receipt["elapsedSecondsThroughRetention"] <= 300
        (EVIDENCE / "summary.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({key: receipt[key] for key in ["elapsedSecondsThroughRetention", "withinAllowance", "allSelectedChecksPassed", "powerAfter"]}), flush=True)


if __name__ == "__main__":
    main()
