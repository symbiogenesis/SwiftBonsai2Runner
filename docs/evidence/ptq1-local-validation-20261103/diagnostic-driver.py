"""One bounded, model-free diagnostic session; no CI or network operations."""
import ctypes
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\ptq1-archive-20261103")
OUT = Path(os.environ["TEMP"]) / "ptq1-local-validation-20261103"
TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-02-aarch64-pc-windows-msvc\bin")
START = time.monotonic()
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

REPORT = {"schemaVersion": 1, "kind": "bounded-model-free-ptq1-local-validation",
          "startedAtUtc": datetime.datetime.now(datetime.UTC).isoformat(),
          "earlierChargeSeconds": 10, "maximumTotalSeconds": 300,
          "workCutoffSeconds": 270, "powerBefore": power(), "checks": [],
          "modelExecutions": 0, "ciActions": 0, "fullSuites": 0,
          "defaultPromotionQualified": False}
ENV = os.environ.copy()
ENV.update(CARGO_TARGET_DIR=str(OUT / "target"), CARGO_BUILD_JOBS="4",
           CARGO_NET_OFFLINE="true", RUSTC=str(TOOLCHAIN / "rustc.exe"),
           PATH=str(TOOLCHAIN) + os.pathsep + ENV["PATH"])

def retain():
    REPORT["elapsedSeconds"] = time.monotonic() - START
    REPORT["chargedSeconds"] = REPORT["elapsedSeconds"] + 10
    (OUT / "receipt.json").write_text(json.dumps(REPORT, indent=2), encoding="utf-8")

def run(name, args, maximum=100):
    remaining = 270 - (time.monotonic() - START)
    if remaining < 3:
        REPORT["checks"].append({"name": name, "status": "pending-budget"})
        retain()
        return False
    record = {"name": name, "arguments": list(map(str, args)), "powerBefore": power()}
    REPORT["checks"].append(record)
    begin = time.monotonic()
    proc = None
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
    cargo = TOOLCHAIN / "cargo.exe"
    run("public-reference-replay", [os.sys.executable, "-B", "tools/native-reference/test_audit_public_reference.py"], 15)
    run("format", [TOOLCHAIN / "rustfmt.exe", "--check", "--edition", "2024",
                    "tools/bonsai/src/evidence_archive.rs", "src/broker/tests/native_model.rs",
                    "src/broker/tests/native_model/adapter_boundaries.rs",
                    "src/broker/tests/native_model/service_integration.rs"], 15)
    archive = run("archive-controls", [cargo, "test", "--locked", "--offline", "--release",
                  "-p", "bonsai-tools", "--lib", "evidence_archive::tests::", "--", "--test-threads=1"], 130)
    # The next focused named cases share one compilation and this original clock.
    broker = run("service-stream-failure", [cargo, "test", "--locked", "--offline", "--release",
                 "-p", "bonsai-broker", "--test", "native_model",
                 "native_adapter_stream_failures_retain_bound_requests_and_pending_response", "--", "--exact"], 85)
    if broker:
        binaries = list((OUT / "target/release/deps").glob("native_model-*.exe"))
        if len(binaries) != 1:
            raise RuntimeError("Expected one source-bound native_model test binary")
        for name, case in [
            ("service-boundary-failure", "adapter_boundaries::native_adapter_boundary_wire_sequence_preserves_rejections_and_failed_replay"),
            ("service-prefix-terminal", "service_integration::native_service_integration_prefix_rejects_terminal_events_already_received"),
            ("service-cancellation-failure", "service_integration::native_service_integration_cancellation_failures_return_without_unwinding")]:
            run(name, [binaries[0], case, "--exact"], 10)
    if archive:
        run("archive-release-clippy", [cargo, "clippy", "--locked", "--offline", "--release",
            "-p", "bonsai-tools", "--lib", "--", "-D", "warnings"], 60)
    if broker:
        run("service-release-clippy", [cargo, "clippy", "--locked", "--offline", "--release",
            "-p", "bonsai-broker", "--test", "native_model", "--", "-D", "warnings"], 45)
    print("Initial focused checks finished; original budget remains running for any necessary fixes.", flush=True)
    handled = set()
    while time.monotonic() - START < 267 and not (OUT / "finish").exists():
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
    REPORT["allRequestedChecksPassed"] = all(check["status"] == "passed" for check in REPORT["checks"])
    REPORT["performanceComparisonQualified"] = False
    REPORT["powerScope"] = "Endpoint observations only; no uninterrupted-stability or performance claim."
    retain()
    print(str(OUT / "receipt.json"), flush=True)
