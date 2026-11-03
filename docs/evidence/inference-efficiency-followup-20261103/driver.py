"""One bounded offline check of reset and attention source changes; no model or CI."""
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

START = time.monotonic()
OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[3] / "efficiency2-integration"
TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\bin")
SOURCES = ["src/graph.rs", "tests/graph.rs", "src/kernels/pinned_attention.rs",
           "src/kernels/pinned_attention_padding_tests.rs", "Cargo.toml", "Cargo.lock"]


class Status(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("seconds", ctypes.c_ulong), ("full_seconds", ctypes.c_ulong)]


def power():
    result = {"observedAtUtc": datetime.datetime.now(datetime.UTC).isoformat(),
              "acConnected": None, "batterySaver": None, "chargePercent": None}
    status = Status()
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        result.update(acConnected={0: False, 1: True}.get(status.ac),
                      batterySaver={0: False, 1: True}.get(status.saver),
                      chargePercent=status.charge if status.charge <= 100 else None,
                      rawAcLineStatus=status.ac, rawBatteryFlag=status.flags)
    return result


REPORT = {"schemaVersion": 1, "kind": "bounded-offline-inference-source-validation",
          "startedAtUtc": datetime.datetime.now(datetime.UTC).isoformat(),
          "earlierChargeSeconds": 5, "maximumTotalSeconds": 300,
          "workCutoffSeconds": 260, "powerBefore": power(), "checks": [],
          "sourceRevisionBeforeChecks": (OUT / "source-revision.txt").read_text().strip(),
          "modelExecutions": 0, "ciActions": 0, "fullSuites": 0, "benchmarks": 0,
          "performanceComparisonQualified": False,
          "powerScope": "Endpoint observations only; no uninterrupted-stability claim."}
ENV = os.environ.copy()
ENV.update(CARGO_TARGET_DIR=str(Path(os.environ["TEMP"]) / "inference-efficiency-local-20261103" / "target"),
           CARGO_BUILD_JOBS="4", CARGO_NET_OFFLINE="true", RUSTC=str(TOOLCHAIN / "rustc.exe"),
           PATH=str(TOOLCHAIN) + os.pathsep + ENV["PATH"])
CARGO = TOOLCHAIN / "cargo.exe"
COMMON = ["--locked", "--offline", "--release", "-p", "prism-inference", "--features", "parallel-rows"]


def retain():
    REPORT["chargedSeconds"] = time.monotonic() - START + 5
    (OUT / "receipt.json").write_text(json.dumps(REPORT, indent=2), encoding="utf-8")


def run(name, args, maximum):
    remaining = 260 - (time.monotonic() - START)
    if remaining < 3:
        REPORT["checks"].append({"name": name, "status": "pending-budget"})
        retain()
        return
    record = {"name": name, "arguments": list(map(str, args)), "powerBefore": power(),
              "sourceSha256": {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
                               for path in SOURCES}}
    REPORT["checks"].append(record)
    begin = time.monotonic()
    try:
        with (OUT / (name + ".log")).open("wb") as log:
            proc = subprocess.Popen(list(map(str, args)), cwd=ROOT, env=ENV,
                                    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                    creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                code = proc.wait(timeout=min(maximum, remaining))
                record.update(exitCode=code, status="passed" if code == 0 else "failed")
            except subprocess.TimeoutExpired:
                record["status"] = "pending-timeout"
                subprocess.run([r"C:\Windows\System32\taskkill.exe", "/PID", str(proc.pid), "/T", "/F"],
                               stdout=log, stderr=log, timeout=8, check=False,
                               creationflags=subprocess.CREATE_NO_WINDOW)
                proc.wait(timeout=5)
    except (OSError, subprocess.SubprocessError) as error:
        record.update(status="failed", error=type(error).__name__)
    finally:
        record.update(elapsedSeconds=time.monotonic() - begin, powerAfter=power())
        retain()
    print(json.dumps({"check": name, "status": record["status"],
                      "seconds": round(record["elapsedSeconds"], 3)}), flush=True)


try:
    run("format", [TOOLCHAIN / "rustfmt.exe", "--check", "--edition", "2024", *SOURCES[:4]], 10)
    run("attention-controls", [CARGO, "test", *COMMON, "--lib", "kernels::pinned_attention"], 130)
    run("reset-controls", [CARGO, "test", *COMMON, "--test", "graph", "--",
        "reset_", "empty_checkpoint_", "hybrid_state_checkpoint_restores_exact_recurrent_prefix",
        "rank_one_checkpoint_and_failed_generation_preserve_complete_state"], 60)
    run("strict-release-clippy", [CARGO, "clippy", *COMMON, "--lib", "--test", "graph",
        "--", "-D", "warnings"], 45)
    if any(check["status"] != "passed" for check in REPORT["checks"]):
        handled = set()
        while time.monotonic() - START < 260 and not (OUT / "finish").exists():
            followup = OUT / "followup.json"
            if followup.exists():
                plan = json.loads(followup.read_text(encoding="utf-8-sig"))
                if plan["id"] not in handled:
                    handled.add(plan["id"])
                    for step in plan["steps"]:
                        run(step["name"], step["arguments"], step.get("maximumSeconds", 30))
            time.sleep(0.2)
finally:
    REPORT["powerAfter"] = power()
    retain()
    artifacts = [{"file": path.name, "bytes": path.stat().st_size,
                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                 for path in sorted(OUT.iterdir()) if path.suffix in (".log", ".json", ".py", ".txt")
                 and path.name not in ("archive.json", "followup.json")]
    charged = time.monotonic() - START + 5
    archive = {"schemaVersion": 1, "chargedSecondsThroughArchival": charged,
               "withinFiveMinuteAllowance": charged < 300, "earliestClockPreserved": True,
               "modelExecutions": 0, "benchmarks": 0, "ciActions": 0,
               "performanceComparisonQualified": False, "artifacts": artifacts}
    (OUT / "archive.json").write_text(json.dumps(archive, indent=2), encoding="utf-8")
    print(json.dumps({"chargedSeconds": charged, "receipt": str(OUT / "receipt.json")}), flush=True)
