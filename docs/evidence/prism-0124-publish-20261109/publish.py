"""One fixed, five-minute Prism 0.1.24 registry operation; no retries."""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import io
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

BASE = Path(__file__).resolve().parent
SOURCE = Path(r"C:\Users\symbi\source\repos\prism-inference")
GIT = Path(r"C:\Program Files\Microsoft Visual Studio\18\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe")
BIN = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-09-aarch64-pc-windows-msvc\bin")
VERSION = "0.1.24"
HELPERS = {"bonsai-ternary": ("0.1.2", "ternary"), "bonsai-prism": ("0.1.4", "prism"),
           "bonsai-native-hash": ("0.1.2", "native-hash"), "bonsai-neon-dot": ("0.1.1", "neon-dot"),
           "bonsai-gguf": ("0.1.4", "gguf")}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                    ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                    ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]
    result = {"utc": datetime.now(timezone.utc).isoformat(), "source": "GetSystemPowerStatus",
              "acConnected": None, "batterySaver": None, "chargePercent": None}
    try:
        status = Status()
        if ctypes.WinDLL("kernel32").GetSystemPowerStatus(ctypes.byref(status)):
            result.update(acConnected=bool(status.ac) if status.ac in (0, 1) else None,
                          batterySaver=bool(status.saver) if status.saver in (0, 1) else None,
                          chargePercent=status.charge if status.charge <= 100 else None)
    except (OSError, AttributeError):
        pass
    return result


def fetch(url, destination):
    request = urllib.request.Request(url, headers={"User-Agent": "SwiftBonsai2Runner owner-authorized dependency publication"})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            data = response.read(32 * 1024 * 1024 + 1)
        if len(data) > 32 * 1024 * 1024:
            raise ValueError("Registry response exceeds fixed size cap")
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        data = b"null\n"
    Path(destination).write_bytes(data)


