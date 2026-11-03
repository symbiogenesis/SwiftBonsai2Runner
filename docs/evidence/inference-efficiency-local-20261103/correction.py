"""Continue the same diagnostic allowance after a UTF-8 BOM command-file error."""
import datetime
import json
import os
from pathlib import Path
import time

out = Path(os.environ["TEMP"]) / "inference-efficiency-local-20261103"
report = json.loads((out / "receipt.json").read_text(encoding="utf-8"))
started = datetime.datetime.fromisoformat(report["startedAtUtc"])
# Preserve the earliest start through the harness correction, plus two extra
# conservative seconds. Neither the five-second setup charge nor cutoff changes.
elapsed = (datetime.datetime.now(datetime.UTC) - started).total_seconds() + 2
source = Path(__file__).with_name("driver.py").read_text(encoding="utf-8")
prefix = source.split('try:\n    run("format"', 1)[0]
prefix = prefix.replace("START = time.monotonic()", "START = ORIGINAL_START")
prefix = prefix.replace("OUT.mkdir(exist_ok=False)", "pass")
namespace = {"ORIGINAL_START": time.monotonic() - elapsed}
exec(compile(prefix, str(Path(__file__).with_name("driver.py")), "exec"), namespace)
namespace["REPORT"] = report
run, retain, power = (namespace[key] for key in ("run", "retain", "power"))
report["diagnosticCorrection"] = "PowerShell emitted a UTF-8 BOM in the follow-up command file. The driver stopped before launching the follow-up. Resume only that pending strict analyzer check using the original absolute start, including intervening time and two extra charged seconds; initial checks and failure remain retained."
report["originalAbsoluteDeadlineExtended"] = False
try:
    plan = json.loads((out / "followup.json").read_text(encoding="utf-8-sig"))
    for step in plan["steps"]:
        run(step["name"], step["arguments"], step.get("maximumSeconds", 45))
finally:
    report["powerAfter"] = power()
    report["finalRequestedChecksPassed"] = (
        all(check["status"] == "passed" for check in report["checks"][:5])
        and report["checks"][-1]["status"] == "passed"
    )
    retain()
    print(str(out / "receipt.json"), flush=True)
