"""Retain the actual local receipts without copying executables or model data."""
import hashlib
import json
from pathlib import Path
import shutil
import sys

from session_run import ROOT, now, power, write

EXTERNAL = Path(r"C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Progress-20261103")


def digest(path):
    with path.open("rb") as source:
        return {"bytes": path.stat().st_size,
                "sha256": hashlib.file_digest(source, "sha256").hexdigest()}


def retain_case(name):
    source = EXTERNAL / name
    if not source.exists():
        return None
    target = ROOT / name
    for path in source.rglob("*"):
        if not path.is_file() or "model-bundle" in path.relative_to(source).parts:
            continue
        assert path.stat().st_size < 2 * 1024 * 1024, "Unexpected evidence size"
        destination = target / path.relative_to(source)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    report_path = target / "device/model-device.json"
    if not report_path.exists():
        return {"outcome": "not-executed-or-no-terminal-receipt"}
    report = json.loads(report_path.read_text(encoding="utf-8-sig"))
    result = report.get("NativeResult", {})
    return {"outcome": report.get("Outcome"), "stage": report.get("Stage"),
            "probeExitCode": report.get("ProbeExitCode"),
            "ownedJobTeardown": report.get("OwnedJobTeardown"),
            "adapterCase": result.get("adapterCase"),
            "cpuBackend": result.get("cpuBackend"), "rowWorkers": result.get("rowWorkers"),
            "residentHostWeights": result.get("residentHostWeights", False),
            "integerAudit": result.get("integerAudit", "all-returned-integers"),
            "operationsCompleted": result.get("nativeOperationsCompleted"),
            "integerValuesCompared": result.get("integerValuesCompared"),
            "logitsBitwiseEqual": result.get("logitsBitwiseEqual"),
            "stateBitwiseEqual": result.get("stateBitwiseEqual"),
            "nativeCleanup": result.get("nativeCleanup"),
            "nativeResources": result.get("nativeResources"),
            "hostResidency": result.get("hostResidency"),
            "timingScope": result.get("timingScope"),
            "tokenComparisons": result.get("tokenComparisons"),
            "power": {key: result.get(key) for key in
                      ["cpuPowerBefore", "cpuPowerAfter", "npuPowerBefore", "npuPowerAfter"]},
            "supervisorElapsedMilliseconds": report.get("ElapsedMilliseconds"),
            "performanceQualified": False}


if __name__ == "__main__":
    cases = {name: retain_case(name) for name in ["standard", "loaded-zero", "resident"]}
    clock = json.loads((ROOT / "clock.json").read_text())
    import datetime as dt
    elapsed = (now() - dt.datetime.fromisoformat(clock["declarationUtc"])).total_seconds()
    write("summary.json", {
        "schemaVersion": 1, "kind": "local-x2-adapter-and-resident-host-progress",
        "dateLocal": "2026-11-03", "retainedUtc": now().isoformat(),
        "cases": cases, "closedHistoricalWorkloadsRetried": False,
        "elapsedSinceDeclarationSeconds": elapsed,
        "earlierReadOnlyPreparationChargeSeconds": clock["earlierReadOnlyPreparationChargeSeconds"],
        "chargedThroughRetentionSeconds": elapsed + clock["earlierReadOnlyPreparationChargeSeconds"],
        "maximumAggregateSeconds": clock["maximumAggregateSeconds"],
        "powerAtRetention": power(), "hostedChecksExecuted": False,
        "ciDispatchedOrMonitored": False, "pushed": False,
        "workloadClosed": "--close" in sys.argv,
        "productionAdmissionEnabled": False, "qualifiedBenefitComparison": False,
        "interpretation": "Correctness covers only the declared first-projection tokens and adapter cases. The resident screen explicitly replaces per-dot CPU audit with complete graph parity. Diagnostic evaluation timings exclude setup and teardown and establish no release benefit or persistent service acceptance."
    })
    if "--close" in sys.argv:
        write("workload-close.json", {
            "closedUtc": now().isoformat(), "originalDeadlineUtc": clock["deadlineUtc"],
            "chargedThroughCloseSeconds": (now() - dt.datetime.fromisoformat(clock["declarationUtc"])).total_seconds()
                + clock["earlierReadOnlyPreparationChargeSeconds"],
            "maximumAggregateSeconds": clock["maximumAggregateSeconds"],
            "stopReason": "Declared correctness and resident diagnostic finished; no speed benefit or usable production NPU profile established. No further device work selected.",
            "power": power(), "historicalDeadlinesReopened": False
        })
    entries = [{"path": path.relative_to(ROOT).as_posix(), **digest(path)}
               for path in sorted(ROOT.rglob("*"))
               if path.is_file() and path.name != "retained-input-index.json"
               and "__pycache__" not in path.parts]
    write("retained-input-index.json", {"schemaVersion": 1, "files": entries})
    print(json.dumps({"cases": {name: None if value is None else {
        key: value.get(key) for key in ["outcome", "stage", "operationsCompleted", "integerValuesCompared",
                                      "logitsBitwiseEqual", "stateBitwiseEqual", "ownedJobTeardown"]}
        for name, value in cases.items()}, "chargedSeconds": elapsed + clock["earlierReadOnlyPreparationChargeSeconds"]}))
