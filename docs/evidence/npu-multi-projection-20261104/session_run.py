"""Execute one predeclared HMX host-reconstruction & multi-projection resident control trial."""

from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

RUNNER = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner")
PRISM = Path(r"C:\Users\symbi\source\repos\prism-inference")
FINAL_EVIDENCE_DIR = RUNNER / "docs" / "evidence" / "npu-multi-projection-20261104"
STAGE_ROOT = Path(os.environ["TEMP"]) / "SwiftBonsai-MultiProj-20261104"
EVIDENCE_DIR = STAGE_ROOT / "evidence"
PRISM_TARGET = Path(os.environ["TEMP"]) / "SwiftBonsai-Hmx-Session-20261103" / "prism-target"
RUNNER_TARGET = Path(os.environ["TEMP"]) / "SwiftBonsai-Hmx-Session-20261103" / "runner-target"
PROBE_EXE = PRISM_TARGET / "aarch64-pc-windows-msvc" / "release" / "prism-hexagon-model-probe.exe"
SUPERVISOR_EXE = RUNNER_TARGET / "release" / "bonsai-tools.exe"
RUSTC = Path.home() / r".rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\bin\rustc.exe"
GIT = Path(shutil.which("git"))
BUDGET_SECONDS = 540
ESTIMATE_SECONDS = 180

BUILD_PRISM_PS1 = r"""$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\source\repos\SwiftBonsai2Runner\docs\evidence\hexagon-adapter-session-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\prism-target'
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-native --lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-native -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --lib --bin prism-hexagon-model-probe -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
exit $LASTEXITCODE
"""

BUILD_RUNNER_PS1 = r"""$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\source\repos\SwiftBonsai2Runner\docs\evidence\hexagon-adapter-session-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\runner-target'
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release -p bonsai-tools --lib hexagon_tools::model_device::tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --offline --locked --release -p bonsai-tools --lib -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --offline --locked --release -p bonsai-tools --bin bonsai-tools
exit $LASTEXITCODE
"""


