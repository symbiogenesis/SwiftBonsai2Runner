"""One bounded, model-free live-save and window check; never restarts its clock."""
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


if sys.argv[1:] == ["--refresh-maintenance"]:
    source = Path(os.environ["CARGO_TARGET_DIR"]) / "aarch64-pc-windows-msvc/release/bonsai-tools.exe"
    target = Path(os.environ["LOCALAPPDATA"]) / "SwiftBonsai2/build/maintenance/release/bonsai-tools.exe"
    temporary = target.with_suffix(".chat-save.pending")
    try:
        shutil.copyfile(source, temporary)
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    print(json.dumps(dict(sha256=hashlib.sha256(target.read_bytes()).hexdigest(), target=str(target))))
    raise SystemExit(0)


ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
PLAN = json.loads((OUT / "plan.json").read_text())
START = time.monotonic()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                    ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                    ("life", ctypes.c_uint32), ("full", ctypes.c_uint32)]
    status = Status()
    known = bool(ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)))
    return dict(observedAtUtc=utc(), source="GetSystemPowerStatus",
                acConnected=(status.ac == 1) if known and status.ac in (0, 1) else None,
                batterySaver=(status.saver == 1) if known and status.saver in (0, 1) else None,
                chargePercent=status.charge if known and status.charge <= 100 else None,
                batteryFlags=status.flags if known and status.flags != 255 else None)


receipt = dict(kind=PLAN["kind"], startedAtUtc=utc(),
               maximumTotalSeconds=PLAN["maximumTotalSeconds"],
               powerBefore=power(), checks=[], status="running")
with (OUT / "clock.json").open("x") as stream:
    json.dump(receipt, stream, indent=2)

toolchain = Path(os.environ["USERPROFILE"]) / ".rustup/toolchains/nightly-2026-11-02-aarch64-pc-windows-msvc"
env = os.environ.copy()
env["PATH"] = str(toolchain / "bin") + os.pathsep + env["PATH"]
cached = Path(os.environ["LOCALAPPDATA"]) / "SwiftBonsai2/build/chat/cd31d71ca8da53bd5d98bc6d"
env["CARGO_TARGET_DIR"] = str(cached)
tools = Path(os.environ["TEMP"]) / "Bonsai-Codex-Tools"
env["CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER"] = str(
    Path(os.environ["USERPROFILE"]) / ".rustup/toolchains/nightly-2026-10-22-aarch64-pc-windows-msvc/lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe")
env["LIB"] = ";".join(str(tools / path) for path in (
    "arm64-crt-onecore/Contents/VC/Tools/MSVC/14.51.36231/lib/onecore/arm64",
    "arm64-crt/Contents/VC/Tools/MSVC/14.51.36231/lib/arm64", "sdk/lib/arm64"))
env["RC_PATH"] = str(tools / "sdk-buildtools/bin/10.0.26100.0/arm64/rc.exe")
cargo = str(toolchain / "bin/cargo.exe")
common = ["--locked", "--offline", "--release", "--target", "aarch64-pc-windows-msvc"]
checks = [("format", [str(toolchain / "bin/rustfmt.exe"), "--edition", "2024",
                       "--config", "skip_children=true", "src/chat-ui/src/recovery.rs",
                       "src/chat-ui/src/window/persistence.rs", "src/chat-ui/src/window.rs",
                       "src/chat-ui/src/window/placement.rs", "src/windows-credentials/src/sealed.rs",
                       "src/artifacts/src/lib.rs", "src/artifacts/tests/lifetime.rs"], None),
          ("artifact-sibling-and-file-guards", [cargo, "test", *common, "--package", "bonsai-artifacts", "--test", "lifetime", "file_guard_"], 2),
          ("ancestor-protection", [cargo, "test", *common, "--package", "bonsai-artifacts", "--test", "contained", "host_verification_denies_nested_ancestor_rename_and_delete_until_drop", "--", "--exact"], 1),
          ("dpapi-errors", [cargo, "test", *common, "--package", "bonsai-windows-credentials", "--lib", "sealed::tests::private_recovery_round_trip_and_tamper_rejection", "--", "--exact"], 1)]
