"""One fixed model-free implementation check; never retries a failed operation."""

import time

STARTED = time.monotonic()

import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-09-aarch64-pc-windows-msvc\bin")
GIT = Path(r"C:\Program Files\Microsoft Visual Studio\18\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe")
HERE = Path(__file__).resolve().parent
MAXIMUM = 540
WORK_CUTOFF = 485


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
    return {"source": "GetSystemPowerStatus", "observedUtc": datetime.now(timezone.utc).isoformat(),
            "acConnected": bool(status.ac) if known and status.ac in (0, 1) else None,
            "batterySaver": bool(status.saver) if known and status.saver in (0, 1) else None,
            "chargePercent": status.charge if known and status.charge <= 100 else None}


def digest(path, deadline=None):
    result = hashlib.sha256()
    with path.open("rb") as source:
        while True:
            if deadline is not None and time.monotonic() - STARTED >= deadline:
                raise RuntimeError("Original allowance exhausted while hashing; no retry")
            block = source.read(1024 * 1024)
            if not block:
                return result.hexdigest()
            result.update(block)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--retain", type=Path, required=True)
    args = parser.parse_args()
    evidence_ready = False
    receipt = {"schemaVersion": 1, "startedUtc": datetime.now(timezone.utc).isoformat(),
               "maximumTotalSeconds": MAXIMUM, "workCutoffSeconds": WORK_CUTOFF,
               "steps": [],
               "status": "failed", "modelsExecuted": 0, "benchmarksExecuted": 0,
               "fullSuitesExecuted": 0, "acceleratorDeviceCalls": 0,
               "ciActions": 0, "pushes": 0, "powerBefore": None}
    env = dict(os.environ)
    env["PATH"] = str(TOOLCHAIN) + os.pathsep + env["PATH"]
    env["CARGO_TARGET_DIR"] = str(args.evidence / "target")
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_BUILD_JOBS"] = "4"
    env["RUSTUP_TOOLCHAIN"] = "nightly-2026-11-09-aarch64-pc-windows-msvc"

    def save():
        if evidence_ready:
            (args.evidence / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def run(name, command, cap=20, minimum=1):
        remaining = WORK_CUTOFF - (time.monotonic() - STARTED)
        if remaining < minimum:
            raise RuntimeError(f"Insufficient remaining allowance for {name}; no retry")
        step = {"name": name, "arguments": [str(value) for value in command], "powerBefore": power()}
        receipt["steps"].append(step)
        begin = time.monotonic()
        log = args.evidence / f"{len(receipt['steps']):02d}-{name[:60]}.log"
        step["log"] = log.name
        code = None
        try:
            with log.open("wb") as output:
                process = subprocess.Popen(step["arguments"], cwd=args.source, env=env,
                                           stdout=output, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
                try:
                    code = process.wait(timeout=min(cap, remaining))
                except BaseException as error:
                    step["timedOut"] = isinstance(error, subprocess.TimeoutExpired)
                    step["cleanupRequired"] = True
                    step["cleanup"] = {"treeTerminationReported": False, "parentExited": False}
                    try:
                        killed = subprocess.run(["taskkill.exe", "/pid", str(process.pid), "/t", "/f"],
                            stdout=output, stderr=subprocess.STDOUT, timeout=10,
                            creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                        step["cleanup"]["taskkillExitCode"] = killed.returncode
                        step["cleanup"]["treeTerminationReported"] = killed.returncode == 0
                    except subprocess.TimeoutExpired:
                        step["cleanup"]["taskkillTimedOut"] = True
                    finally:
                        if process.poll() is None:
                            process.kill()
                        code = process.wait(timeout=5)
                        step["cleanup"]["parentExited"] = True
                    raise
        finally:
            step.update(exitCode=code, elapsedSeconds=time.monotonic() - begin, powerAfter=power())
            save()
        print(f"{name}: exit {code}, {step['elapsedSeconds']:.3f}s", flush=True)
        if code != 0:
            raise RuntimeError(f"{name} failed; operation closes without retry")
        return log.read_text(encoding="utf-8", errors="replace")

    def source_hashes(label):
        names = run(label, [GIT, "-c", f"safe.directory={args.source.as_posix()}", "ls-files"]).splitlines()
        return {name: digest(args.source / name, WORK_CUTOFF) for name in names if (args.source / name).is_file()}

    try:
        args.evidence.mkdir(parents=True, exist_ok=False)
        evidence_ready = True
        receipt["powerBefore"] = power()
        controls = json.loads((HERE / "controls.json").read_text(encoding="utf-8"))
        plan = json.loads((HERE / "workload-plan.json").read_text(encoding="utf-8"))
        identities = {path.name: digest(path, WORK_CUTOFF) for path in
                      [Path(__file__), HERE / "controls.json", HERE / "workload-plan.json"]}
        receipt.update(declarationSha256=identities, controls=controls)
        if (plan["budget"]["maximumTotalSeconds"] != MAXIMUM
                or plan["budget"]["workCutoffSeconds"] != WORK_CUTOFF):
            raise RuntimeError("Fixed declaration differs from driver budget")
        names = [name for selected in controls.values() for name in selected]
        if (set(controls) != set(plan["execution"]["testTargets"])
                or any(not selected for selected in controls.values())
                or len(names) != len(set(names))
                or len(names) != plan["execution"]["selectedRustControls"]):
            raise RuntimeError("Frozen control inventory differs from declaration")
        run("compiler", [TOOLCHAIN / "rustc.exe", "-vV"])
        receipt["sourceCommit"] = run("source-head", [GIT, "-c", f"safe.directory={args.source.as_posix()}", "rev-parse", "HEAD"]).strip()
        if receipt["sourceCommit"] != plan["source"]["prismSelectedRevision"]:
            raise RuntimeError("Selected source differs from declaration")
        run("format-check", [TOOLCHAIN / "cargo.exe", "fmt", "--all", "--", "--check"], cap=25)
        receipt["formattedSourceDiff"] = run("formatted-diff", [GIT, "-c", f"safe.directory={args.source.as_posix()}", "diff", "HEAD", "--no-ext-diff", "--no-color"])
        receipt["sourceSha256"] = source_hashes("initial-source-files")
        output = run("receipt-controls", [os.sys.executable, "-B", "-m", "unittest", "discover", "-s", str(args.source / ".github/ci"), "-p", "test_inference_efficiency.py", "-v"])
        if not re.search(r"Ran 6 tests in [0-9.]+s\s+OK\s*$", output):
            raise RuntimeError("Receipt controls missing or not passed")
        packages = ["-p", "prism-inference", "-p", "bonsai-hexagon-runtime"]
        targets = ["--lib", "--test", "graph", "--test", "tokenizer"]
        output = run("compile-controls", [TOOLCHAIN / "cargo.exe", "test", "--locked", "--offline", *packages, "--features", "prism-inference/parallel-rows", *targets, "--no-run", "--message-format=json"], cap=180, minimum=160)
        executables = {}
        for line in output.splitlines():
            if not line.startswith("{"):
                continue
            record = json.loads(line)
            if record.get("reason") == "compiler-artifact" and record.get("executable") and record.get("profile", {}).get("test"):
                target = record["target"]["name"]
                if target in controls:
                    if target in executables:
                        raise RuntimeError("Duplicate selected test artifact")
                    executables[target] = Path(record["executable"])
        if set(executables) != set(controls):
            raise RuntimeError("Missing selected test artifact")
        receipt["executableSha256"] = {target: digest(path, WORK_CUTOFF) for target, path in executables.items()}
        for target, selected in controls.items():
            for name in selected:
                output = run(name.replace("::", "-"), [executables[target], "--exact", name, "--nocapture"],
                             cap=plan["execution"]["controlCapsSeconds"].get(name, 20))
                lines = output.replace("\r\n", "\n").splitlines()
                if (lines.count("running 1 test") != 1 or lines.count(f"test {name} ... ok") != 1
                        or sum(bool(re.fullmatch(r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in .+", line)) for line in lines) != 1):
                    raise RuntimeError("Absent, ignored, failed or duplicate exact control")
        run("strict-release-analysis", [TOOLCHAIN / "cargo.exe", "clippy", "--locked", "--offline", "--release", *packages, "--features", "prism-inference/parallel-rows", *targets, "--", "-D", "warnings"], cap=125, minimum=100)
        run("production-worker-link", [TOOLCHAIN / "cargo.exe", "build", "--locked", "--offline", "--release", "-p", "prism-inference", "--bin", "bonsai-swift-worker", "--features", "parallel-rows"], cap=55, minimum=45)
        run("feature-disabled", [TOOLCHAIN / "cargo.exe", "check", "--locked", "--offline", "-p", "prism-inference", "--lib", "--no-default-features"], cap=30, minimum=20)
        if source_hashes("final-source-files") != receipt["sourceSha256"]:
            raise RuntimeError("Sources changed during checks")
        if any(digest(HERE / name, WORK_CUTOFF) != value for name, value in identities.items()):
            raise RuntimeError("Declaration, driver or inventory changed during checks")
        receipt["status"] = "passed"
    except Exception as error:
        receipt["failure"] = str(error)
    finally:
        receipt["cleanupIncomplete"] = any(step.get("cleanupRequired") and not (
            step.get("cleanup", {}).get("treeTerminationReported") and step.get("cleanup", {}).get("parentExited")) for step in receipt["steps"])
        if receipt["cleanupIncomplete"]:
            receipt["status"] = "failed"
        receipt.update(powerAfter=power(), elapsedSecondsThroughChecks=time.monotonic() - STARTED,
                       clockClosed=True, clockRestarted=False, retries=0,
                       continuousPowerStabilityEstablished=False, performanceComparisonQualified=False)
        save()
        args.retain.mkdir(parents=True, exist_ok=False)
        verified = evidence_ready
        retained_hashes = {}
        if not evidence_ready:
            (args.retain / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        for path in args.evidence.iterdir() if evidence_ready else []:
            if path.is_file():
                try:
                    if time.monotonic() - STARTED >= MAXIMUM - 3:
                        raise RuntimeError("Original retention allowance exhausted")
                    shutil.copyfile(path, args.retain / path.name)
                    original = digest(path, MAXIMUM - 2)
                    retained = digest(args.retain / path.name, MAXIMUM - 2)
                    retained_hashes[path.name] = retained
                    if original != retained:
                        raise RuntimeError("Retained source copy differs")
                except Exception as error:
                    verified = False
                    receipt["retentionFailure"] = str(error)
                    break
        archive = {"clockClosed": True, "sourceCopiesVerified": verified,
                   "failure": receipt.get("retentionFailure"), "files": retained_hashes}
        (args.retain / "archive.json").write_text(json.dumps(archive, indent=2) + "\n", encoding="utf-8")
        final_clock = {"powerAfterRetention": power(), "elapsedSecondsThroughRetention": time.monotonic() - STARTED,
                       "maximumTotalSeconds": MAXIMUM, "clockClosed": True}
        final_clock["withinDeclaredAllowance"] = final_clock["elapsedSecondsThroughRetention"] <= MAXIMUM
        final_clock["status"] = "passed" if receipt["status"] == "passed" and not receipt["cleanupIncomplete"] and final_clock["withinDeclaredAllowance"] and verified else "failed"
        (args.retain / "final-clock.json").write_text(json.dumps(final_clock, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"failure": receipt.get("failure"), **final_clock}), flush=True)
    raise SystemExit(0 if final_clock["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