def utc_now():
    return datetime.now(timezone.utc).isoformat()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def query_power():
    ps_script = (
        "Add-Type -AssemblyName System.Windows.Forms; "
        "$p = [System.Windows.Forms.SystemInformation]::PowerStatus; "
        "$ac = switch ($p.PowerLineStatus) { 'Online' { $true } 'Offline' { $false } default { $null } }; "
        "$pct = if ($p.BatteryLifePercent -ge 0 -and $p.BatteryLifePercent -le 1) "
        "{ [int][math]::Round($p.BatteryLifePercent * 100) } else { $null }; "
        "$saver = $null; "
        "try { $v = (Get-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Power' "
        "-Name 'EnergySaverState' -ErrorAction Stop).EnergySaverState; $saver = [bool]($v -ne 0) } catch {}; "
        "@{ available = ($null -ne $ac); acConnected = $ac; batterySaver = $saver; batteryPercent = $pct; "
        "powerLineStatus = $p.PowerLineStatus.ToString(); batteryChargeStatus = $p.BatteryChargeStatus.ToString(); "
        "sampledAtUtc = (Get-Date).ToUniversalTime().ToString('o') } | ConvertTo-Json -Compress"
    )
    try:
        completed = subprocess.run(
            ["powershell.exe", "-NoProfile", "-Command", ps_script],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
        return json.loads(completed.stdout.strip())
    except Exception as exc:
        return {
            "available": False,
            "acConnected": None,
            "batterySaver": None,
            "batteryPercent": None,
            "error": str(exc),
            "sampledAtUtc": utc_now(),
        }


def run_step(name, command, cwd, started_monotonic, earlier_charged, max_step_seconds):
    elapsed_before = earlier_charged + (time.monotonic() - started_monotonic)
    remaining = BUDGET_SECONDS - elapsed_before - 10
    if remaining < 5:
        raise RuntimeError(f"Insufficient aggregate budget before step {name}: {remaining:.2f}s remaining")
    timeout = min(max_step_seconds, remaining)
    power_before = query_power()
    started_at = utc_now()
    step_start = time.monotonic()
    timed_out = False
    try:
        completed = subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        exit_code = completed.returncode
        stdout = completed.stdout
        stderr = completed.stderr
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = -1
        stdout = exc.stdout or ""
        stderr = exc.stderr or ""
    duration = time.monotonic() - step_start
    power_after = query_power()
    receipt = {
        "schemaVersion": 1,
        "step": name,
        "command": [str(part) for part in command],
        "workingDirectory": str(cwd),
        "startedAtUtc": started_at,
        "completedAtUtc": utc_now(),
        "durationSeconds": round(duration, 3),
        "AggregateElapsedSecondsAtEnd": round(earlier_charged + (time.monotonic() - started_monotonic), 3),
        "AggregateBudgetSeconds": BUDGET_SECONDS,
        "timedOut": timed_out,
        "exitCode": exit_code,
        "powerBefore": power_before,
        "powerAfter": power_after,
    }
    (EVIDENCE_DIR / f"{name}.stdout.txt").write_text(stdout[-65536:], encoding="utf-8")
    (EVIDENCE_DIR / f"{name}.stderr.txt").write_text(stderr[-65536:], encoding="utf-8")
    write_json(EVIDENCE_DIR / f"{name}.json", receipt)
    if timed_out or exit_code != 0:
        raise RuntimeError(f"Step {name} failed (exitCode={exit_code}, timedOut={timed_out})")
    return receipt


def ensure_committed(repo, message):
    prefix = [str(GIT), "-c", f"safe.directory={repo.as_posix()}", "-C", str(repo)]
    status = subprocess.check_output(prefix + ["status", "--porcelain"], text=True).strip()
    if status:
        subprocess.run(prefix + ["add", "-u"], check=True)
        subprocess.run(prefix + ["commit", "-m", message], check=True)
    post_status = subprocess.check_output(prefix + ["status", "--porcelain"], text=True).strip()
    if post_status:
        raise RuntimeError(f"Repository {repo} still dirty after commit: {post_status}")
    return subprocess.check_output(prefix + ["rev-parse", "HEAD"], text=True).strip()


def retain_control_artifacts(source_dir, dest_dir):
    dest_dir.mkdir(parents=True, exist_ok=True)
    for filename in ["intake.json", "local-model-build.json", "model-device-request.json", "observed-probe-build.json"]:
        src = source_dir / filename
        if src.is_file():
            shutil.copy2(src, dest_dir / filename)
    device_dir = source_dir / "device"
    if device_dir.is_dir():
        dest_device = dest_dir / "device"
        dest_device.mkdir(parents=True, exist_ok=True)
        for item in device_dir.iterdir():
            if item.is_file():
                shutil.copy2(item, dest_device / item.name)


def extract_case_metrics(case_dir):
    report = json.loads((case_dir / "device" / "model-device.json").read_text(encoding="utf-8"))
    native = report["NativeResult"]
    comparisons = native["tokenComparisons"]
    return {
        "outcome": report["Outcome"],
        "stage": report["Stage"],
        "elapsedMilliseconds": report["ElapsedMilliseconds"],
        "powerBefore": report["PowerBefore"],
        "powerAfter": report["PowerAfter"],
        "cpuPowerBefore": native["cpuPowerBefore"],
        "cpuPowerAfter": native["cpuPowerAfter"],
        "npuPowerBefore": native["npuPowerBefore"],
        "npuPowerAfter": native["npuPowerAfter"],
        "projections": native.get("projections", [native["projection"]]),
        "residentHostWeights": native["residentHostWeights"],
        "batchFourOperations": native["batchFourOperations"],
        "nativeBatchOperations": native["nativeBatchOperations"],
        "nativeOperationsSubmitted": native["nativeOperationsSubmitted"],
        "nativeOperationsCompleted": native["nativeOperationsCompleted"],
        "nativeQueueWritesSubmitted": native["nativeQueueWritesSubmitted"],
        "nativeQueueWritesCompleted": native["nativeQueueWritesCompleted"],
        "projectionsCompleted": native["projectionsCompleted"],
        "integerAudit": native["integerAudit"],
        "integerValuesCompared": native["integerValuesCompared"],
        "logitsBitwiseEqual": native["logitsBitwiseEqual"],
        "stateBitwiseEqual": native["stateBitwiseEqual"],
        "logitsSha256": native["logitsSha256"],
        "stateSha256": native["stateSha256"],
        "nativeCompletedCycle": native["nativeCompletedCycle"],
        "nativeCleanup": native["nativeCleanup"],
        "nativeResources": native["nativeResources"],
        "hostResidency": native.get("hostResidency"),
        "tokenTimings": [
            {
                "index": item["index"],
                "tokenId": item["tokenId"],
                "logitsSha256": item["logitsSha256"],
                "stateSha256": item["stateSha256"],
                **item["timing"],
            }
            for item in comparisons
        ],
    }


def main():
    if STAGE_ROOT.exists():
        shutil.rmtree(STAGE_ROOT)
    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    (EVIDENCE_DIR / "build-prism.ps1").write_text(BUILD_PRISM_PS1, encoding="utf-8")
    (EVIDENCE_DIR / "build-runner.ps1").write_text(BUILD_RUNNER_PS1, encoding="utf-8")

    # Preserve original clock and prior build receipts from the same predeclared allowance.
    earlier_charged = 0.0
    if FINAL_EVIDENCE_DIR.exists():
        for filename in ["workload-plan.json", "clock.json"]:
            if (FINAL_EVIDENCE_DIR / filename).is_file():
                shutil.copy2(FINAL_EVIDENCE_DIR / filename, EVIDENCE_DIR / filename)
        if (FINAL_EVIDENCE_DIR / "workload-close.json").is_file():
            prior_close = json.loads((FINAL_EVIDENCE_DIR / "workload-close.json").read_text(encoding="utf-8"))
            earlier_charged = float(prior_close.get("elapsedSeconds", 53.406))
        for prefix in [
            "initial-build-prism",
            "build-prism",
            "initial-build-runner",
            "second-build-runner",
            "build-runner",
        ]:
            for ext in [".json", ".stdout.txt", ".stderr.txt"]:
                src = FINAL_EVIDENCE_DIR / f"{prefix}{ext}"
                if src.is_file():
                    shutil.copy2(src, EVIDENCE_DIR / f"{prefix}{ext}")
        shutil.rmtree(FINAL_EVIDENCE_DIR)

    if not (EVIDENCE_DIR / "workload-plan.json").is_file():
        plan = {
            "schemaVersion": 1,
            "kind": "owner-device-predeclared-workload-plan",
            "declaredAtUtc": utc_now(),
            "defaultBudgetSeconds": 300,
            "predeclaredAllowanceSeconds": BUDGET_SECONDS,
            "conservativeEstimateSeconds": ESTIMATE_SECONDS,
            "ceilingSeconds": 900,
            "reason": (
                "Two supervised 3-token local-build model-device controls (single-projection "
                "blk.0.ffn_up.weight batch4-resident control to measure host-side bitplane/dot "
                "reconstruction optimization, and two-projection blk.0.ffn_up.weight + "
                "blk.0.ffn_gate.weight batch4-resident control to evaluate multi-projection "
                "placement within the 64 MiB arena and 24,000-operation cap) plus incremental "
                "offline release compilation and unit tests of prism-hexagon-model-probe and "
                "bonsai-tools."
            ),
            "fixedWork": [
                "Incremental offline release build and unit tests of bonsai-hexagon-native, bonsai-hexagon-runtime, and prism-hexagon-model-probe",
                "Incremental offline release build and unit tests of bonsai-tools",
                "Control 1 (single-projection-batch4-resident): 3 tokens [42, 43, 128], standard-scale-1.3 adapter, neon 4 row workers, residentHostWeights=true, batchFourOperations=true, projections=['blk.0.ffn_up.weight'] (120 ops in 30 queue writes, 25,067,520 resident weight bytes)",
                "Control 2 (two-projection-batch4-resident): 3 tokens [42, 43, 128], standard-scale-1.3 adapter, neon 4 row workers, residentHostWeights=true, batchFourOperations=true, projections=['blk.0.ffn_up.weight', 'blk.0.ffn_gate.weight'] (240 ops in 60 queue writes, 50,135,040 resident weight bytes)",
                "Evidence retention and staging directory cleanup",
            ],
        }
        write_json(EVIDENCE_DIR / "workload-plan.json", plan)

    started_monotonic = time.monotonic()
    if (EVIDENCE_DIR / "clock.json").is_file():
        clock_obj = json.loads((EVIDENCE_DIR / "clock.json").read_text(encoding="utf-8"))
        clock_start_utc = clock_obj["startedAtUtc"]
        power_before = clock_obj["powerBefore"]
        clock_obj["earlierChargedExecutionSeconds"] = earlier_charged
        clock_obj["resumedAtUtc"] = utc_now()
        write_json(EVIDENCE_DIR / "clock.json", clock_obj)
    else:
        clock_start_utc = utc_now()
        power_before = query_power()
        write_json(
            EVIDENCE_DIR / "clock.json",
            {
                "schemaVersion": 1,
                "kind": "owner-device-workload-clock",
                "startedAtUtc": clock_start_utc,
                "budgetSeconds": BUDGET_SECONDS,
                "estimatedSeconds": ESTIMATE_SECONDS,
                "powerBefore": power_before,
            },
        )

    failure = None
    try:
        prism_already_built = False
        if (EVIDENCE_DIR / "build-prism.json").is_file():
            prior_prism = json.loads((EVIDENCE_DIR / "build-prism.json").read_text(encoding="utf-8"))
            prism_already_built = prior_prism.get("exitCode") == 0 and not prior_prism.get("timedOut", False)
        if not prism_already_built:
            run_step(
                "build-prism",
                [
                    "powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(EVIDENCE_DIR / "build-prism.ps1"),
                ],
                PRISM,
                started_monotonic,
                earlier_charged,
                180,
            )
        runner_already_built = False
        if (EVIDENCE_DIR / "build-runner.json").is_file():
            prior_runner = json.loads((EVIDENCE_DIR / "build-runner.json").read_text(encoding="utf-8"))
            runner_already_built = prior_runner.get("exitCode") == 0 and not prior_runner.get("timedOut", False)
        if not runner_already_built:
            run_step(
                "build-runner",
                [
                    "powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(EVIDENCE_DIR / "build-runner.ps1"),
                ],
                RUNNER,
                started_monotonic,
                earlier_charged,
                120,
            )

        prism_rev = ensure_committed(
            PRISM,
            "Optimize HMX host bitplane reconstruction and support multi-projection probe",
        )
        runner_rev = ensure_committed(
            RUNNER,
            "Support multi-projection HMX controls in model-device supervisor",
        )
        write_json(
            EVIDENCE_DIR / "revisions.json",
            {"prismRevision": prism_rev, "runnerRevision": runner_rev},
        )

        controls = [
            ("single-projection-batch4-resident", ["blk.0.ffn_up.weight"]),
            ("two-projection-batch4-resident", ["blk.0.ffn_up.weight", "blk.0.ffn_gate.weight"]),
        ]
        for case_name, projections in controls:
            stage_case = STAGE_ROOT / case_name
            prep_cmd = [
                "python",
                str(RUNNER / "tools" / "hexagon" / "prepare_model_control.py"),
                "--runner-repository",
                str(RUNNER),
                "--prism-repository",
                str(PRISM),
                "--probe",
                str(PROBE_EXE),
                "--supervisor",
                str(SUPERVISOR_EXE),
                "--compiler",
                str(RUSTC),
                "--successful-build-receipt",
                str(EVIDENCE_DIR / "build-prism.json"),
                "--output-directory",
                str(stage_case),
                "--git",
                str(GIT),
                "--adapter-case",
                "standard-scale-1.3",
                "--token-ids",
                "42",
                "43",
                "128",
                "--resident-host-weights",
                "--batch-four-operations",
                "--projections",
                *projections,
            ]
            run_step(f"prepare-{case_name}", prep_cmd, RUNNER, started_monotonic, earlier_charged, 30)
            intake = json.loads((stage_case / "intake.json").read_text(encoding="utf-8"))
            run_step(f"device-{case_name}", intake["deviceCommand"], RUNNER, started_monotonic, earlier_charged, 240)
            retain_control_artifacts(stage_case, EVIDENCE_DIR / case_name)
            shutil.rmtree(stage_case, ignore_errors=True)

        summary = {
            "schemaVersion": 1,
            "kind": "hexagon-hmx-multi-projection-control-summary",
            "prismRevision": prism_rev,
            "runnerRevision": runner_rev,
            "singleProjectionBatch4Resident": extract_case_metrics(
                EVIDENCE_DIR / "single-projection-batch4-resident"
            ),
            "twoProjectionBatch4Resident": extract_case_metrics(
                EVIDENCE_DIR / "two-projection-batch4-resident"
            ),
        }
        write_json(EVIDENCE_DIR / "summary.json", summary)
    except Exception as exc:
        failure = str(exc)
        raise
    finally:
        power_after = query_power()
        elapsed_total = round(earlier_charged + (time.monotonic() - started_monotonic), 3)
        write_json(
            EVIDENCE_DIR / "workload-close.json",
            {
                "schemaVersion": 1,
                "kind": "owner-device-workload-close",
                "startedAtUtc": clock_start_utc,
                "completedAtUtc": utc_now(),
                "earlierChargedExecutionSeconds": earlier_charged,
                "elapsedSeconds": elapsed_total,
                "budgetSeconds": BUDGET_SECONDS,
                "withinBudget": elapsed_total <= BUDGET_SECONDS,
                "powerBefore": power_before,
                "powerAfter": power_after,
                "powerStableAcrossWorkload": (
                    power_before.get("available") is True
                    and power_after.get("available") is True
                    and power_before.get("acConnected") == power_after.get("acConnected")
                    and power_before.get("batterySaver") == power_after.get("batterySaver")
                ),
                "failure": failure,
            },
        )
        if FINAL_EVIDENCE_DIR.exists():
            shutil.rmtree(FINAL_EVIDENCE_DIR)
        shutil.copytree(EVIDENCE_DIR, FINAL_EVIDENCE_DIR)
        shutil.copy2(Path(__file__), FINAL_EVIDENCE_DIR / "session_run.py")
        shutil.rmtree(STAGE_ROOT, ignore_errors=True)


if __name__ == "__main__":
    main()
