"""One non-retryable model-free source check with a shared nine-minute clock for three new implementations."""

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
INVENTORY = Path(__file__).with_name("controls.json")
RUNNER_INVENTORY = Path(__file__).with_name("runner-controls.json")
CONTROLS = json.loads(INVENTORY.read_text(encoding="utf-8"))
RUNNER_CONTROLS = json.loads(RUNNER_INVENTORY.read_text(encoding="utf-8"))


class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]


def power():
    status = Power()
    query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
    query.argtypes = [ctypes.POINTER(Power)]
    query.restype = ctypes.c_int
    known = query(ctypes.byref(status)) != 0
    return {"source": "GetSystemPowerStatus", "observedUtc": datetime.now(timezone.utc).isoformat(),
            "acConnected": bool(status.ac) if known and status.ac in (0, 1) else None,
            "batterySaver": bool(status.saver) if known and status.saver in (0, 1) else None,
            "chargePercent": status.charge if known and status.charge <= 100 else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--runner-source", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--retain", type=Path, required=True)
    args = parser.parse_args()
    started = STARTED
    args.evidence.mkdir(parents=True, exist_ok=False)
    receipt = {"schemaVersion": 1, "startedUtc": datetime.now(timezone.utc).isoformat(),
               "maximumTotalSeconds": 540, "workCutoffSeconds": 490, "steps": [],
               "controls": CONTROLS,
               "driverSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               "inventorySha256": hashlib.sha256(INVENTORY.read_bytes()).hexdigest(),
               "runnerInventorySha256": hashlib.sha256(RUNNER_INVENTORY.read_bytes()).hexdigest(),
               "planSha256": hashlib.sha256(Path(__file__).with_name("workload-plan.json").read_bytes()).hexdigest(),
               "status": "failed", "modelsExecuted": 0, "benchmarksExecuted": 0,
               "fullSuitesExecuted": 0, "acceleratorDeviceCalls": 0,
               "ciActions": 0, "powerBefore": power()}
    env = dict(os.environ)
    env["PATH"] = str(TOOLCHAIN) + os.pathsep + env["PATH"]
    env["CARGO_TARGET_DIR"] = str(args.evidence / "target")
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_BUILD_JOBS"] = "4"
    env["RUSTUP_TOOLCHAIN"] = "nightly-2026-11-09-aarch64-pc-windows-msvc"

    def save():
        (args.evidence / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def run(name, command, cap=180, minimum=1):
        remaining = 490 - (time.monotonic() - started)
        if remaining < minimum:
            raise RuntimeError(f"Remaining allowance cannot fit {name}; no retry.")
        step = {"name": name, "arguments": [str(value) for value in command], "powerBefore": power()}
        receipt["steps"].append(step)
        begin = time.monotonic()
        # Short stable log names keep the retained tree below Windows path limits.
        log = args.evidence / (f"{len(receipt['steps']):02d}-" + name[:64] + ".log")
        step["log"] = log.name
        code = None
        try:
            with log.open("wb") as output:
                process = subprocess.Popen(step["arguments"], cwd=args.source, env=env,
                                           stdout=output, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
                try:
                    code = process.wait(timeout=min(cap, remaining))
                except BaseException as wait_error:
                    step["timedOut"] = isinstance(wait_error, subprocess.TimeoutExpired)
                    step["waitFailure"] = type(wait_error).__name__
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
        if code != 0 or step.get("timedOut"):
            raise RuntimeError(f"{name} failed; trial closes without retry.")
        return log.read_text(encoding="utf-8", errors="replace")

    def source_hashes(root, label):
        names = run(label + "-source-files", [GIT, "-C", root, "-c", f"safe.directory={root.as_posix()}",
                                      "ls-files"], cap=10).splitlines()
        return {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
                for name in names if (root / name).is_file()}

    try:
        all_controls = [name for names in CONTROLS.values() for name in names] + RUNNER_CONTROLS
        if (len(CONTROLS) != 6 or any(not names for names in CONTROLS.values())
                or sum(map(len, CONTROLS.values())) != 37 or len(RUNNER_CONTROLS) != 6
                or len(set(all_controls)) != 43):
            raise RuntimeError("Fixed inventories must contain exactly 37 distinct Prism and six Runner controls")
        run("compiler", [TOOLCHAIN / "rustc.exe", "-vV"], cap=10)
        receipt["sourceCommit"] = run("source-head", [GIT, "-c", f"safe.directory={args.source.as_posix()}",
                                                     "rev-parse", "HEAD"], cap=10).strip()
        declared_source = json.loads(Path(__file__).with_name("workload-plan.json").read_text(encoding="utf-8"))["source"]["prismSelectedRevision"]
        if receipt["sourceCommit"] != declared_source:
            raise RuntimeError("Selected source does not match the fixed declaration")
        # Apply canonical formatting once before any compiler/test command.
        run("format", [TOOLCHAIN / "cargo.exe", "fmt", "--all"], cap=30)
        run("format-check", [TOOLCHAIN / "cargo.exe", "fmt", "--all", "--", "--check"], cap=30)
        run("runner-format", [TOOLCHAIN / "cargo.exe", "fmt", "--manifest-path", args.runner_source / "Cargo.toml", "--all"], cap=30)
        run("runner-format-check", [TOOLCHAIN / "cargo.exe", "fmt", "--manifest-path", args.runner_source / "Cargo.toml", "--all", "--", "--check"], cap=30)
        receipt["runnerSourceCommit"] = run("runner-source-head", [GIT, "-C", args.runner_source, "-c", f"safe.directory={args.runner_source.as_posix()}", "rev-parse", "HEAD"], cap=10).strip()
        declared_runner = json.loads(Path(__file__).with_name("workload-plan.json").read_text(encoding="utf-8"))["source"]["runnerSelectedRevision"]
        if receipt["runnerSourceCommit"] != declared_runner:
            raise RuntimeError("Runner source differs from fixed declaration")
        receipt["sourceSha256"] = source_hashes(args.source, "prism")
        receipt["runnerSourceSha256"] = source_hashes(args.runner_source, "runner")
        receipt["runnerFormattedSourceDiff"] = run("runner-formatted-diff", [GIT, "-C", args.runner_source, "-c", f"safe.directory={args.runner_source.as_posix()}", "diff", "HEAD", "--no-ext-diff", "--no-color"], cap=10)
        receipt["formattedSourceDiff"] = run("formatted-source-diff", [GIT, "-c",
            f"safe.directory={args.source.as_posix()}", "diff", "HEAD", "--no-ext-diff", "--no-color"], cap=10)
        output = run("receipt-controls", [os.sys.executable, "-B", "-m", "unittest", "discover", "-s",
                                 str(args.source / ".github/ci"), "-p", "test_inference_efficiency.py", "-v"], cap=15)
        expected_receipt_controls = [
            "test_source_identity_binds_runtime_controls_and_format_policy",
            "test_source_identity_binds_the_analyzer_dependency_policy",
            "test_exact_control_rejects_zero_ignored_failed_or_duplicate_matches",
            "test_artifacts_require_all_unique_selected_test_executables",
            "test_fixed_inventory_is_unique_and_all_controls_exist_in_source",
            "test_neon_controls_require_the_exact_supported_target_without_mutating_portable_gate",
        ]
        if (not re.search(r"Ran 6 tests in [0-9.]+s", output)
                or output.splitlines().count("OK") != 1
                or any(not re.search(r"^" + re.escape(name) + r" \(.+\) \.\.\. ok$", output, re.MULTILINE)
                       for name in expected_receipt_controls)):
            raise RuntimeError("Absent or skipped receipt control")
        packages = ["-p", "prism-inference", "-p", "bonsai-hexagon-native", "-p", "bonsai-hexagon-runtime", "-p", "prism-inference-server"]
        output = run("compile-controls", [TOOLCHAIN / "cargo.exe", "test", "--locked", "--offline",
                      *packages, "--features", "prism-inference/parallel-rows,prism-inference-server/parallel-rows", "--lib", "--test", "graph", "--test", "runtime", "--no-run",
                      "--message-format=json"], cap=175, minimum=160)
        executables = {}
        for line in output.splitlines():
            if not line.startswith("{"):
                continue
            record = json.loads(line)
            if record.get("reason") == "compiler-artifact" and record.get("executable") and record.get("profile", {}).get("test"):
                name = record["target"]["name"]
                if name in CONTROLS:
                    if name in executables:
                        raise RuntimeError("Duplicate test artifact")
                    executables[name] = Path(record["executable"])
        if set(executables) != set(CONTROLS):
            raise RuntimeError("Missing selected test artifact")
        receipt["executableSha256"] = {name: hashlib.sha256(path.read_bytes()).hexdigest()
                                       for name, path in executables.items()}
        for target, names in CONTROLS.items():
            for name in names:
                output = run(name.replace("::", "-"), [executables[target], "--exact", name, "--nocapture"], cap=15)
                lines = output.replace("\r\n", "\n").splitlines()
                if (lines.count("running 1 test") != 1 or lines.count(f"test {name} ... ok") != 1
                        or sum(bool(re.fullmatch(
                            r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in .+",
                            line)) for line in lines) != 1):
                    raise RuntimeError("Absent, ignored or failed exact control")
        output = run("runner-compile-controls", [TOOLCHAIN / "cargo.exe", "test", "--manifest-path", args.runner_source / "Cargo.toml", "--locked", "--offline", "-p", "bonsai-tools", "--lib", "--no-run", "--message-format=json"], cap=85, minimum=75)
        runner_artifacts = [json.loads(line) for line in output.splitlines() if line.startswith("{")]
        runner_executables = [Path(record["executable"]) for record in runner_artifacts if record.get("reason") == "compiler-artifact" and record.get("executable") and record.get("profile", {}).get("test") and record["target"]["name"] == "bonsai_tools"]
        if len(runner_executables) != 1:
            raise RuntimeError("Runner parser test executable missing or duplicated")
        runner_executable = runner_executables[0]
        receipt["runnerExecutableSha256"] = hashlib.sha256(runner_executable.read_bytes()).hexdigest()
        for name in RUNNER_CONTROLS:
            output = run(name.replace("::", "-"), [runner_executable, "--exact", name, "--nocapture"], cap=15)
            lines = output.replace("\r\n", "\n").splitlines()
            if (lines.count("running 1 test") != 1 or lines.count(f"test {name} ... ok") != 1
                    or sum(bool(re.fullmatch(r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in .+", line)) for line in lines) != 1):
                raise RuntimeError("Runner parser exact control absent, ignored or failed")
        run("runner-strict-analysis", [TOOLCHAIN / "cargo.exe", "clippy", "--manifest-path", args.runner_source / "Cargo.toml", "--locked", "--offline", "--release", "-p", "bonsai-tools", "--lib", "--", "-D", "warnings"], cap=45, minimum=35)
        run("strict-analysis", [TOOLCHAIN / "cargo.exe", "clippy", "--locked", "--offline", "--release",
            *packages, "--lib", "--test", "graph", "--test", "runtime", "--features", "prism-inference/parallel-rows,prism-inference-server/parallel-rows", "--", "-D", "warnings"], cap=115, minimum=100)
        run("production-worker-link", [TOOLCHAIN / "cargo.exe", "build", "--locked", "--offline", "--release",
            "-p", "prism-inference", "-p", "prism-inference-server", "--bin", "bonsai-swift-worker", "--bin", "prism-inference-server", "--features", "prism-inference/parallel-rows,prism-inference-server/parallel-rows"], cap=55, minimum=45)
        run("feature-disabled", [TOOLCHAIN / "cargo.exe", "check", "--locked", "--offline", "-p",
            "prism-inference", "--lib", "--no-default-features"], cap=35, minimum=25)
        if (source_hashes(args.source, "prism") != receipt["sourceSha256"]
                or source_hashes(args.runner_source, "runner") != receipt["runnerSourceSha256"]):
            raise RuntimeError("Sources changed during checks")
        identities = [(Path(__file__), "driverSha256"), (INVENTORY, "inventorySha256"),
                      (RUNNER_INVENTORY, "runnerInventorySha256"), (Path(__file__).with_name("workload-plan.json"), "planSha256")]
        if any(hashlib.sha256(path.read_bytes()).hexdigest() != receipt[key] for path, key in identities):
            raise RuntimeError("Driver, declaration or fixed inventories changed during checks")
        receipt["status"] = "passed"
    except Exception as error:
        receipt["failure"] = str(error)
    finally:
        receipt["cleanupIncomplete"] = any(
            step.get("cleanupRequired") and not (
                step.get("cleanup", {}).get("treeTerminationReported")
                and step.get("cleanup", {}).get("parentExited")
            ) for step in receipt["steps"]
        )
        if receipt["cleanupIncomplete"]:
            receipt["status"] = "failed"
            receipt["failure"] = "Owned process cleanup incomplete"
        receipt["powerAfter"] = power()
        receipt["elapsedSecondsThroughChecks"] = time.monotonic() - started
        receipt["clockClosed"] = True
        receipt["clockRestarted"] = False
        receipt["retries"] = 0
        receipt["continuousPowerStabilityEstablished"] = False
        receipt["performanceComparisonQualified"] = False
        save()
        args.retain.mkdir(parents=True, exist_ok=False)
        for path in args.evidence.iterdir():
            if path.is_file():
                shutil.copyfile(path, args.retain / path.name)
        archival_verified = all(hashlib.sha256(path.read_bytes()).hexdigest() == hashlib.sha256((args.retain / path.name).read_bytes()).hexdigest()
                                for path in args.evidence.iterdir() if path.is_file())
        archive = {"clockClosed": True, "sourceCopiesVerified": archival_verified,
                   "files": {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                             for path in args.retain.iterdir() if path.is_file()}}
        (args.retain / "archive.json").write_text(json.dumps(archive, indent=2) + "\n", encoding="utf-8")
        final_clock = {"powerAfterRetention": power(),
                       "elapsedSecondsThroughRetention": time.monotonic() - started,
                       "maximumTotalSeconds": 540, "clockClosed": True}
        final_clock["withinDeclaredAllowance"] = final_clock["elapsedSecondsThroughRetention"] <= 540
        final_clock["status"] = "passed" if receipt["status"] == "passed" and not receipt["cleanupIncomplete"] and final_clock["withinDeclaredAllowance"] and archival_verified else "failed"
        (args.retain / "final-clock.json").write_text(json.dumps(final_clock, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"failure": receipt.get("failure"),
                          **final_clock}), flush=True)
    raise SystemExit(0 if final_clock["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
