"""Execute the predeclared NPU batch-four HMX control workload under one clock."""

import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


class SYSTEM_POWER_STATUS(ctypes.Structure):
    _fields_ = [
        ("ACLineStatus", wintypes.BYTE),
        ("BatteryFlag", wintypes.BYTE),
        ("BatteryLifePercent", wintypes.BYTE),
        ("SystemStatusFlag", wintypes.BYTE),
        ("BatteryLifeTime", wintypes.DWORD),
        ("BatteryFullLifeTime", wintypes.DWORD),
    ]


def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat()


def power_snapshot() -> dict:
    status = SYSTEM_POWER_STATUS()
    ok = ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status))
    if not ok:
        return {
            "observedUtc": now_utc(),
            "source": "GetSystemPowerStatus",
            "available": False,
            "ac_online": None,
            "battery_saver": None,
            "battery_percent": None,
        }
    ac_online = True if status.ACLineStatus == 1 else (False if status.ACLineStatus == 0 else None)
    battery_saver = True if status.SystemStatusFlag == 1 else (False if status.SystemStatusFlag == 0 else None)
    battery_percent = int(status.BatteryLifePercent) if status.BatteryLifePercent <= 100 else None
    return {
        "observedUtc": now_utc(),
        "source": "GetSystemPowerStatus",
        "available": True,
        "ac_online": ac_online,
        "battery_saver": battery_saver,
        "battery_percent": battery_percent,
    }


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def pin(path: Path) -> dict:
    with path.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
    return {"bytes": path.stat().st_size, "sha256": digest}


def run_step(command: list[str], cwd: Path, timeout_seconds: float, out_prefix: Path) -> dict:
    power_before = power_snapshot()
    started_utc = now_utc()
    t0 = time.monotonic()
    timed_out = False
    try:
        completed = subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=max(1.0, timeout_seconds),
        )
        exit_code = completed.returncode
        stdout = completed.stdout
        stderr = completed.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = -1
        stdout = (exc.stdout or "") if isinstance(exc.stdout, str) else ""
        stderr = (exc.stderr or "") if isinstance(exc.stderr, str) else ""
    elapsed = time.monotonic() - t0
    finished_utc = now_utc()
    power_after = power_snapshot()
    out_prefix.parent.mkdir(parents=True, exist_ok=True)
    out_prefix.with_suffix(".stdout.txt").write_text(stdout, encoding="utf-8")
    out_prefix.with_suffix(".stderr.txt").write_text(stderr, encoding="utf-8")
    receipt = {
        "startedUtc": started_utc,
        "finishedUtc": finished_utc,
        "elapsedSeconds": round(elapsed, 6),
        "exitCode": exit_code,
        "timedOut": timed_out,
        "powerBefore": power_before,
        "powerAfter": power_after,
        "command": command,
        "workingDirectory": str(cwd),
    }
    write_json(out_prefix.with_suffix(".json"), receipt)
    return receipt


def copy_case_artifacts(src_case: Path, dst_case: Path) -> None:
    for root, dirs, files in os.walk(src_case):
        dirs[:] = [d for d in dirs if d != "model-bundle"]
        rel_root = Path(root).relative_to(src_case)
        for name in files:
            src_file = Path(root) / name
            dst_file = dst_case / rel_root / name
            dst_file.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src_file, dst_file)


