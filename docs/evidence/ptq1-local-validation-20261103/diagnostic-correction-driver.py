"""Necessary diagnostic corrections within the original absolute allowance."""
import datetime
import json
import os
from pathlib import Path
import re
import time

original = Path(os.environ["TEMP"]) / "ptq1-local-validation-20261103.py"
source = original.read_text(encoding="utf-8")
out = Path(os.environ["TEMP"]) / "ptq1-local-validation-20261103"
report = json.loads((out / "receipt.json").read_text(encoding="utf-8"))
started = datetime.datetime.fromisoformat(report["startedAtUtc"])
elapsed = (datetime.datetime.now(datetime.UTC) - started).total_seconds() + 2
prefix = source.split("try:\n    cargo = TOOLCHAIN", 1)[0]
prefix = prefix.replace("START = time.monotonic()", "START = time.monotonic() - ORIGINAL_ELAPSED")
prefix = prefix.replace("OUT.mkdir(exist_ok=False)", "pass")
namespace = {"ORIGINAL_ELAPSED": elapsed}
exec(compile(prefix, str(original), "exec"), namespace)
namespace["REPORT"] = report
report["diagnosticCorrection"] = "The first driver used the old deps binary location; current Cargo put the test executable under build/<package>/<hash>/out. Follow-up preserves the earliest original UTC deadline with two additional conservative seconds, including intervening time. No model or matrix was run."
run, retain, power = (namespace[key] for key in ("run", "retain", "power"))
cargo = namespace["TOOLCHAIN"] / "cargo.exe"
try:
    run("archive-controls-directory-sharing-fix", [cargo, "test", "--locked", "--offline", "--release", "-p", "bonsai-tools", "--lib", "evidence_archive::tests::", "--", "--test-threads=1"], 35)
    log = (out / "service-stream-failure.log").read_text(encoding="utf-8")
    binary = re.search(r"\(([^\r\n]*native_model-[^\r\n]*\.exe)\)", log).group(1)
    for name, case in [
        ("service-boundary-failure", "adapter_boundaries::native_adapter_boundary_wire_sequence_preserves_rejections_and_failed_replay"),
        ("service-prefix-terminal", "service_integration::native_service_integration_prefix_rejects_terminal_events_already_received"),
        ("service-cancellation-failure", "service_integration::native_service_integration_cancellation_failures_return_without_unwinding")]:
        run(name, [binary, case, "--exact"], 5)
    run("archive-release-clippy-fixed", [cargo, "clippy", "--locked", "--offline", "--release", "-p", "bonsai-tools", "--lib", "--", "-D", "warnings"], 35)
    run("service-release-clippy", [cargo, "clippy", "--locked", "--offline", "--release", "-p", "bonsai-broker", "--test", "native_model", "--", "-D", "warnings"], 25)
finally:
    report["powerAfter"] = power()
    report["allRequestedChecksPassed"] = False
    report["originalDiagnosticFailurePreserved"] = True
    report["originalAbsoluteDeadlineExtended"] = False
    retain()
    print(str(out / "receipt.json"), flush=True)