def main():
    original_start = datetime.fromisoformat(json.loads((BASE / "workload-plan.json").read_text(encoding="utf-8-sig"))["originalStartedUtc"])
    started = time.monotonic() - (datetime.now(timezone.utc) - original_start).total_seconds()
    evidence = BASE / "raw"
    evidence.mkdir(exist_ok=False)
    declaration = json.loads((BASE / "workload-plan.json").read_text(encoding="utf-8-sig"))
    receipt = {"schemaVersion": 1, "kind": "prism-0124-registry-publication", "version": VERSION,
               "startedUtc": datetime.now(timezone.utc).isoformat(), "maximumTotalSeconds": 300,
               "workCutoffSeconds": 270, "estimatedSeconds": 240, "status": "failed",
               "powerBefore": power(), "steps": [], "powerSamples": [], "helpers": [],
               "publicationAttempted": False, "publicationOutcome": "not-attempted",
               "modelsExecuted": 0, "benchmarksExecuted": 0, "fullSuitesExecuted": 0,
               "ciActions": 0, "pushes": 0, "retries": 0}
    env = dict(os.environ)
    env.update(PATH=str(BIN) + os.pathsep + str(GIT.parent) + os.pathsep + env["PATH"],
               CARGO_BUILD_JOBS="4", CARGO_NET_OFFLINE="true",
               RUSTUP_TOOLCHAIN="nightly-2026-11-09-aarch64-pc-windows-msvc",
               CARGO_TARGET_DIR=str(Path(os.environ["TEMP"]) / "SwiftBonsai-Prism0124-Registry-20261109/target"),
               GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="safe.directory", GIT_CONFIG_VALUE_0="*")
    for name in ("GITHUB_TOKEN", "GH_TOKEN", "ACTIONS_RUNTIME_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN", "ACTIONS_RUNNER_INPUT_TOKEN"):
        env.pop(name, None)
    receipt["compilerCache"] = env["CARGO_TARGET_DIR"]

    def save():
        (evidence / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")

    def run(name, command, cap, minimum=1, online=False):
        if 270 - (time.monotonic() - started) < minimum:
            raise RuntimeError(f"Insufficient original time for {name}")
        step = {"name": name, "arguments": list(map(str, command)), "powerBefore": power()}
        receipt["steps"].append(step)
        log = evidence / f"{len(receipt['steps']):02d}-{name}.log"
        step["log"] = log.name
        child_env = dict(env)
        if online:
            child_env["CARGO_NET_OFFLINE"] = "false"
        began = time.monotonic()
        deadline = min(started + 270, began + cap)
        print(f"Starting {name}", flush=True)
        child = None
        try:
            with log.open("wb") as output:
                child = subprocess.Popen(step["arguments"], cwd=SOURCE, env=child_env,
                                         stdout=output, stderr=subprocess.STDOUT,
                                         creationflags=subprocess.CREATE_NO_WINDOW)
                while child.poll() is None:
                    receipt["powerSamples"].append(power())
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f"Original deadline reached for {name}")
                    try:
                        child.wait(timeout=min(1, max(.001, deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
                step["exitCode"] = child.returncode
        finally:
            if child is not None and child.poll() is None:
                result = subprocess.run(["taskkill.exe", "/PID", str(child.pid), "/T", "/F"],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                        timeout=8, creationflags=subprocess.CREATE_NO_WINDOW)
                step["cleanup"] = {"treeTerminationReported": result.returncode == 0}
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=5)
                step["cleanup"]["parentExited"] = child.poll() is not None
            step.update(powerAfter=power(), elapsedSeconds=time.monotonic() - began)
            save()
        if step.get("exitCode") != 0:
            raise RuntimeError(f"{name} failed; no retry")
        print(f"Finished {name}: {step['elapsedSeconds']:.3f}s", flush=True)
        return log.read_text(encoding="utf-8", errors="replace")

    def download(name, url, leaf):
        path = evidence / leaf
        run(name, [sys.executable, "-B", __file__, "--fetch", url, path], 15, minimum=16, online=True)
        return path.read_bytes()

    def clean_source(expected):
        if run("source-head", [GIT, "rev-parse", "HEAD"], 5).strip() != expected:
            raise RuntimeError("Source revision changed")
        if run("source-branch", [GIT, "branch", "--show-current"], 5).strip() != "main":
            raise RuntimeError("Primary Prism checkout must remain on main")
        if run("source-status", [GIT, "status", "--porcelain"], 5).strip():
            raise RuntimeError("Source must be clean")

    try:
        save()
        shutil.copyfile(BASE / "workload-plan.json", evidence / "workload-plan.json")
        shutil.copyfile(__file__, evidence / "publish.py")
        revision = declaration["sourceRevision"]
        receipt["sourceRevision"] = revision
        receipt["originalStartedUtc"] = declaration["originalStartedUtc"]
        receipt["preEntryLaunchFailure"] = declaration["preEntryLaunchFailure"]
        receipt["powerAtOriginalStart"] = "unknown; no power provider executed during failed parser entry"
        clean_source(revision)
        helper_delta = run("unchanged-helper-trees", [GIT, "diff", "--name-only", "055c7430cb4771dfddb24ddaa30494af6ad182da", revision, "--", *["tools/" + value[1] for value in HELPERS.values()]], 5)
        if helper_delta.strip():
            raise RuntimeError("Published helper source trees have changed")
        receipt["helperTreesUnchangedSincePublishedSource"] = "055c7430cb4771dfddb24ddaa30494af6ad182da"
        run("compiler-identity", [BIN / "rustc.exe", "-vV"], 5)
        source_archive = BASE / "source.tar"
        run("source-archive", [GIT, "archive", "--format=tar", "--output", source_archive, revision], 5)
        source_files = {}
        with tarfile.open(source_archive) as archive:
            for member in archive:
                if member.isfile():
                    source_files[member.name] = archive.extractfile(member).read()
        receipt["sourceArchiveSha256"] = sha(source_archive.read_bytes())
        for name, (version, folder) in HELPERS.items():
            metadata = json.loads(download(name + "-metadata", f"https://crates.io/api/v1/crates/{name}/{version}", name + ".json"))["version"]
            data = download(name + "-archive", f"https://static.crates.io/crates/{name}/{name}-{version}.crate", f"{name}-{version}.crate")
            if metadata["yanked"] or sha(data) != metadata["checksum"]:
                raise RuntimeError(f"Unusable registry helper {name}")
            checked = []
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
                for member in archive:
                    relative = member.name.removeprefix(f"{name}-{version}/")
                    if not member.isfile() or relative in ("Cargo.toml", "Cargo.lock", ".cargo_vcs_info.json"):
                        continue
                    local_relative = "Cargo.toml" if relative == "Cargo.toml.orig" else relative
                    registry_bytes = archive.extractfile(member).read()
                    local_bytes = source_files.get(f"tools/{folder}/{local_relative}")
                    if local_bytes is None or registry_bytes.replace(b"\r\n", b"\n") != local_bytes.replace(b"\r\n", b"\n"):
                        raise RuntimeError(f"Registry helper source differs: {name}/{relative}")
                    checked.append(relative)
            receipt["helpers"].append({"name": name, "version": version, "checksum": metadata["checksum"],
                                       "sourceFilesCompared": checked, "comparison": "equal after CRLF-to-LF normalization"})
            save()
        before = json.loads(download("registry-before", f"https://crates.io/api/v1/crates/prism-inference/{VERSION}", "registry-before.json"))
        if before is not None:
            raise RuntimeError("Version already exists; do not upload")
        cargo = BIN / "cargo.exe"
        run("normalized-package", [cargo, "package", "--locked", "--offline", "-p", "prism-inference"], 140, minimum=170)
        package = Path(env["CARGO_TARGET_DIR"]) / f"package/prism-inference-{VERSION}.crate"
        with tarfile.open(package, "r:gz") as archive:
            prefix = f"prism-inference-{VERSION}/"
            manifest = tomllib.loads(archive.extractfile(prefix + "Cargo.toml").read().decode())
            vcs = json.load(archive.extractfile(prefix + ".cargo_vcs_info.json"))
            if manifest["package"]["version"] != VERSION or vcs["git"]["sha1"] != revision or vcs["git"].get("dirty", False):
                raise RuntimeError("Normalized package identity differs")
            pins = {}
            for group in (manifest, *manifest.get("target", {}).values()):
                for section in ("dependencies", "build-dependencies", "dev-dependencies"):
                    for name, dependency in group.get(section, {}).items():
                        if any(key in dependency for key in ("path", "git", "workspace")):
                            raise RuntimeError("Normalized registry dependency retains a source override")
                        if name in HELPERS:
                            pins[name] = dependency["version"]
            if pins != {name: "=" + value[0] for name, value in HELPERS.items()}:
                raise RuntimeError("Normalized helper pins differ")
            receipt["packageAudit"] = {"sha256": sha(package.read_bytes()), "bytes": package.stat().st_size,
                                       "members": len(archive.getmembers()), "helperPins": pins}
        shutil.copyfile(package, evidence / package.name)
        receipt["normalizedPackageVerified"] = True
        clean_source(revision)
        receipt.update(publicationAttempted=True, publicationOutcome="unknown-until-confirmed")
        save()
        run("publish", [cargo, "publish", "--locked", "--registry", "crates-io", "-p", "prism-inference"], 90, minimum=100, online=True)
        after = json.loads(download("registry-after", f"https://crates.io/api/v1/crates/prism-inference/{VERSION}", "registry-after.json"))["version"]
        if after["checksum"] != receipt["packageAudit"]["sha256"] or after["yanked"] or after["num"] != VERSION:
            raise RuntimeError("Public checksum/version differs")
        receipt.update(status="passed", publicationOutcome="verified-version-and-checksum",
                       publication={key: after[key] for key in ("num", "checksum", "created_at", "yanked")})
    except Exception as error:
        receipt["failure"] = f"{type(error).__name__}: {error}"
    finally:
        receipt.update(powerAfter=power(), clockClosed=True, clockRestarted=False,
                       continuousPowerStabilityEstablished=False, fullFoundationQualified=False,
                       modelAcceptanceQualified=False, performanceQualified=False)
        receipt["cleanupConfirmed"] = all(step.get("cleanup", {}).get("treeTerminationReported") is True
                                           and step["cleanup"].get("parentExited") is True
                                           for step in receipt["steps"] if "cleanup" in step)
        save()
        inventory = {path.name: {"bytes": path.stat().st_size, "sha256": sha(path.read_bytes())}
                     for path in evidence.iterdir() if path.is_file()}
        (evidence / "archive.json").write_text(json.dumps({"files": inventory}, indent=2) + "\n")
        clock = {"maximumTotalSeconds": 300, "elapsedSecondsThroughRetention": time.monotonic() - started,
                 "powerAfterRetention": power(), "clockClosed": True, "retries": 0,
                 "cleanupConfirmed": receipt["cleanupConfirmed"]}
        clock["withinDeclaredAllowance"] = clock["elapsedSecondsThroughRetention"] <= 300
        (evidence / "final-clock.json").write_text(json.dumps(clock, indent=2) + "\n")
        print(json.dumps({"status": receipt["status"], "failure": receipt.get("failure"),
                          "publicationOutcome": receipt["publicationOutcome"], **clock}), flush=True)
    return 0 if receipt["status"] == "passed" and clock["withinDeclaredAllowance"] else 1


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--fetch":
        fetch(sys.argv[2], sys.argv[3])
    else:
        raise SystemExit(main())
