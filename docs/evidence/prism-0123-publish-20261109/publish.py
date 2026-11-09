"""One bounded registry publication after exact-source hosted bundle acceptance."""

import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import time
import tomllib
import urllib.error
import urllib.request


REVISION = "a7cdee96080d6cc33fb6a22b62ede94117150b9b"
VERSION = "0.1.23"
MAXIMUM, WORK_CUTOFF = 300, 270
TOOLCHAIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-09-aarch64-pc-windows-msvc\bin")
GIT = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe\cmd\git.exe")
HELPERS = {"bonsai-ternary": "=0.1.2", "bonsai-prism": "=0.1.4",
           "bonsai-native-hash": "=0.1.2", "bonsai-neon-dot": "=0.1.1", "bonsai-gguf": "=0.1.4"}
REGISTRY_URL = "https://crates.io/api/v1/crates/prism-inference/0.1.23"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                    ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                    ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]
    value, known = Status(), False
    try:
        query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
        query.argtypes, query.restype = [ctypes.POINTER(Status)], ctypes.c_int
        known = query(ctypes.byref(value)) != 0
    except (OSError, AttributeError):
        pass
    return {"source": "GetSystemPowerStatus", "utc": datetime.now(timezone.utc).isoformat(),
            "acConnected": bool(value.ac) if known and value.ac in (0, 1) else None,
            "batterySaver": bool(value.saver) if known and value.saver in (0, 1) else None,
            "chargePercent": value.charge if known and value.charge <= 100 else None}


