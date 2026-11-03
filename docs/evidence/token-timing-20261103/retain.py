"""Finish selected archival within the capture's unchanged original allowance."""
import hashlib
import json
import os
from pathlib import Path
import time
from capture import ROOT, model_processes, power, read_json, save, utc

session = read_json(ROOT/"session.json")
if (ROOT/"retention.json").exists():
    raise SystemExit("Retention already closed; never repeat the trial")
if time.monotonic() >= session["startedMonotonic"]+session["maximumSeconds"]:
    raise SystemExit("Original allowance is closed")
report = dict(startedUtc=utc(),originalStartedMonotonic=session["startedMonotonic"],
              originalMaximumSeconds=session["maximumSeconds"],modelAttemptsAdded=0,
              powerBeforeFinalArchive=power())
report["finalModelProcessScan"] = model_processes()
scratch = Path(session["scratchDirectory"]).resolve()
report["remainingTrialCredentialFiles"] = len(list(scratch.rglob("*.key")))
helper = Path(os.environ["LOCALAPPDATA"])/"SwiftBonsai2"/"build"/"chat"/"cd31d71ca8da53bd5d98bc6d"/"aarch64-pc-windows-msvc"/"release"/"examples"/"token_timing_owner.exe"
contents = helper.read_bytes()
digest = hashlib.sha256(contents).hexdigest()
if digest != session["helperSha256"]:
    raise RuntimeError("Compiled owner changed before evidence retention")
archive = Path(os.environ["LOCALAPPDATA"])/"SwiftBonsai2"/"qualification"/"token-timing-20261103"
archive.mkdir(parents=True,exist_ok=True)
binary = archive/(digest+".exe")
if binary.exists():
    if hashlib.sha256(binary.read_bytes()).hexdigest() != digest:
        raise RuntimeError("Existing retained owner digest mismatch")
else:
    with binary.open("xb") as output:
        output.write(contents)
report["retainedOwner"] = dict(path=str(binary),bytes=len(contents),sha256=digest)
source = ROOT/"compiled-owner-source.rs"
if hashlib.sha256(source.read_bytes()).hexdigest() != session["helperSourceSha256"]:
    raise RuntimeError("Compiled owner source archive digest mismatch")
manifest = []
for path in sorted(ROOT.iterdir()):
    if path.is_file() and path.name not in {"retention.json","retention.json.partial"}:
        contents = path.read_bytes()
        manifest.append(dict(path=path.name,bytes=len(contents),sha256=hashlib.sha256(contents).hexdigest()))
report["retainedFiles"] = manifest
report["powerAfterFinalRetention"] = power()
report["elapsedSecondsThroughFinalRetention"] = time.monotonic()-session["startedMonotonic"]
report["withinOriginalAllowance"] = report["elapsedSecondsThroughFinalRetention"] <= session["maximumSeconds"]
report["closedUtc"] = utc()
save("retention.json",report)
print(json.dumps({key:report[key] for key in ["finalModelProcessScan","remainingTrialCredentialFiles","elapsedSecondsThroughFinalRetention","withinOriginalAllowance","powerAfterFinalRetention"]},indent=2))
