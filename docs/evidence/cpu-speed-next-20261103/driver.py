"""Single bounded source check; an existing receipt permanently closes this run."""

import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


START = time.monotonic()
OUT = Path(__file__).resolve().parent
PLAN = json.loads((OUT / "workload-plan.json").read_text(encoding="utf-8-sig"))
STEPS = json.loads((OUT / "checks.json").read_text(encoding="utf-8-sig"))
with (OUT / "started.json").open("x", encoding="utf-8") as marker:
    json.dump({"startedAtUtc": datetime.datetime.now(datetime.UTC).isoformat()}, marker)


class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("life", ctypes.c_uint32), ("full_life", ctypes.c_uint32)]


def power():
    status = Power()
    result = {"observedAtUtc": datetime.datetime.now(datetime.UTC).isoformat(),
              "acConnected": None, "batterySaver": None, "chargePercent": None}
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        result.update(acConnected={0: False, 1: True}.get(status.ac),
                      batterySaver={0: False, 1: True}.get(status.saver),
                      chargePercent=status.charge if status.charge <= 100 else None,
                      rawAcLineStatus=status.ac, rawBatteryFlag=status.flags)
    return result


def hashes(step):
    root = Path(step["root"])
    paths = {path for pattern in step.get("sources", [])
             for path in root.glob(pattern) if path.is_file()}
    return {str(path.relative_to(root)).replace("\\", "/"):
            hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(paths)}


REPORT = {"schemaVersion": 1, "kind": PLAN["kind"], "powerBefore": power(),
          "maximumTotalSeconds": PLAN["maximumTotalSeconds"],
          "workCutoffSeconds": PLAN["workCutoffSeconds"], "checks": [],
          "modelExecutions": 0, "benchmarks": 0, "fullSuites": 0,
          "ciActions": 0, "pushes": 0, "performanceComparisonQualified": False}


def retain():
    REPORT["elapsedSeconds"] = time.monotonic() - START
    (OUT / "receipt.json").write_text(json.dumps(REPORT, indent=2) + "\n", encoding="utf-8")


def run(step):
    remaining = PLAN["workCutoffSeconds"] - (time.monotonic() - START)
    if remaining < step.get("minimumRemainingSeconds", 5):
        REPORT["checks"].append({"name": step["name"], "status": "pending-budget"})
        return False
    env = os.environ.copy()
    env.update(step.get("environment", {}))
    env.update(CARGO_NET_OFFLINE="true", CARGO_TERM_COLOR="never",
               CARGO_UNSTABLE_CHECKSUM_FRESHNESS="true", CARGO_BUILD_FINGERPRINT="content",
               CARGO_BUILD_JOBS="4")
    if compiler := step.get("compiler"):
        env.update(PATH=compiler + os.pathsep + env["PATH"],
                   RUSTC=str(Path(compiler) / "rustc.exe"),
                   RUSTDOC=str(Path(compiler) / "rustdoc.exe"))
    entry = {"name": step["name"], "arguments": step["arguments"],
             "sourceSha256Before": hashes(step), "powerBefore": power(), "status": "failed"}
    REPORT["checks"].append(entry)
    begin = time.monotonic()
    log_path = OUT / (step["name"] + ".log")
    try:
        with log_path.open("wb") as log:
            child = subprocess.Popen(step["arguments"], cwd=step.get("cwd", step["root"]),
                                     env=env, stdin=subprocess.DEVNULL,
                                     stdout=log, stderr=subprocess.STDOUT,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                # Setup and source hashing consume the same original allowance.
                remaining = PLAN["workCutoffSeconds"] - (time.monotonic() - START)
                code = child.wait(timeout=max(0.01, min(step["maximumSeconds"], remaining)))
                entry.update(exitCode=code, status="passed" if code == 0 else "failed")
            except subprocess.TimeoutExpired:
                entry["status"] = "pending-timeout"
                stopped = subprocess.run(
                    [r"C:\Windows\System32\taskkill.exe", "/PID", str(child.pid), "/T", "/F"],
                    stdout=log, stderr=log, timeout=10, check=False,
                    creationflags=subprocess.CREATE_NO_WINDOW)
                entry["cleanupExitCode"] = stopped.returncode
                child.wait(timeout=5)
    except (OSError, subprocess.SubprocessError) as error:
        entry.update(status="failed", error=type(error).__name__)
    if "requiredPassedTests" in step:
        output = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
        passed = re.findall(r"^test (.+) \.\.\. ok$", output, re.MULTILINE)
        entry["passedTests"] = passed
        entry["missingRequiredTests"] = sorted(set(step["requiredPassedTests"]) - set(passed))
        if entry["missingRequiredTests"]:
            entry["status"] = "failed"
    entry.update(elapsedSeconds=time.monotonic() - begin, powerAfter=power(),
                 sourceSha256After=hashes(step))
    if step.get("immutableSource", True) and entry["sourceSha256Before"] != entry["sourceSha256After"]:
        entry.update(status="failed", error="source-changed-during-check")
    retain()
    print(json.dumps({"check": step["name"], "status": entry["status"],
                      "seconds": round(entry["elapsedSeconds"], 3)}), flush=True)
    return entry["status"] == "passed"


try:
    for step in STEPS:
        if not run(step):
            break
finally:
    REPORT["powerAfter"] = power()
    REPORT["clockClosed"] = True
    REPORT["clockRestarted"] = False
    retain()
    artifacts = [{"file": path.name, "bytes": path.stat().st_size,
                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                 for path in sorted(OUT.iterdir()) if path.suffix in (".log", ".json", ".py")
                 and path.name != "archive.json"]
    archive = {"schemaVersion": 1, "artifacts": artifacts,
               "powerAfterRetention": power(),
               "elapsedSecondsThroughRetention": time.monotonic() - START,
               "withinDeclaredAllowance": time.monotonic() - START < PLAN["maximumTotalSeconds"]}
    (OUT / "archive.json").write_text(json.dumps(archive, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"clockClosed": True, "seconds": time.monotonic() - START}), flush=True)