def observe_registry():
    """Runs in an owned child so the parent's deadline also bounds HTTP reads."""
    request = urllib.request.Request(REGISTRY_URL, headers={
        "User-Agent": "SwiftBonsai2Runner owner-authorized registry delivery (github.com/symbiogenesis/prism-inference)"})
    try:
        with urllib.request.urlopen(request, timeout=8) as response:
            body = response.read(512 * 1024 + 1)
        if len(body) > 512 * 1024:
            raise ValueError("Unexpectedly large registry response")
        metadata = json.loads(body)["version"]
        value = {key: metadata[key] for key in ("num", "checksum", "created_at", "yanked")}
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        value = None
    print(json.dumps(value))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "evidence", "retain", "operator-receipt", "operator-clock"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    started = time.monotonic()
    args.evidence.mkdir(parents=True, exist_ok=False)
    driver, declaration = Path(__file__).resolve(), Path(__file__).with_name("workload-plan.json")
    receipt = {"schemaVersion": 1, "kind": "hosted-accepted-source-registry-publication",
               "sourceRevision": REVISION, "version": VERSION, "startedUtc": datetime.now(timezone.utc).isoformat(),
               "maximumTotalSeconds": MAXIMUM, "workCutoffSeconds": WORK_CUTOFF,
               "driverSha256": digest(driver), "declarationSha256": digest(declaration),
               "status": "failed", "steps": [], "powerSamples": [], "powerBefore": power(),
               "publicationAttempted": False, "publicationOutcome": "not-attempted",
               "normalizedPackageVerified": False, "modelsExecuted": 0, "benchmarksExecuted": 0,
               "fullSuitesExecuted": 0, "localSourceChecksExecuted": 0, "ciActions": 0, "pushes": 0}
    env = dict(os.environ)
    env.update(PATH=str(TOOLCHAIN) + os.pathsep + env["PATH"], CARGO_NET_OFFLINE="true",
               CARGO_BUILD_JOBS="4", RUSTUP_TOOLCHAIN="nightly-2026-11-09-aarch64-pc-windows-msvc",
               PYTHONDONTWRITEBYTECODE="1",
               CARGO_TARGET_DIR=str(Path(os.environ["TEMP"]) / "SwiftBonsai-PerformanceAdvance-20261109/target"))
    for name in ("GITHUB_TOKEN", "GH_TOKEN", "ACTIONS_RUNTIME_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN", "ACTIONS_RUNNER_INPUT_TOKEN"):
        env.pop(name, None)
    receipt["compilerCache"] = env["CARGO_TARGET_DIR"]

    def save():
        (args.evidence / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def remaining():
        return WORK_CUTOFF - (time.monotonic() - started)

    def run(name, command, cap, minimum=1, online=False):
        if remaining() < minimum:
            raise RuntimeError(f"Insufficient original allowance for {name}; no retry")
        step = {"name": name, "arguments": [str(item) for item in command], "networkEnabled": online,
                "powerBefore": power(), "exitCode": None}
        receipt["steps"].append(step)
        log = args.evidence / f"{len(receipt['steps']):02d}-{name}.log"
        step["log"] = log.name
        began, child = time.monotonic(), None
        print(f"Starting {name}", flush=True)
        try:
            child_env = dict(env)
            if online:
                child_env["CARGO_NET_OFFLINE"] = "false"
            with log.open("wb") as output:
                child = subprocess.Popen(step["arguments"], cwd=args.source, env=child_env,
                                         stdout=output, stderr=subprocess.STDOUT,
                                         creationflags=subprocess.CREATE_NO_WINDOW)
                deadline = min(started + WORK_CUTOFF, began + cap)
                while child.poll() is None:
                    receipt["powerSamples"].append(power())
                    if time.monotonic() >= deadline:
                        step["timedOut"] = True
                        raise TimeoutError(f"{name} reached its original deadline")
                    try:
                        child.wait(timeout=min(1, max(0.001, deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
                step["exitCode"] = child.returncode
        finally:
            if child is not None and child.poll() is None:
                cleanup = {"treeTerminationReported": False, "parentExited": False}
                step["cleanup"] = cleanup
                try:
                    killed = subprocess.run(["taskkill.exe", "/PID", str(child.pid), "/T", "/F"],
                                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                            timeout=8, creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                    cleanup["treeTerminationReported"] = killed.returncode == 0
                    cleanup["taskkillExitCode"] = killed.returncode
                except (OSError, subprocess.TimeoutExpired) as error:
                    cleanup["failureKind"] = type(error).__name__
                finally:
                    if child.poll() is None:
                        child.kill()
                    try:
                        step["exitCode"] = child.wait(timeout=5)
                        cleanup["parentExited"] = True
                    except subprocess.TimeoutExpired:
                        cleanup["parentExitTimedOut"] = True
            step.update(elapsedSeconds=time.monotonic() - began, powerAfter=power())
            save()
        if step["exitCode"] != 0:
            raise RuntimeError(f"{name} exited {step['exitCode']}; no retry")
        print(f"Finished {name}: {step['elapsedSeconds']:.3f}s", flush=True)
        return log.read_text(encoding="utf-8", errors="replace")

    def source_hashes():
        names = run("tracked-inputs", [GIT, "ls-files", "-z"], 5)
        return {name: digest(args.source / name) for name in names.split("\0") if name}

    def clean_source():
        if (run("source-head", [GIT, "rev-parse", "HEAD"], 5).strip() != REVISION
                or run("source-branch", [GIT, "branch", "--show-current"], 5).strip() != "main"
                or run("source-clean", [GIT, "status", "--porcelain"], 5).strip()):
            raise RuntimeError("Expected exact clean Prism local main")

    def registry(name):
        value = json.loads(run(name, [sys.executable, "-B", driver, "--observe-registry"], 10, minimum=10, online=True))
        receipt[name] = value
        save()
        return value

    try:
        save()
        if args.evidence.resolve().is_relative_to(args.source.resolve()) or args.retain.resolve().is_relative_to(args.source.resolve()):
            raise RuntimeError("Evidence must be outside the frozen Prism source checkout")
        shutil.copyfile(driver, args.evidence / "publish.py")
        shutil.copyfile(declaration, args.evidence / "workload-plan.json")
        operator_bytes, clock_bytes = args.operator_receipt.read_bytes(), args.operator_clock.read_bytes()
        accepted, clock = json.loads(operator_bytes), json.loads(clock_bytes)
        required = {"passed": True, "sourceRevision": REVISION, "version": VERSION,
                    "workflowFile": "foundation-bundle.yml", "runConclusion": "success",
                    "finalSourceChecksPassed": True, "productionLinkingPassed": True,
                    "fullFoundationQualified": False}
        if any(accepted.get(key) != value or (isinstance(value, bool) and accepted.get(key) is not value)
               for key, value in required.items()):
            raise RuntimeError("Exact-source hosted bundle acceptance is missing or invalid")
        for key in ("runId", "runAttempt"):
            value = accepted.get(key)
            if isinstance(value, bool) or re.fullmatch(r"[0-9]+", str(value)) is None or int(value) <= 0:
                raise RuntimeError("A genuine positive hosted run/attempt identity is required")
        elapsed = clock.get("elapsedSecondsThroughRetention")
        if (clock.get("clockClosed") is not True or clock.get("withinDeclaredAllowance") is not True
                or clock.get("retentionComplete") is not True
                or clock.get("cleanupConfirmed") is not True or clock.get("maximumTotalSeconds") != 300
                or clock.get("cleanupIncomplete", False) is not False
                or not isinstance(elapsed, (int, float)) or isinstance(elapsed, bool)
                or not math.isfinite(elapsed) or not 0 <= elapsed <= 300):
            raise RuntimeError("Hosted operator clock or cleanup is not accepted")
        receipt["operatorInputSha256"] = {"verified-bundle.json": hashlib.sha256(operator_bytes).hexdigest(),
                                          "operator-final-clock.json": hashlib.sha256(clock_bytes).hexdigest()}
        (args.evidence / "verified-bundle.json").write_bytes(operator_bytes)
        (args.evidence / "operator-final-clock.json").write_bytes(clock_bytes)
        receipt["hostedAcceptance"] = {key: accepted[key] for key in (*required, "runId", "runAttempt")}
        clean_source()
        receipt["sourceSha256"] = source_hashes()
        receipt["compilerIdentity"] = run("compiler-identity", [TOOLCHAIN / "rustc.exe", "-Vv"], 5)
        before = registry("registryBefore")
        cargo = TOOLCHAIN / "cargo.exe"
        # Admit the whole remaining fixed sequence, not a package build that
        # already cannot leave its conservative audit/upload/confirmation time.
        required_remaining = 250 if before is None else 150
        run("normalized-package", [cargo, "package", "--locked", "--offline", "-p", "prism-inference"],
            140, minimum=required_remaining)
        package = Path(env["CARGO_TARGET_DIR"]) / "package/prism-inference-0.1.23.crate"
        prefix = "prism-inference-0.1.23/"
        with tarfile.open(package, "r:gz") as archive:
            manifest = tomllib.loads(archive.extractfile(prefix + "Cargo.toml").read().decode("utf-8"))
            vcs = json.load(archive.extractfile(prefix + ".cargo_vcs_info.json"))
            if (manifest["package"]["name"] != "prism-inference" or manifest["package"]["version"] != VERSION
                    or "workspace" in manifest or vcs["git"]["sha1"] != REVISION or vcs["git"].get("dirty", False) is not False):
                raise RuntimeError("Normalized package identity or clean VCS binding differs")
            groups = [manifest, *manifest.get("target", {}).values()]
            helpers = {}
            for group in groups:
                for section in ("dependencies", "build-dependencies", "dev-dependencies"):
                    for name, dependency in group.get(section, {}).items():
                        if any(key in dependency for key in ("path", "git", "workspace")):
                            raise RuntimeError("A normalized registry dependency retains a checkout override")
                        if name in HELPERS:
                            helpers[name] = dependency["version"]
            if helpers != HELPERS:
                raise RuntimeError("Normalized exact registry helper pins differ")
            receipt["packageAudit"] = {"version": VERSION, "sourceRevision": REVISION, "helperPins": helpers,
                                       "archiveMembers": len(archive.getmembers()), "sha256": digest(package),
                                       "bytes": package.stat().st_size}
        shutil.copyfile(package, args.evidence / package.name)
        receipt["normalizedPackageVerified"] = True
        clean_source()
        if (source_hashes() != receipt["sourceSha256"] or digest(driver) != receipt["driverSha256"]
                or digest(declaration) != receipt["declarationSha256"]
                or digest(args.operator_receipt) != receipt["operatorInputSha256"]["verified-bundle.json"]
                or digest(args.operator_clock) != receipt["operatorInputSha256"]["operator-final-clock.json"]):
            raise RuntimeError("Source or immutable publication inputs changed")
        if before is None:
            if remaining() < 100:
                raise RuntimeError("Cannot fit publication and confirmation in the original allowance")
            receipt.update(publicationAttempted=True, publicationOutcome="unknown-until-confirmed")
            save()
            run("publish-prism-inference", [cargo, "publish", "--locked", "--registry", "crates-io", "-p", "prism-inference"],
                90, minimum=100, online=True)
            receipt["publicationCommandPassed"] = True
            metadata = registry("registryAfter")
        else:
            metadata = before
        if (metadata is None or metadata.get("num") != VERSION or metadata.get("yanked") is not False
                or metadata.get("checksum") != receipt["packageAudit"]["sha256"]):
            raise RuntimeError("Immutable public version/checksum differs from the verified archive")
        receipt.update(status="passed", publicationOutcome="verified-version-and-checksum",
                       publication={"name": "prism-inference", "version": VERSION,
                                    "checksum": metadata["checksum"], "createdAt": metadata["created_at"],
                                    "url": "https://crates.io/crates/prism-inference/0.1.23"})
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
    finally:
        receipt.update(powerAfter=power(), clockClosed=True, clockRestarted=False, retries=0,
                       continuousPowerStabilityEstablished=False, fullFoundationQualified=False,
                       performanceQualified=False, elapsedSecondsThroughWork=time.monotonic() - started)
        receipt["cleanupConfirmed"] = all(item.get("cleanup", {}).get("treeTerminationReported") is True
                                           and item["cleanup"].get("parentExited") is True
                                           for item in receipt["steps"] if "cleanup" in item)
        save()
        args.retain.mkdir(parents=True, exist_ok=False)
        for path in args.evidence.iterdir():
            if path.is_file():
                shutil.copyfile(path, args.retain / path.name)
        inventory = {path.name: {"bytes": path.stat().st_size, "sha256": digest(path)}
                     for path in args.retain.iterdir() if path.is_file()}
        (args.retain / "archive.json").write_text(json.dumps({"files": inventory}, indent=2) + "\n", encoding="utf-8")
        clock = {"maximumTotalSeconds": MAXIMUM, "elapsedSecondsThroughRetention": time.monotonic() - started,
                 "powerAfterRetention": power(), "clockClosed": True, "retries": 0,
                 "cleanupConfirmed": receipt["cleanupConfirmed"],
                 "scope": "Includes raw evidence and archive manifest; excludes writing this final small clock receipt."}
        clock["withinDeclaredAllowance"] = clock["elapsedSecondsThroughRetention"] <= MAXIMUM
        (args.retain / "final-clock.json").write_text(json.dumps(clock, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"status": receipt["status"], "failure": receipt.get("failure"),
                          "publicationOutcome": receipt["publicationOutcome"], **clock}), flush=True)
    return 0 if receipt["status"] == "passed" and receipt["cleanupConfirmed"] and clock["withinDeclaredAllowance"] else 1


if __name__ == "__main__":
    if sys.argv[1:] == ["--observe-registry"]:
        observe_registry()
    else:
        raise SystemExit(main())
