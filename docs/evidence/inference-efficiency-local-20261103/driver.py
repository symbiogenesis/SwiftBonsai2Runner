"""One bounded offline source-validation session; no model, benchmark or CI."""
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

START = time.monotonic()
ROOT = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\speed-integration")
OUT = Path(os.environ["TEMP"]) / "inference-efficiency-local-20261103"
TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\bin")
OUT.mkdir(exist_ok=False)


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
          "workCutoffSeconds": 265, "powerBefore": power(), "checks": [],
          "sourceRevisionBeforeChecks": "ba732ff", "modelExecutions": 0,
          "ciActions": 0, "fullSuites": 0, "benchmarks": 0,
          "performanceComparisonQualified": False,
          "powerScope": "Endpoint observations only; no uninterrupted-stability claim."}
ENV = os.environ.copy()
ENV.update(CARGO_TARGET_DIR=str(OUT / "target"), CARGO_BUILD_JOBS="4",
           CARGO_NET_OFFLINE="true", RUSTC=str(TOOLCHAIN / "rustc.exe"),
           PATH=str(TOOLCHAIN) + os.pathsep + ENV["PATH"])
CARGO = TOOLCHAIN / "cargo.exe"
COMMON = ["--locked", "--offline", "--release", "-p", "prism-inference", "--features", "parallel-rows"]
SOURCES = ["src/kernels.rs", "src/graph.rs", "src/sampling/swift.rs",
           "src/sampling/upstream.rs", "tests/graph.rs", "Cargo.toml", "Cargo.lock"]


def retain():
    REPORT["elapsedSeconds"] = time.monotonic() - START
    REPORT["chargedSeconds"] = REPORT["elapsedSeconds"] + 5
    (OUT / "receipt.json").write_text(json.dumps(REPORT, indent=2), encoding="utf-8")


def run(name, args, maximum=80):
    remaining = 265 - (time.monotonic() - START)
    if remaining < 3:
        REPORT["checks"].append({"name": name, "status": "pending-budget"})
        retain()
        return False
    record = {"name": name, "arguments": list(map(str, args)), "powerBefore": power(),
              "sourceSha256": {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
                               for path in SOURCES}}
    REPORT["checks"].append(record)
    begin = time.monotonic()
    try:
        with (OUT / (name + ".log")).open("wb") as log:
            proc = subprocess.Popen(list(map(str, args)), cwd=ROOT, env=ENV,
                                    stdin=subprocess.DEVNULL, stdout=log,
                                    stderr=subprocess.STDOUT,
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
    return record["status"] == "passed"


try:
    run("format", [TOOLCHAIN / "rustfmt.exe", "--check", "--edition", "2024", *SOURCES[:5]], 10)
    run("kernel-sampler-controls", [CARGO, "test", *COMMON, "--lib", "--",
        "kernels::tests::scalar_column_projection_",
        "kernels::tests::matvec_scratch_retains_quantized_and_floating_storage",
        "rank_one_products_round_before_scale_and_base_addition",
        "sampling::swift::tests::"], 145)
    run("checkpoint-controls", [CARGO, "test", *COMMON, "--test", "graph", "--",
        "empty_checkpoint_", "rank_one_checkpoint_and_failed_generation_preserve_complete_state",
        "hybrid_state_checkpoint_restores_exact_recurrent_prefix"], 35)
    run("native-sampler-fixture", [CARGO, "test", *COMMON, "--test", "upstream_sampling",
        "swift_chain_matches_verified_pinned_arm64_candidates_and_seeded_draws", "--", "--exact"], 25)
    run("sampler-rollback", [CARGO, "test", *COMMON, "--test", "swift_sampling",
        "swift_rng_rolls_back_after_cancelled_or_abandoned_stream_then_retries", "--", "--exact"], 25)
    run("release-clippy", [CARGO, "clippy", *COMMON, "--lib", "--test", "graph",
        "--test", "upstream_sampling", "--test", "swift_sampling", "--", "-D", "warnings"], 50)
    if any(check["status"] != "passed" for check in REPORT["checks"]):
        print("Corrections may use only this original remaining allowance.", flush=True)
        handled = set()
        while time.monotonic() - START < 265 and not (OUT / "finish").exists():
            followup = OUT / "followup.json"
            if followup.exists():
                plan = json.loads(followup.read_text(encoding="utf-8"))
                if plan["id"] not in handled:
                    handled.add(plan["id"])
                    for step in plan["steps"]:
                        run(step["name"], step["arguments"], step.get("maximumSeconds", 45))
            time.sleep(0.2)
finally:
    REPORT["powerAfter"] = power()
    REPORT["allInitialChecksPassed"] = all(check["status"] == "passed" for check in REPORT["checks"][:6])
    retain()
    print(str(OUT / "receipt.json"), flush=True)
