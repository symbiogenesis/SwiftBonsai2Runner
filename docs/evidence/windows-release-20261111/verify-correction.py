"""Check only corrected source and a previously blocked parser within the original deadline."""

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

from validate import power


here = Path(__file__).resolve().parent
original = json.loads((here / "summary.json").read_text(encoding="utf-8"))
start = datetime.fromisoformat(original["startedUtc"])
root = here.parents[2]
scratch = Path(original["retainedScratchDirectory"])
destination = here / "correction.json"
if destination.exists():
    raise RuntimeError("The source-correction check is already closed.")
receipt = {"schemaVersion": 1, "kind": "source-correction-within-original-static-allowance",
           "originalStartedUtc": original["startedUtc"], "maximumAggregateSeconds": 300,
           "newAllowanceStarted": False, "powerBefore": power(), "checks": [],
           "correction": "Move runner.temp from workflow defaults to individual step working directories.",
           "parserInvocationFix": "Use process-local ExecutionPolicy Bypass for a read-only AST parser.",
           "sourceCommitBeforeCorrection": original["sourceRevision"],
           "hostedBuildAndMsiAcceptance": "pending", "releasePublished": False}
try:
    commands = [
        ("actionlint-corrected", [scratch / "actionlint.exe", "-shellcheck=", "-pyflakes=",
                                  root / ".github/workflows/release.yml"]),
        ("powershell-parser-unblocked", ["powershell.exe", "-NoProfile", "-NonInteractive",
                                          "-ExecutionPolicy", "Bypass", "-File", scratch / "parse.ps1",
                                          "-Root", root, "-Blocks", scratch / "blocks"]),
    ]
    for name, command in commands:
        remaining = 270 - (datetime.now(timezone.utc) - start).total_seconds()
        if remaining <= 0:
            raise TimeoutError("Original deadline reached; no new allowance or retry.")
        result = subprocess.run([str(item) for item in command], cwd=root,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=min(30, remaining), check=False)
        (here / "raw" / f"{name}.log").write_bytes(result.stdout)
        receipt["checks"].append({"name": name, "exitCode": result.returncode,
                                   "passed": result.returncode == 0})
    receipt["finalWorkflowSha256"] = hashlib.sha256((root / ".github/workflows/release.yml").read_bytes()).hexdigest()
except Exception as error:
    receipt["failure"] = f"{type(error).__name__}: {error}"
finally:
    receipt["powerAfter"] = power()
    receipt["continuousPowerStabilityEstablished"] = False
    receipt["closed"] = True
    receipt["elapsedAggregateSecondsFromOriginalStart"] = round((datetime.now(timezone.utc) - start).total_seconds(), 6)
    receipt["withinOriginalAllowance"] = receipt["elapsedAggregateSecondsFromOriginalStart"] < 300
    receipt["correctedChecksPassed"] = ("failure" not in receipt and len(receipt["checks"]) == 2 and
                                         all(item["passed"] for item in receipt["checks"]))
    destination.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))
if not receipt["correctedChecksPassed"]:
    raise SystemExit(1)