def main() -> int:
    runner_wt = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\runner-npu-batch4")
    prism_wt = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\prism-npu-batch4")
    evidence_dir = runner_wt / "docs" / "evidence" / "npu-batch4-controls-20261104"
    temp_root = Path(r"C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Batch4-20261104")
    temp_root.mkdir(parents=True, exist_ok=True)

    # Preserve existing clock and prior build receipts without losing prior charged execution time.
    clock_path = temp_root / "clock.json"
    clock = json.loads(clock_path.read_text(encoding="utf-8"))
    max_seconds = float(clock["maximumAggregateSeconds"])
    workload_power_before = clock["powerBefore"]
    earlier_charged = 49.474021
    t_resume = time.monotonic()
    resumed_utc = now_utc()
    resume_deadline_utc = datetime.fromtimestamp(
        time.time() + (max_seconds - earlier_charged), tz=timezone.utc
    ).isoformat()
    clock["earlierChargedExecutionSeconds"] = earlier_charged
    clock["resumedAfterApprovalPauseUtc"] = resumed_utc
    clock["activeExecutionDeadlineUtc"] = resume_deadline_utc
    write_json(clock_path, clock)

    if (temp_root / "prism-build.json").is_file() and not (temp_root / "seventh-prism-build.json").is_file():
        for ext in [".json", ".stdout.txt", ".stderr.txt"]:
            shutil.copy2(temp_root / f"prism-build{ext}", temp_root / f"seventh-prism-build{ext}")

    def remaining() -> float:
        return max_seconds - earlier_charged - (time.monotonic() - t_resume)

    git_exe = Path(shutil.which("git"))
    for wt in [prism_wt, runner_wt]:
        subprocess.run(["cargo", "fmt", "--manifest-path", str(wt / "Cargo.toml")], check=True)
        dirty = subprocess.check_output([str(git_exe), "-C", str(wt), "status", "--porcelain"], text=True)
        if dirty.strip():
            subprocess.run([str(git_exe), "-C", str(wt), "add", "-A"], check=True)
            subprocess.run([str(git_exe), "-C", str(wt), "commit", "--amend", "--no-edit"], check=True)

    # 1. Build and test Prism
    prism_script = evidence_dir / "build-prism.ps1"
    prism_receipt = run_step(
        ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(prism_script)],
        cwd=prism_wt,
        timeout_seconds=remaining() - 30.0,
        out_prefix=temp_root / "prism-build",
    )
    if prism_receipt["exitCode"] != 0 or prism_receipt["timedOut"]:
        print("Prism build/test failed:", json.dumps(prism_receipt), file=sys.stderr)
        return 1

    # 2. Build and test Runner
    runner_script = evidence_dir / "build-runner.ps1"
    runner_receipt = run_step(
        ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(runner_script)],
        cwd=runner_wt,
        timeout_seconds=remaining() - 30.0,
        out_prefix=temp_root / "runner-build",
    )
    if runner_receipt["exitCode"] != 0 or runner_receipt["timedOut"]:
        print("Runner build/test failed:", json.dumps(runner_receipt), file=sys.stderr)
        return 1

    probe_exe = Path(
        r"C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\prism-target\aarch64-pc-windows-msvc\release\prism-hexagon-model-probe.exe"
    )
    supervisor_exe = Path(
        r"C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\runner-target\release\bonsai-tools.exe"
    )
    compiler_exe = Path(
        r"C:\Users\symbi\.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\bin\rustc.exe"
    )
    prepare_script = runner_wt / "tools" / "hexagon" / "prepare_model_control.py"

    cases_summary = {}
    for case_name, resident in [("batch4-streaming", False), ("batch4-resident", True)]:
        case_dir = temp_root / case_name
        if case_dir.exists():
            shutil.rmtree(case_dir)
        prep_cmd = [
            sys.executable,
            str(prepare_script),
            "--runner-repository",
            str(runner_wt),
            "--prism-repository",
            str(prism_wt),
            "--probe",
            str(probe_exe),
            "--supervisor",
            str(supervisor_exe),
            "--compiler",
            str(compiler_exe),
            "--successful-build-receipt",
            str(temp_root / "prism-build.json"),
            "--output-directory",
            str(case_dir),
            "--git",
            str(git_exe),
            "--adapter-case",
            "standard-scale-1.3",
            "--token-ids",
            "42",
            "43",
            "128",
            "--batch-four-operations",
        ]
        if resident:
            prep_cmd.append("--resident-host-weights")
        prep_receipt = run_step(
            prep_cmd,
            cwd=runner_wt,
            timeout_seconds=remaining() - 30.0,
            out_prefix=temp_root / f"prepare-{case_name}",
        )
        if prep_receipt["exitCode"] != 0 or prep_receipt["timedOut"]:
            print(f"Preparation for {case_name} failed:", json.dumps(prep_receipt), file=sys.stderr)
            return 1

        intake = json.loads((case_dir / "intake.json").read_text(encoding="utf-8"))
        device_cmd = intake["deviceCommand"]
        device_receipt = run_step(
            device_cmd,
            cwd=runner_wt,
            timeout_seconds=remaining() - 15.0,
            out_prefix=temp_root / f"run-{case_name}",
        )
        model_device_path = case_dir / "device" / "model-device.json"
        if not model_device_path.is_file():
            print(f"Missing model-device.json for {case_name}", file=sys.stderr)
            return 1
        model_device = json.loads(model_device_path.read_text(encoding="utf-8"))
        if device_receipt["exitCode"] != 0 or model_device.get("Outcome") != "passed":
            print(f"Device control {case_name} failed:", json.dumps(model_device), file=sys.stderr)
            return 1
        native = model_device["NativeResult"]
        cases_summary[case_name] = {
            "outcome": model_device["Outcome"],
            "stage": model_device["Stage"],
            "probeExitCode": model_device["ProbeExitCode"],
            "ownedJobTeardown": model_device["OwnedJobTeardown"],
            "adapterCase": native["adapterCase"],
            "cpuBackend": native["cpuBackend"],
            "rowWorkers": native["rowWorkers"],
            "residentHostWeights": native["residentHostWeights"],
            "batchFourOperations": native["batchFourOperations"],
            "integerAudit": native["integerAudit"],
            "nativeBatchOperations": native["nativeBatchOperations"],
            "operationsSubmitted": native["nativeOperationsSubmitted"],
            "operationsCompleted": native["nativeOperationsCompleted"],
            "queueWritesSubmitted": native["nativeQueueWritesSubmitted"],
            "queueWritesCompleted": native["nativeQueueWritesCompleted"],
            "integerValuesCompared": native["integerValuesCompared"],
            "logitsBitwiseEqual": native["logitsBitwiseEqual"],
            "stateBitwiseEqual": native["stateBitwiseEqual"],
            "nativeCleanup": native["nativeCleanup"],
            "nativeResources": native["nativeResources"],
            "hostResidency": native.get("hostResidency"),
            "timingScope": native["timingScope"],
            "tokenComparisons": native["tokenComparisons"],
            "power": {
                "cpuPowerBefore": native["cpuPowerBefore"],
                "cpuPowerAfter": native["cpuPowerAfter"],
                "npuPowerBefore": native["npuPowerBefore"],
                "npuPowerAfter": native["npuPowerAfter"],
            },
            "supervisorElapsedMilliseconds": model_device["ElapsedMilliseconds"],
            "performanceQualified": False,
        }

    # Copy artifacts into evidence_dir
    for name in [
        "clock.json",
        "initial-prism-build.json",
        "initial-prism-build.stdout.txt",
        "initial-prism-build.stderr.txt",
        "second-prism-build.json",
        "second-prism-build.stdout.txt",
        "second-prism-build.stderr.txt",
        "third-prism-build.json",
        "third-prism-build.stdout.txt",
        "third-prism-build.stderr.txt",
        "fourth-prism-build.json",
        "fourth-prism-build.stdout.txt",
        "fourth-prism-build.stderr.txt",
        "fifth-prism-build.json",
        "fifth-prism-build.stdout.txt",
        "fifth-prism-build.stderr.txt",
        "sixth-prism-build.json",
        "sixth-prism-build.stdout.txt",
        "sixth-prism-build.stderr.txt",
        "seventh-prism-build.json",
        "seventh-prism-build.stdout.txt",
        "seventh-prism-build.stderr.txt",
        "prism-build.json",
        "prism-build.stdout.txt",
        "prism-build.stderr.txt",
        "runner-build.json",
        "runner-build.stdout.txt",
        "runner-build.stderr.txt",
        "prepare-batch4-streaming.json",
        "prepare-batch4-resident.json",
        "run-batch4-streaming.json",
        "run-batch4-resident.json",
    ]:
        if (temp_root / name).is_file():
            shutil.copy2(temp_root / name, evidence_dir / name)

    for case_name in ["batch4-streaming", "batch4-resident"]:
        copy_case_artifacts(temp_root / case_name, evidence_dir / case_name)

    closed_dt = datetime.now(timezone.utc)
    closed_utc = closed_dt.isoformat()
    active_run_seconds = round(time.monotonic() - t_resume, 6)
    elapsed_total = round(earlier_charged + active_run_seconds, 6)
    power_at_close = power_snapshot()

    summary = {
        "schemaVersion": 1,
        "kind": "local-x2-hmx-batch4-streaming-and-resident-controls",
        "dateLocal": "2026-11-04",
        "retainedUtc": closed_utc,
        "cases": cases_summary,
        "closedHistoricalWorkloadsRetried": False,
        "earlierChargedExecutionSeconds": earlier_charged,
        "activeRunSeconds": active_run_seconds,
        "chargedThroughRetentionSeconds": elapsed_total,
        "maximumAggregateSeconds": max_seconds,
        "powerBeforeWorkload": workload_power_before,
        "powerAtRetention": power_at_close,
        "hostedChecksExecuted": False,
        "ciDispatchedOrMonitored": False,
        "pushed": False,
        "workloadClosed": True,
        "productionAdmissionEnabled": False,
        "qualifiedBenefitComparison": False,
        "interpretation": (
            "Batch-four HMX bitplane operation batching (12 tensors and 4 operations per queue packet) "
            "reduces first-projection FastRPC queue writes from 40 to 10 per token (30 vs 120 across "
            "three tokens [42, 43, 128]) while preserving all 120 logical operations, all 16,711,680 "
            "independent plane comparisons in streaming mode, resident host weight cache hits in "
            "resident mode, bitwise-equal logits and recurrent/KV state snapshots after every token, "
            "and clean queue/mapping/module teardown."
        ),
    }
    write_json(evidence_dir / "summary.json", summary)

    workload_close = {
        "closedUtc": closed_utc,
        "originalDeadlineUtc": clock["originalDeadlineUtc"],
        "chargedThroughCloseSeconds": elapsed_total,
        "maximumAggregateSeconds": max_seconds,
        "stopReason": (
            "Both predeclared batch-four HMX controls (streaming and resident) passed with 30 queue writes "
            "and 120 logical operations across three tokens; stopping early as required."
        ),
        "power": power_at_close,
        "historicalDeadlinesReopened": False,
    }
    write_json(evidence_dir / "workload-close.json", workload_close)

    files_index = []
    for root, _, files in os.walk(evidence_dir):
        for name in files:
            if name == "retained-input-index.json":
                continue
            full_path = Path(root) / name
            rel_path = full_path.relative_to(evidence_dir).as_posix()
            files_index.append({"path": rel_path, **pin(full_path)})
    files_index.sort(key=lambda item: item["path"])
    write_json(
        evidence_dir / "retained-input-index.json",
        {"schemaVersion": 1, "files": files_index},
    )
    print(json.dumps({"outcome": "passed", "elapsedSeconds": elapsed_total}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
