"""One fixed, offline updater check with an aggregate five-minute ceiling."""

import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


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
    return {"observedUtc": datetime.now(timezone.utc).isoformat(),
            "source": "GetSystemPowerStatus",
            "acConnected": bool(status.ac) if known and status.ac in (0, 1) else None,
            "batterySaver": bool(status.saver) if known and status.saver in (0, 1) else None,
            "chargePercent": status.charge if known and status.charge <= 100 else None}


def main():
    started = time.monotonic()
    here = Path(__file__).resolve().parent
    root = here.parents[2]
    destination = here / "summary.json"
    if destination.exists():
        raise RuntimeError("This fixed workload is already closed; no retry or clock restart.")
    plan = json.loads((here / "workload-plan.json").read_text(encoding="utf-8"))
    raw = here / "raw"
    raw.mkdir(exist_ok=False)
    scratch = Path(tempfile.mkdtemp(prefix="Bonsai-UpdateCheck-"))
    receipt = {"schemaVersion": 1, "kind": "offline-native-updater-validation",
               "startedUtc": datetime.now(timezone.utc).isoformat(),
               "maximumAggregateSeconds": 300, "powerBefore": power(),
               "checks": [], "comparisonArms": [], "performanceQualified": False,
               "releasePublished": False, "msiExecuted": False,
               "existingInstalledExecutablesReplaced": False,
               "scratchDirectory": str(scratch)}
    git = Path(r"C:\Program Files\Microsoft Visual Studio\18\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe")
    cargo = Path(os.environ["USERPROFILE"]) / ".cargo/bin/cargo.exe"
    rustfmt = Path(os.environ["USERPROFILE"]) / ".rustup/toolchains/nightly-2026-11-09-aarch64-pc-windows-msvc/bin/rustfmt.exe"
    rumdl = Path(os.environ["USERPROFILE"]) / ".cargo/bin/rumdl.exe"
    environment = os.environ.copy()
    environment["CARGO_BUILD_JOBS"] = "4"
    environment["CARGO_UNSTABLE_CHECKSUM_FRESHNESS"] = "true"
    environment["CARGO_BUILD_FINGERPRINT"] = "content"

    def check(name, command, limit=15):
        remaining = 270 - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError("Stop before the original aggregate deadline.")
        print(f"Starting {name}", flush=True)
        log = raw / f"{name}.log"
        timed_out = False
        with log.open("wb") as output:
            process = subprocess.Popen([str(item) for item in command],
                                       cwd=scratch if Path(command[0]) == cargo else root,
                                       stdout=output, stderr=subprocess.STDOUT, env=environment)
            try:
                code = process.wait(timeout=min(limit, remaining))
            except subprocess.TimeoutExpired:
                timed_out = True
                subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"),
                                "/PID", str(process.pid), "/T", "/F"],
                               stdout=output, stderr=subprocess.STDOUT, timeout=15, check=False)
                code = process.wait(timeout=10)
        receipt["checks"].append({"name": name, "exitCode": code,
                                   "timedOut": timed_out, "passed": code == 0 and not timed_out})
        print(f"Finished {name}: {code}" + (" (timed out)" if timed_out else ""), flush=True)
        return log.read_text(encoding="utf-8", errors="replace").strip()

    try:
        receipt["sourceRevision"] = check("source-revision", [git, "-C", root, "rev-parse", "HEAD"])
        changed = check("changed-rust-paths", [git, "-C", root, "diff", "--name-only", "8d1177c0", "HEAD"])
        paths = [root / name for name in changed.splitlines() if name.endswith(".rs")]
        receipt["sourceSha256"] = {str(path.relative_to(root)).replace("\\", "/"):
                                   hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
        check("rustfmt", [rustfmt, "--check", "--edition", "2024", "--config",
                          "skip_children=true", *paths])
        docs = [root / name for name in ("README.md", "PLAN.md", "docs/releases.md",
                "docs/releases/v0.1.0.md", "docs/chat-packages.md", "docs/ui-architecture.md")]
        check("markdown-check", [rumdl, "check", "--no-cache", *docs])
        check("markdown-format", [rumdl, "fmt", "--check", "--no-cache", *docs])
        check("diff-check", [git, "-C", root, "diff", "--check", "8d1177c0", "HEAD"])
        common = [cargo, "+nightly-2026-11-09", "test", "--offline", "--locked", "--release",
                  "--manifest-path", root / "Cargo.toml", "--package", "bonsai-tools", "--lib",
                  "--target-dir", scratch / "target"]
        check("updater-fixtures", [*common, "app_update::"], plan["constraints"]["coreTestMaximumSeconds"])
        first = receipt["checks"][-1]
        if first["passed"]:
            check("acknowledged-handoff", [*common,
                  "chat_launch::tests::installer_handoff_requires_acknowledged_update_exit_and_ignores_ordinary_close",
                  "--", "--exact"], 15)
        else:
            receipt["checks"].append({"name": "acknowledged-handoff", "passed": False,
                                       "notRun": "core compilation or fixtures failed; no retry"})
        if first["timedOut"]:
            receipt["checks"].append({"name": "native-ui-check", "passed": False,
                                       "notRun": "core compile did not fit; keep remaining compilation on hosted CI"})
        else:
            check("native-ui-check", [cargo, "+nightly-2026-11-09", "check", "--offline", "--locked",
                  "--release", "--manifest-path", root / "Cargo.toml", "--package", "bonsai-chat",
                  "--bin", "bonsai-chat", "--target-dir", scratch / "target"],
                  plan["constraints"]["nativeUiCheckMaximumSeconds"])
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
    finally:
        receipt["powerAfter"] = power()
        receipt["continuousPowerStabilityEstablished"] = False
        receipt["closed"] = True
        receipt["elapsedSecondsThroughRetention"] = round(time.monotonic() - started, 6)
        receipt["withinAllowance"] = receipt["elapsedSecondsThroughRetention"] < 300
        receipt["allSelectedChecksPassed"] = ("failure" not in receipt and
                                               all(item["passed"] for item in receipt["checks"]))
        destination.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({key: receipt[key] for key in ("allSelectedChecksPassed", "elapsedSecondsThroughRetention", "powerBefore", "powerAfter")}))
    return 0 if receipt["allSelectedChecksPassed"] else 1


if __name__ == "__main__":
    sys.exit(main())
