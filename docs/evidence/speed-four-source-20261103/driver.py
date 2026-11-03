"""One fixed, still-open source-development check; never runs models or CI."""

import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


OUT = Path(__file__).resolve().parent
PLAN = json.loads((OUT / "workload-plan.json").read_text(encoding="utf-8-sig"))
STEPS = json.loads((OUT / "checks.json").read_text(encoding="utf-8-sig"))
START = time.monotonic()


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
    files = set()
    for pattern in step.get("sources", []):
        files.update(path for path in root.glob(pattern) if path.is_file())
    return {str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(files)}


REPORT = {"schemaVersion": 1, "kind": PLAN["kind"],
          "startedAtUtc": datetime.datetime.now(datetime.UTC).isoformat(),
          "maximumTotalSeconds": PLAN["maximumTotalSeconds"],
          "workCutoffSeconds": PLAN["workCutoffSeconds"], "powerBefore": power(),
          "checks": [], "modelExecutions": 0, "benchmarks": 0, "fullSuites": 0,
          "ciActions": 0, "pushes": 0, "performanceComparisonQualified": False}


def retain():
    REPORT["elapsedSeconds"] = time.monotonic() - START
    (OUT / "receipt.json").write_text(json.dumps(REPORT, indent=2) + "\n", encoding="utf-8")


def run(step, suffix=""):
    remaining = PLAN["workCutoffSeconds"] - (time.monotonic() - START)
    if remaining < 5:
        REPORT["checks"].append({"name": step["name"] + suffix, "status": "pending-budget"})
        return False
    env = os.environ.copy()
    env.update(step.get("environment", {}))
    env.update(CARGO_NET_OFFLINE="true", CARGO_TERM_COLOR="never", CARGO_UNSTABLE_CHECKSUM_FRESHNESS="true",
               CARGO_BUILD_FINGERPRINT="content", CARGO_BUILD_JOBS="4")
    compiler = step.get("compiler")
    if compiler:
        env.update(PATH=compiler + os.pathsep + env["PATH"],
                   RUSTC=str(Path(compiler) / "rustc.exe"),
                   RUSTDOC=str(Path(compiler) / "rustdoc.exe"))
    entry = {"name": step["name"] + suffix, "arguments": step["arguments"],
             "sourceSha256": hashes(step), "powerBefore": power(), "status": "failed"}
    REPORT["checks"].append(entry)
    begin = time.monotonic()
    try:
        with (OUT / (entry["name"] + ".log")).open("wb") as log:
            child = subprocess.Popen(step["arguments"], cwd=step.get("cwd", step["root"]), env=env,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                code = child.wait(timeout=min(step.get("maximumSeconds", remaining), remaining))
                entry.update(exitCode=code, status="passed" if code == 0 else "failed")
            except subprocess.TimeoutExpired:
                entry["status"] = "pending-timeout"
                subprocess.run([r"C:\Windows\System32\taskkill.exe", "/PID", str(child.pid), "/T", "/F"],
                               stdout=log, stderr=log, timeout=10, check=False,
                               creationflags=subprocess.CREATE_NO_WINDOW)
                child.wait(timeout=5)
    except (OSError, subprocess.SubprocessError) as error:
        entry.update(status="failed", error=type(error).__name__)
    if "requiredPassedTests" in step:
        output = (OUT / (entry["name"] + ".log")).read_text(encoding="utf-8", errors="replace")
        passed = re.findall(r"^test (.+) \.\.\. ok$", output, re.MULTILINE)
        entry["passedTests"] = passed
        entry["missingRequiredTests"] = sorted(set(step["requiredPassedTests"]) - set(passed))
        if entry["missingRequiredTests"]:
            entry["status"] = "failed"
    entry.update(elapsedSeconds=time.monotonic() - begin, powerAfter=power())
    retain()
    print(json.dumps({"check": entry["name"], "status": entry["status"],
                      "seconds": round(entry["elapsedSeconds"], 3),
                      "remaining": round(PLAN["workCutoffSeconds"] - (time.monotonic() - START), 3)}), flush=True)
    return entry["status"] == "passed"


try:
    correction_used = False
    for step in STEPS:
        if not run(step):
            if correction_used:
                break
            # A source correction is allowed only while this original session
            # stays open, once, and only for changed source. Never restart it.
            print(json.dumps({"awaitingSourceCorrection": step["name"]}), flush=True)
            repair_file = OUT / "source-correction.json"
            while time.monotonic() - START < PLAN["workCutoffSeconds"] and not repair_file.exists():
                if (OUT / "finish").exists():
                    break
                time.sleep(0.2)
            if not repair_file.exists():
                break
            repair = json.loads(repair_file.read_text(encoding="utf-8-sig"))
            if repair.get("failedStep") != step["name"] or hashes(step) == REPORT["checks"][-1]["sourceSha256"]:
                REPORT["correctionRejected"] = "Missing changed-source binding for the one correction pass"
                break
            REPORT["sourceCorrection"] = repair
            correction_used = True
            if not all(run(corrected, "-source-correction") for corrected in repair["checks"]):
                break
            repair_file.rename(OUT / "source-correction-consumed.json")
            # Continue still-unexecuted fixed checks without extending the clock.
finally:
    REPORT["powerAfter"] = power()
    REPORT["clockClosed"] = True
    retain()
    artifacts = [{"file": path.name, "bytes": path.stat().st_size,
                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                 for path in sorted(OUT.iterdir()) if path.suffix in (".log", ".json", ".py")
                 and path.name != "archive.json"]
    (OUT / "archive.json").write_text(json.dumps({
        "schemaVersion": 1, "elapsedSecondsThroughRetention": time.monotonic() - START,
        "withinDeclaredAllowance": time.monotonic() - START < PLAN["maximumTotalSeconds"],
        "originalClockPreserved": True, "artifacts": artifacts,
    }, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"clockClosed": True, "seconds": time.monotonic() - START}), flush=True)
