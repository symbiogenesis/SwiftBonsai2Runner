"""One explicitly authorized final-source registry release; no historical retry."""

import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import urllib.error
import urllib.request


REVISION = "3bd63f675fd4a97a927e0e6abc11874089814d60"
VERSION = "0.1.23"
MAXIMUM = 540
WORK_CUTOFF = 500
TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-09-aarch64-pc-windows-msvc\bin")
GIT = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe\cmd\git.exe")
HELPERS = {"bonsai-ternary": "=0.1.2", "bonsai-prism": "=0.1.4",
           "bonsai-native-hash": "=0.1.2", "bonsai-neon-dot": "=0.1.1",
           "bonsai-gguf": "=0.1.4"}
REGISTRY_URL = "https://crates.io/api/v1/crates/prism-inference/0.1.23"


class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]


def power():
    value = Power()
    known = False
    try:
        query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
        query.argtypes = [ctypes.POINTER(Power)]
        query.restype = ctypes.c_int
        known = query(ctypes.byref(value)) != 0
    except (OSError, AttributeError):
        pass
    return {"source": "GetSystemPowerStatus", "utc": datetime.now(timezone.utc).isoformat(),
            "acConnected": bool(value.ac) if known and value.ac in (0, 1) else None,
            "batterySaver": bool(value.saver) if known and value.saver in (0, 1) else None,
            "chargePercent": value.charge if known and value.charge <= 100 else None}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--retain", type=Path, required=True)
    args = parser.parse_args()
    started = time.monotonic()
    args.evidence.mkdir(parents=True, exist_ok=False)
    receipt = {"schemaVersion": 1, "kind": "local-final-source-registry-release",
               "startedUtc": datetime.now(timezone.utc).isoformat(), "sourceRevision": REVISION,
               "maximumTotalSeconds": MAXIMUM, "workCutoffSeconds": WORK_CUTOFF,
               "driverSha256": digest(Path(__file__)),
               "declarationSha256": digest(Path(__file__).with_name("workload-plan.json")),
               "status": "failed", "steps": [], "powerSamples": [], "powerBefore": power(),
               "modelsExecuted": 0, "acceleratorDeviceCalls": 0, "benchmarksExecuted": 0,
               "fullSuitesExecuted": 0, "ciActions": 0, "pushes": 0,
               "publicationAttempted": False, "publicationOutcome": "not-attempted",
               "sourceChecksPassed": False, "normalizedPackageVerified": False}
    env = dict(os.environ)
    env["PATH"] = str(TOOLCHAIN) + os.pathsep + env["PATH"]
    env["CARGO_TARGET_DIR"] = str(Path(os.environ["TEMP"]) / "SwiftBonsai-PerformanceAdvance-20261109/target")
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_BUILD_JOBS"] = "4"
    env["RUSTUP_TOOLCHAIN"] = "nightly-2026-11-09-aarch64-pc-windows-msvc"
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    for name in ("GITHUB_TOKEN", "GH_TOKEN", "ACTIONS_RUNTIME_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN", "ACTIONS_RUNNER_INPUT_TOKEN"):
        env.pop(name, None)
    receipt["compilerCache"] = env["CARGO_TARGET_DIR"]

    def save():
        (args.evidence / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def remaining():
        return WORK_CUTOFF - (time.monotonic() - started)

    def run(name, command, cap, minimum=1, online=False):
        if remaining() < minimum:
            raise RuntimeError(f"Insufficient remaining allowance for {name}; no retry.")
        step = {"name": name, "arguments": [str(arg) for arg in command], "powerBefore": power(),
                "networkEnabled": online}
        receipt["steps"].append(step)
        log = args.evidence / f"{len(receipt['steps']):02d}-{name}.log"
        step["log"] = log.name
        stage_started = time.monotonic()
        process = None
        code = None
        print(f"Starting {name}", flush=True)
        try:
            child_env = dict(env)
            if online:
                child_env["CARGO_NET_OFFLINE"] = "false"
            with log.open("wb") as output:
                process = subprocess.Popen(step["arguments"], cwd=args.source, env=child_env,
                                           stdout=output, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
                stage_deadline = min(started + WORK_CUTOFF, stage_started + cap)
                while process.poll() is None:
                    receipt["powerSamples"].append(power())
                    if time.monotonic() >= stage_deadline:
                        step["timedOut"] = True
                        raise TimeoutError(f"{name} exceeded its original deadline")
                    try:
                        process.wait(timeout=min(1, max(0.001, stage_deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
                code = process.returncode
        finally:
            if process is not None and process.poll() is None:
                step["cleanupRequired"] = True
                cleanup = {"treeTerminationReported": False, "parentExited": False}
                step["cleanup"] = cleanup
                try:
                    killed = subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                            timeout=8, creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                    cleanup["taskkillExitCode"] = killed.returncode
                    cleanup["treeTerminationReported"] = killed.returncode == 0
                finally:
                    if process.poll() is None:
                        process.kill()
                    code = process.wait(timeout=5)
                    cleanup["parentExited"] = True
            step.update(exitCode=code, elapsedSeconds=time.monotonic() - stage_started, powerAfter=power())
            save()
        print(f"Finished {name}: exit {code}, {step['elapsedSeconds']:.3f}s", flush=True)
        if code != 0:
            raise RuntimeError(f"{name} failed with exit {code}; no retry.")
        return log.read_text(encoding="utf-8", errors="replace")

    def source_hashes():
        selected = subprocess.check_output([str(GIT), "ls-files", "-z"], cwd=args.source,
                                           timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
        return {name: digest(args.source / name)
                for name in selected.decode("utf-8").split("\0") if name}

    def registry(name, absent_allowed=False):
        if remaining() < 10:
            raise RuntimeError("Insufficient allowance for registry observation")
        before = power()
        try:
            request = urllib.request.Request(REGISTRY_URL, headers={
                "User-Agent": "SwiftBonsai2Runner owner-authorized source release (github.com/symbiogenesis/prism-inference)"})
            with urllib.request.urlopen(request, timeout=10) as response:
                result = json.load(response)["version"]
        except urllib.error.HTTPError as error:
            if absent_allowed and error.code == 404:
                result = None
            else:
                raise
        finally:
            receipt.setdefault("registryPower", []).append({"name": name, "before": before, "after": power()})
            save()
        receipt[name] = result
        return result

    try:
        save()
        head = run("source-head", [GIT, "rev-parse", "HEAD"], 10).strip()
        branch = run("source-branch", [GIT, "branch", "--show-current"], 10).strip()
        clean = run("source-clean", [GIT, "status", "--porcelain"], 10).strip()
        if head != REVISION or branch != "main" or clean:
            raise RuntimeError("Expected exact clean Prism local main")
        receipt["sourceSha256"] = source_hashes()
        receipt["compilerIdentity"] = run("compiler-identity", [TOOLCHAIN / "rustc.exe", "-Vv"], 10)
        existing = registry("registryBefore", absent_allowed=True)
        output = run("receipt-controls", [sys.executable, "-B", "-m", "unittest", "discover",
                    "-s", ".github/ci", "-p", "test_inference_efficiency.py", "-v"], 10)
        if "Ran 6 tests" not in output or "\nOK\n" not in output.replace("\r\n", "\n"):
            raise RuntimeError("All six named receipt controls are required")
        cargo = TOOLCHAIN / "cargo.exe"
        run("formatting", [cargo, "fmt", "--all", "--", "--check"], 10)
        packages = ["-p", "prism-inference", "-p", "prism-inference-server",
                    "-p", "bonsai-hexagon-native", "-p", "bonsai-hexagon-runtime"]
        run("strict-release-analysis", [cargo, "clippy", "--release", "--locked", "--offline",
            *packages, "--all-targets", "--features", "prism-inference/parallel-rows,prism-inference-server/parallel-rows",
            "--", "-D", "warnings"], 100, minimum=80)
        run("feature-disabled", [cargo, "check", "--release", "--locked", "--offline", "-p",
            "prism-inference", "--lib", "--no-default-features"], 30, minimum=20)
        run("normalized-package", [cargo, "package", "--locked", "--offline", "-p", "prism-inference"], 180, minimum=140)
        receipt["normalizedPackageVerified"] = True
        package = Path(env["CARGO_TARGET_DIR"]) / "package/prism-inference-0.1.23.crate"
        prefix = "prism-inference-0.1.23/"
        with tarfile.open(package, "r:gz") as archive:
            manifest = tomllib.loads(archive.extractfile(prefix + "Cargo.toml").read().decode("utf-8"))
            vcs = json.load(archive.extractfile(prefix + ".cargo_vcs_info.json"))
            if manifest["package"]["version"] != VERSION or vcs["git"]["sha1"] != REVISION or vcs["git"].get("dirty", False):
                raise RuntimeError("Normalized package version/VCS identity mismatch")
            dependency_tables = [manifest.get("dependencies", {})]
            dependency_tables.extend(table.get("dependencies", {}) for table in manifest.get("target", {}).values())
            normalized_helpers = {}
            for table in dependency_tables:
                for name, dependency in table.items():
                    if "path" in dependency or "git" in dependency:
                        raise RuntimeError("Normalized registry dependency retains a source override")
                    if name in HELPERS:
                        normalized_helpers[name] = dependency["version"]
            if normalized_helpers != HELPERS:
                raise RuntimeError("Normalized helper versions differ from unchanged published pins")
            receipt["packageAudit"] = {"version": VERSION, "sourceRevision": REVISION,
                                       "helperPins": normalized_helpers, "archiveMembers": len(archive.getmembers()),
                                       "sha256": digest(package), "bytes": package.stat().st_size}
        receipt["sourceChecksPassed"] = True
        if source_hashes() != receipt["sourceSha256"]:
            raise RuntimeError("Source changed during release checks")
        if run("source-clean-before-upload", [GIT, "status", "--porcelain"], 10).strip():
            raise RuntimeError("Source dirty before upload")
        if run("source-head-before-upload", [GIT, "rev-parse", "HEAD"], 10).strip() != REVISION:
            raise RuntimeError("Source revision changed before upload")
        if existing is None:
            if remaining() < 120:
                raise RuntimeError("Cannot fit publication and confirmation in original allowance")
            receipt["publicationAttempted"] = True
            receipt["publicationOutcome"] = "unknown-until-confirmed"
            save()
            run("publish-prism-inference", [cargo, "publish", "--locked", "--registry", "crates-io", "-p", "prism-inference"],
                110, minimum=120, online=True)
            receipt["publicationCommandPassed"] = True
            metadata = registry("registryAfter")
        else:
            metadata = existing
        if metadata["num"] != VERSION or metadata["checksum"] != receipt["packageAudit"]["sha256"]:
            raise RuntimeError("Public registry version/checksum does not match verified archive")
        receipt["publicationOutcome"] = "verified-version-and-checksum"
        receipt["publication"] = {"name": "prism-inference", "version": VERSION,
            "checksum": metadata["checksum"], "createdAt": metadata["created_at"],
            "registryUrl": "https://crates.io/crates/prism-inference/0.1.23"}
        receipt["status"] = "passed"
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
    finally:
        receipt["powerAfter"] = power()
        receipt["cleanupIncomplete"] = any(step.get("cleanupRequired") and not (
            step.get("cleanup", {}).get("treeTerminationReported") and step.get("cleanup", {}).get("parentExited"))
            for step in receipt["steps"])
        receipt["clockClosed"] = True
        receipt["clockRestarted"] = False
        receipt["retries"] = 0
        receipt["continuousPowerStabilityEstablished"] = False
        receipt["performanceComparisonQualified"] = False
        receipt["elapsedSecondsThroughChecks"] = time.monotonic() - started
        save()
        args.retain.mkdir(parents=True, exist_ok=False)
        for path in args.evidence.iterdir():
            if path.is_file():
                shutil.copyfile(path, args.retain / path.name)
        package = Path(env["CARGO_TARGET_DIR"]) / "package/prism-inference-0.1.23.crate"
        if receipt.get("packageAudit") and package.is_file():
            shutil.copyfile(package, args.retain / package.name)
        archive = {"files": {path.name: {"bytes": path.stat().st_size, "sha256": digest(path)}
                             for path in args.retain.iterdir() if path.is_file()}}
        (args.retain / "archive.json").write_text(json.dumps(archive, indent=2) + "\n", encoding="utf-8")
        clock = {"maximumTotalSeconds": MAXIMUM, "elapsedSecondsThroughRetention": time.monotonic() - started,
                 "powerAfterRetention": power(), "clockClosed": True, "retries": 0}
        clock["withinDeclaredAllowance"] = clock["elapsedSecondsThroughRetention"] <= MAXIMUM
        (args.retain / "final-clock.json").write_text(json.dumps(clock, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"status": receipt["status"], "failure": receipt.get("failure"),
                          "publicationOutcome": receipt["publicationOutcome"], **clock}), flush=True)
    return 0 if receipt["status"] == "passed" and not receipt["cleanupIncomplete"] and clock["withinDeclaredAllowance"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