for name, filter_name, count in (
    ("automatic-and-invalid", "window::persistence::tests::recovery_automatic", 2),
    ("opt-out-and-reenable", "window::persistence::tests::recovery_remembers_opt_out_and_explicit_reenable", 1),
    ("partial-history", "window::persistence::tests::recovery_preserves_drafts_without_promoting_partial_turns", 1),
    ("rapid-toggles", "window::persistence::tests::recovery_rapid_toggles_ignore_stale_erase_and_save_acknowledgments", 1),
    ("encrypted-storage", "recovery::tests::encrypted_recovery_", 3),
    ("window-placement", "window::placement::tests::window_placement_", 3),
):
    checks.append((name, [cargo, "test", *common, "--package", "bonsai-chat", "--bin", "bonsai-chat", filter_name], count))
checks.extend([
    ("strict-release-clippy", [cargo, "clippy", *common, "--package", "bonsai-chat", "--package", "bonsai-artifacts", "--package", "bonsai-windows-credentials", "--all-targets", "--", "-D", "warnings"], None),
    ("production-build", [cargo, "build", *common, "--package", "bonsai-chat", "--package", "bonsai-broker", "--package", "bonsai-tools"], None),
    ("refresh-default-maintenance", [sys.executable, str(Path(__file__).resolve()), "--refresh-maintenance"], None),
])

try:
    for name, command, expected in checks:
        remaining = PLAN["workCutoffSeconds"] - (time.monotonic() - START)
        if remaining <= 0:
            raise TimeoutError("Original work cutoff reached")
        check = dict(name=name, command=command, powerBefore=power(),
                     startedSeconds=time.monotonic() - START)
        receipt["checks"].append(check)
        print("Starting " + name, flush=True)
        with (OUT / (name + ".stdout.log")).open("w", encoding="utf-8") as stdout, (OUT / (name + ".stderr.log")).open("w", encoding="utf-8") as stderr:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr)
            try:
                code = process.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=10, check=False)
                process.wait(timeout=10)
                raise
        check.update(exitCode=code, powerAfter=power(), finishedSeconds=time.monotonic() - START)
        passed = code == 0
        if expected is not None:
            output = (OUT / (name + ".stdout.log")).read_text(encoding="utf-8")
            match = re.search(r"test result: ok\. (\d+) passed; (\d+) failed", output)
            check["expectedTests"] = expected
            passed = passed and bool(match and int(match[1]) == expected and int(match[2]) == 0)
        check["passed"] = passed
        print(name + (" passed" if passed else " failed"), flush=True)
        if not passed:
            raise RuntimeError("Check failed: " + name)
    receipt["status"] = "passed"
except Exception as error:
    receipt["status"] = "failed"
    receipt["error"] = str(error)
    if receipt["checks"] and "powerAfter" not in receipt["checks"][-1]:
        receipt["checks"][-1].update(powerAfter=power(), finishedSeconds=time.monotonic() - START, passed=False)
finally:
    receipt["sourceSha256"] = {
        path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
        for path in ("src/chat-ui/src/recovery.rs", "src/chat-ui/src/window/persistence.rs", "src/chat-ui/src/window.rs", "src/chat-ui/src/window/placement.rs", "src/windows-credentials/src/sealed.rs", "src/artifacts/src/lib.rs", "src/artifacts/tests/lifetime.rs", "src/chat-ui/Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
    }
    receipt["elapsedSeconds"] = time.monotonic() - START
    receipt["pendingChecks"] = [name for name, _, _ in checks if name not in [entry["name"] for entry in receipt["checks"]]]
    (OUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    retention = dict(powerAfter=power(), elapsedSeconds=time.monotonic() - START,
                     performanceComparisonEligible=False, continuousPowerStability="unknown")
    (OUT / "retention.json").write_text(json.dumps(retention, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(status=receipt["status"], elapsedSeconds=retention["elapsedSeconds"])), flush=True)
raise SystemExit(0 if receipt["status"] == "passed" else 1)
