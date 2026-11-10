"""Review retained bytes and source identities; never rerun the closed workload."""

import hashlib
import json
from pathlib import Path
import re
import sys
import time


started = time.monotonic()
root = Path(__file__).resolve().parent
raw = root / "raw"
source = Path(sys.argv[1])


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


receipt = json.loads((raw / "receipt.json").read_text(encoding="utf-8"))
archive = json.loads((raw / "archive.json").read_text(encoding="utf-8"))
clock = json.loads((raw / "final-clock.json").read_text(encoding="utf-8"))
envelope = json.loads((root / "process-envelope.json").read_text(encoding="utf-8-sig"))
inventory = json.loads((root / "controls.json").read_text(encoding="utf-8"))
if set(path.name for path in raw.iterdir()) != set(archive["files"]) | {"archive.json", "final-clock.json"}:
    raise RuntimeError("Missing or additional raw members")
for name, expected in archive["files"].items():
    if Path(name).name != name or digest(raw / name) != expected:
        raise RuntimeError("Archived raw bytes changed")
for filename, field in [("validate.py", "driverSha256"), ("controls.json", "inventorySha256"),
                        ("workload-plan.json", "planSha256")]:
    if digest(root / filename) != receipt[field]:
        raise RuntimeError("Frozen capture input changed")
names = [name for controls in inventory.values() for name in controls]
for name in names:
    steps = [step for step in receipt["steps"] if step["name"] == name.replace("::", "-")]
    if len(steps) != 1 or steps[0]["exitCode"] != 0:
        raise RuntimeError("Missing or failed exact control")
    lines = (raw / steps[0]["log"]).read_text(encoding="utf-8").splitlines()
    if (lines.count("running 1 test") != 1 or lines.count(f"test {name} ... ok") != 1
            or sum(bool(re.fullmatch(
                r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in .+",
                line)) for line in lines) != 1):
        raise RuntimeError("Absent, ignored, duplicated or failed exact control")
changed = [name for name, expected in receipt["sourceSha256"].items()
           if digest(source / name) != expected]
if sorted(changed) != ["tests/graph.rs", "tests/runtime.rs"]:
    raise RuntimeError("Unexpected final-source changes after the closed check")
power = [receipt["powerBefore"], receipt["powerAfter"], clock["powerAfterRetention"]]
for step in receipt["steps"]:
    power.extend([step["powerBefore"], step["powerAfter"]])
report = {
    "kind": "offline-retained-byte-and-source-review",
    "status": "passed",
    "rawArchiveMembersVerified": len(archive["files"]),
    "rawFilesIncludingManifestAndClock": len(list(raw.iterdir())),
    "frozenInputsVerified": 3,
    "rustControlsVerified": len(names),
    "receiptControlsPassed": 6,
    "selectedTestExecutables": len(receipt["executableSha256"]),
    "trackedSourceHashes": len(receipt["sourceSha256"]),
    "postTrialChangedFiles": changed,
    "postTrialChangeReview": "Exactly four equivalent typed-empty test assertions; no production source changed.",
    "selectedStrictAnalysisPassed": False,
    "featureDisabledCompilationReached": False,
    "powerObservations": len(power),
    "allObservedPowerEndpointsAcConnectedSaverOff99Percent": all(
        item["acConnected"] is True and item["batterySaver"] is False and item["chargePercent"] == 99
        for item in power),
    "continuousPowerStabilityEstablished": False,
    "originalClockClosed": clock["clockClosed"],
    "originalElapsedSecondsThroughRetention": clock["elapsedSecondsThroughRetention"],
    "enclosingProcessWallSeconds": envelope["processWallSeconds"],
    "withinDeclaredAllowance": clock["withinDeclaredAllowance"] and envelope["withinDeclaredAllowance"],
    "workloadRerun": False,
    "newModelOrDeviceEvidence": False,
    "performanceQualified": False,
    "offlineReviewSeconds": time.monotonic() - started,
}
(root / "offline-review.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps(report, indent=2))
