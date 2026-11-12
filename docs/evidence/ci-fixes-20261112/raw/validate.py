import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner")
EVIDENCE = ROOT / "docs/evidence/ci-fixes-20261112"
RAW = EVIDENCE / "raw"
TARGET = Path(os.environ["TEMP"]) / "Bonsai-UpdateCheck-7d0gpwmw/target"
SOURCE_PATHS = [
    "docs/performance-expectations.md", "tools/native-reference/pins.json",
    "src/broker/tests/native_model/progress.rs",
    "src/broker/tests/native_model/token_timing.rs", "PLAN.md",
]

class PowerStatus(ctypes.Structure):
    _fields_ = [("ac", wintypes.BYTE), ("flags", wintypes.BYTE),
                ("charge", wintypes.BYTE), ("saver", wintypes.BYTE),
                ("life", wintypes.DWORD), ("full_life", wintypes.DWORD)]

def utc():
    return datetime.now(timezone.utc).isoformat()

def power():
    status = PowerStatus()
    available = bool(ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)))
    return {
        "observedUtc": utc(), "source": "GetSystemPowerStatus",
        "available": available,
        "acConnected": bool(status.ac) if available and status.ac in (0, 1) else None,
        "batterySaver": bool(status.saver) if available and status.saver in (0, 1) else None,
        "chargePercent": status.charge if available and status.charge <= 100 else None,
    }

def main():
    started = time.monotonic()
    RAW.mkdir(parents=True, exist_ok=True)
    receipt = {"schemaVersion": 1, "kind": "ci-fix-focused-local-validation",
               "startedUtc": utc(), "maximumAggregateSeconds": 300,
               "powerBefore": power(), "checks": [], "retries": 0,
               "completeSuites": False, "modelOrBenchmarkWork": False,
               "continuousPowerStabilityEstablished": False,
               "performanceQualified": False, "buildCache": str(TARGET)}
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(TARGET), CARGO_BUILD_JOBS="4")
    env.pop("GITHUB_ACTIONS", None)
    env.pop("BONSAI_NATIVE_MODEL_ACCEPTANCE", None)
    git = shutil.which("git")
    cargo = shutil.which("cargo")
    rumdl = shutil.which("rumdl")
    steps = [
        ("source-revision", [git, "rev-parse", "HEAD"], 3, ROOT),
        ("rumdl-version", [rumdl, "--version"], 3, ROOT),
        ("markdown-check", [rumdl, "check", "--no-cache", "."], 7, ROOT),
        ("markdown-format", [rumdl, "fmt", "--check", "--no-cache", "."], 7, ROOT),
        ("rust-format", [cargo, "fmt", "--all", "--check"], 7, ROOT),
        ("whitespace", [git, "diff", "--check", "e03ec67d"], 3, ROOT),
        ("strict-clippy", [cargo, "clippy", "--offline", "--locked", "--release",
                           "--manifest-path", str(ROOT / "Cargo.toml"),
                           "--package", "bonsai-broker", "--package", "bonsai-tools",
                           "--package", "bonsai-chat", "--all-targets", "--all-features"],
         150, Path(os.environ["TEMP"])),
        ("qualification-regressions", [cargo, "test", "--offline", "--locked", "--release",
                                       "--manifest-path", str(ROOT / "Cargo.toml"),
                                       "--package", "bonsai-tools", "--lib",
                                       "native_qualification::tests::"],
         80, Path(os.environ["TEMP"])),
    ]
    try:
        for name, command, limit, cwd in steps:
            remaining = 270 - (time.monotonic() - started)
            if remaining < limit:
                receipt["checks"].append({"name": name, "status": "not-started",
                                           "reason": "Cannot fit fixed command limit and cleanup reserve."})
                print(name + ": not started (aggregate reserve)", flush=True)
                continue
            begin = time.monotonic()
            result = {"name": name, "command": command, "timeoutSeconds": limit,
                      "workingDirectory": str(cwd), "timedOut": False}
            with (RAW / (name + ".stdout.log")).open("wb") as out, (RAW / (name + ".stderr.log")).open("wb") as err:
                process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err)
                try:
                    result["exitCode"] = process.wait(timeout=limit)
                except subprocess.TimeoutExpired:
                    result["timedOut"] = True
                    cleanup = subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=10)
                    (RAW / (name + ".cleanup.log")).write_bytes(cleanup.stdout)
                    result["cleanupExitCode"] = cleanup.returncode
                    result["exitCode"] = process.wait(timeout=5)
            result["elapsedSeconds"] = round(time.monotonic() - begin, 3)
            result["passed"] = result["exitCode"] == 0 and not result["timedOut"]
            receipt["checks"].append(result)
            if name == "source-revision":
                receipt["sourceRevision"] = (RAW / (name + ".stdout.log")).read_text().strip()
            print(name + ": " + json.dumps(result), flush=True)
    finally:
        receipt["sourceSha256"] = {
            path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in SOURCE_PATHS
        }
        receipt["powerAfter"] = power()
        receipt["completedUtc"] = utc()
        receipt["elapsedAggregateSecondsThroughRetention"] = round(time.monotonic() - started, 3)
        receipt["clockClosed"] = True
        (EVIDENCE / "summary.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print("SUMMARY " + json.dumps({key: receipt[key] for key in (
            "powerBefore", "powerAfter", "elapsedAggregateSecondsThroughRetention", "clockClosed")}), flush=True)

if __name__ == "__main__":
    main()
