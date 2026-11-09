"""One five-minute authorized source push and Foundation bundle operation. No retry."""

import argparse
import base64
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile


REPOSITORY = "symbiogenesis/prism-inference"
REVISION = "a7cdee96080d6cc33fb6a22b62ede94117150b9b"
REMOTE_BASE = "3bd63f675fd4a97a927e0e6abc11874089814d60"
VERSION = "0.1.23"
TARGET = "aarch64-pc-windows-msvc"
TOOLCHAIN = "nightly-2026-11-09-" + TARGET
WORKFLOW = "foundation-bundle.yml"
FULL_WORKFLOW = "foundation.yml"
STEM = f"prism-inference-{VERSION}-{TARGET}"
MAXIMUM, WORK_CUTOFF, CLEANUP_CUTOFF = 300, 260, 285
MIB = 1024 * 1024
MAX_ARCHIVE = 64 * MIB
GH = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\GitHub.cli_Microsoft.Winget.Source_8wekyb3d8bbwe\bin\gh.exe")
GIT = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe\cmd\git.exe")
EXECUTABLES = {"prism-inference.exe", "bonsai-token-worker.exe", "bonsai-swift-worker.exe",
               "prism-inference-server.exe", "bonsai-gguf.exe"}
FILES = EXECUTABLES | {"LICENSE", "NOTICE", "PRISM-LICENSE.txt", "CAMELID-LICENSE.txt"}


def utc():
    return datetime.now(timezone.utc).isoformat()


class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]


def power():
    status, known = Power(), False
    try:
        query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
        query.argtypes, query.restype = [ctypes.POINTER(Power)], ctypes.c_int
        known = query(ctypes.byref(status)) != 0
    except (OSError, AttributeError):
        pass
    return {"source": "GetSystemPowerStatus", "utc": utc(),
            "acConnected": bool(status.ac) if known and status.ac in (0, 1) else None,
            "batterySaver": bool(status.saver) if known and status.saver in (0, 1) else None,
            "chargePercent": int(status.charge) if known and status.charge <= 100 else None}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha256(path, check=lambda: None):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(65536):
            check()
            result.update(block)
    return result.hexdigest()


def source_download(url, output):
    """Anonymous source download child; parent enforces a single ten-second cap."""
    deadline = time.monotonic() + 9
    request = urllib.request.Request(url, headers={"User-Agent": "SwiftBonsai2Runner bounded source review"})
    with urllib.request.urlopen(request, timeout=8) as response, output.open("xb") as stream:
        total = 0
        while block := response.read(65536):
            total += len(block)
            if total > MAX_ARCHIVE or time.monotonic() >= deadline:
                raise TimeoutError("Source download exceeded its fixed byte/time bound")
            stream.write(block)


class Operation:
    def __init__(self, args, started, before):
        self.args, self.started = args, started
        self.owned = {}
        self.source_hashes = {}
        self.source_blobs = {}
        self.verified = None
        self.env = dict(os.environ)
        for key in ("GH_DEBUG", "DEBUG", "ACTIONS_STEP_DEBUG", "ACTIONS_RUNNER_DEBUG"):
            self.env.pop(key, None)
        self.env.update(GH_HOST="github.com", GH_PROMPT_DISABLED="1", GIT_TERMINAL_PROMPT="0")
        self.receipt = {"schemaVersion": 1, "kind": "bounded-foundation-bundle-operation",
                        "sourceRevision": REVISION, "version": VERSION, "operationId": args.operation_id,
                        "startedUtc": utc(), "maximumAggregateSeconds": MAXIMUM,
                        "workCutoffSeconds": WORK_CUTOFF, "cancellationCutoffSeconds": CLEANUP_CUTOFF,
                        "driverSha256": sha256(Path(__file__)),
                        "declarationSha256": sha256(Path(__file__).with_name("workload-plan.json")),
                        "status": "preparing", "steps": [], "powerBefore": before, "powerAfter": None,
                        "powerSamples": [], "dispatchAttempted": False, "pushAttempted": False,
                        "fullFoundationMayHaveStarted": False, "cleanupIncomplete": False,
                        "clockClosed": False, "clockRestarted": False, "retries": 0,
                        "modelsExecuted": 0, "benchmarksExecuted": 0, "otherCiQueried": False,
                        "fullFoundationQualified": False, "performanceQualified": False,
                        "continuousPowerStabilityProven": False, "ownedRuns": self.owned}
        self.save()

    def sanitize_log(self, path):
        text = path.read_text(encoding="utf-8", errors="replace")
        for name in ("GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"):
            if os.environ.get(name):
                text = text.replace(os.environ[name], "[redacted credential]")
        text = re.sub(r"https?://[^/\s]+@", "https://[redacted userinfo]@", text)
        text = re.sub(r"(https?://[^\s?]+)\?[^\s]+", r"\1?[redacted query]", text)
        path.write_text(text, encoding="utf-8")

    def elapsed(self):
        return time.monotonic() - self.started

    def check(self, cutoff=WORK_CUTOFF, minimum=0):
        if self.elapsed() + minimum >= cutoff:
            raise TimeoutError("Original operation allowance cannot fit further work; no retry")

    def save(self):
        self.receipt["elapsedSeconds"] = self.elapsed()
        write_json(self.args.work / "receipt.json", self.receipt)

    def command(self, name, arguments, *, payload=None, binary=False, cutoff=WORK_CUTOFF,
                allow_failure=False, maximum_bytes=4 * MIB):
        self.check(cutoff, minimum=1)
        number = len(self.receipt["steps"]) + 1
        output = self.args.work / f"{number:02d}-{name}.{'zip' if binary else 'json'}"
        error_path = self.args.work / f"{number:02d}-{name}.stderr.log"
        step = {"name": name, "arguments": [str(arg) for arg in arguments],
                "powerBefore": power(), "startedAtSeconds": self.elapsed(),
                "stdout": output.name, "stderr": error_path.name, "timedOut": False}
        self.receipt["steps"].append(step)
        self.save()
        print(f"{self.elapsed():.1f}s {name}", flush=True)
        deadline = min(self.started + cutoff, time.monotonic() + 10)
        process = None
        try:
            with output.open("xb") as stdout, error_path.open("xb") as stderr:
                process = subprocess.Popen(step["arguments"], cwd=self.args.source, env=self.env,
                                           stdin=subprocess.PIPE if payload is not None else subprocess.DEVNULL,
                                           stdout=stdout, stderr=stderr,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
                if payload is not None:
                    process.stdin.write(json.dumps(payload).encode("utf-8"))
                    process.stdin.close()
                while process.poll() is None:
                    if time.monotonic() >= deadline or output.stat().st_size > maximum_bytes:
                        step["timedOut"] = True
                        raise TimeoutError(f"{name} exceeded its single ten-second or byte bound")
                    self.receipt["powerSamples"].append(power())
                    try:
                        process.wait(timeout=min(0.5, max(0.001, deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
            step["exitCode"] = process.returncode
            if output.stat().st_size > maximum_bytes:
                raise RuntimeError(f"{name} exceeded its byte bound")
            if process.returncode and not allow_failure:
                raise RuntimeError(f"{name} failed with exit {process.returncode}; no retry")
            return output
        finally:
            if process is not None and process.poll() is None:
                step["terminationRequired"] = True
                # A source push may have spawned a credential helper: retain tree cleanup.
                try:
                    killed = subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"),
                                             "/PID", str(process.pid), "/T", "/F"],
                                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                            timeout=min(3, max(0.1, MAXIMUM - self.elapsed() - 15)),
                                            creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                    step["treeTerminationReported"] = killed.returncode == 0
                    process.wait(timeout=2)
                except (OSError, subprocess.TimeoutExpired):
                    step["treeTerminationReported"] = False
                    process.kill()
                step["parentExitConfirmed"] = process.poll() is not None
                if not step.get("treeTerminationReported") or not step["parentExitConfirmed"]:
                    self.receipt["cleanupIncomplete"] = True
            step["powerAfter"] = power()
            step["finishedAtSeconds"] = self.elapsed()
            if error_path.exists():
                self.sanitize_log(error_path)
            self.save()

    def api(self, name, path, *, payload=None, binary=False, cutoff=WORK_CUTOFF,
            allow_failure=False, maximum_bytes=4 * MIB):
        command = [GH, "api", "--hostname", "github.com", "-H", "X-GitHub-Api-Version: 2022-11-28"]
        if payload is not None:
            command += ["--method", "POST", "--input", "-"]
        command += [f"repos/{REPOSITORY}/{path}"]
        result = self.command(name, command, payload=payload, binary=binary, cutoff=cutoff,
                              allow_failure=allow_failure, maximum_bytes=maximum_bytes)
        return result if binary or payload is not None else read_json(result)

    def git(self, name, *arguments):
        return self.command(name, [GIT, "-C", self.args.source, *arguments]).read_text(encoding="utf-8").strip()

    def pause(self, cutoff, seconds=2):
        self.check(cutoff, minimum=seconds)
        self.receipt["powerSamples"].append(power())
        time.sleep(seconds)

    def discover(self, workflow, event, *, nonce=None, cutoff=WORK_CUTOFF):
        value = self.api("discover-" + workflow.removesuffix(".yml"),
                         f"actions/workflows/{workflow}/runs?event={event}&branch=main&head_sha={REVISION}&per_page=25",
                         cutoff=cutoff)
        candidates = []
        for run in value.get("workflow_runs", []):
            if run.get("head_sha") != REVISION or run.get("event") != event or run.get("head_branch") != "main":
                continue
            if run.get("path", "").split("@", 1)[0] != ".github/workflows/" + workflow:
                continue
            if nonce is not None and run.get("display_title") != "Foundation bundle / " + nonce:
                continue
            candidates.append(run)
        if len(candidates) > 1:
            raise RuntimeError("More than one run matches the exact authorized operation")
        return candidates[0] if candidates else None

    def bind_run(self, label, run, workflow, event, nonce=None):
        if (not isinstance(run.get("id"), int) or run["id"] <= 0 or run.get("run_attempt") != 1
                or run.get("head_sha") != REVISION or run.get("head_branch") != "main"
                or run.get("event") != event
                or run.get("name") != ("Foundation bundle" if workflow == WORKFLOW else "Foundation")
                or run.get("path", "").split("@", 1)[0] != ".github/workflows/" + workflow
                or nonce is not None and run.get("display_title") != "Foundation bundle / " + nonce):
            raise RuntimeError("Run identity differs from the one authorized source/workflow operation")
        self.owned[label] = {"runId": run["id"], "runAttempt": 1, "workflow": workflow,
                             "event": event, "nonce": nonce, "status": run["status"],
                             "url": run.get("html_url"),
                             "conclusion": run.get("conclusion"), "terminalConfirmed": run["status"] == "completed",
                             "cancelAttempted": False}
        self.save()

    def observe_run(self, label, cutoff):
        owned = self.owned[label]
        run = self.api("observe-" + label, f"actions/runs/{owned['runId']}", cutoff=cutoff)
        prior_cancel = owned["cancelAttempted"]
        self.bind_run(label, run, owned["workflow"], owned["event"], owned["nonce"])
        self.owned[label]["cancelAttempted"] = prior_cancel
        return run

    def cancel(self, label, cutoff=CLEANUP_CUTOFF):
        owned = self.owned[label]
        if owned["terminalConfirmed"]:
            return
        if not owned["cancelAttempted"]:
            owned["cancelAttempted"] = True
            self.save()
            self.api("cancel-" + label, f"actions/runs/{owned['runId']}/cancel", payload={},
                     cutoff=cutoff, allow_failure=True)
        while not self.owned[label]["terminalConfirmed"]:
            self.observe_run(label, cutoff)
            if not self.owned[label]["terminalConfirmed"]:
                self.pause(cutoff)

    def source_intake_and_push(self):
        if self.git("local-head", "rev-parse", "HEAD") != REVISION:
            raise RuntimeError("Local Prism source is not the declared revision")
        if self.git("local-branch", "branch", "--show-current") != "main" or self.git("local-clean", "status", "--porcelain"):
            raise RuntimeError("Exact clean local Prism main is required")
        origin = self.git("local-origin", "remote", "get-url", "origin")
        if origin not in (f"https://github.com/{REPOSITORY}", f"https://github.com/{REPOSITORY}.git"):
            (self.args.work / self.receipt["steps"][-1]["stdout"]).write_text("Unexpected origin URL redacted.\n", encoding="utf-8")
            raise RuntimeError("Origin is not the exact approved official Prism repository")
        self.git("local-ancestor", "merge-base", "--is-ancestor", REMOTE_BASE, REVISION)
        tracked = self.git("local-tracked", "ls-files", "-z").split("\0")
        for name in filter(None, tracked):
            self.check()
            path = self.args.source / name
            if path.is_symlink() or not path.is_file():
                raise RuntimeError("Only regular tracked source files are admitted")
            self.source_hashes[name] = sha256(path, self.check)
        write_json(self.args.work / "source-inputs.json", self.source_hashes)
        tree = self.git("canonical-source-tree", "ls-tree", "-r", "-z", REVISION)
        for entry in filter(None, tree.split("\0")):
            identity, name = entry.split("\t", 1)
            mode, kind, oid = identity.split()
            if mode not in ("100644", "100755") or kind != "blob" or re.fullmatch(r"[a-f0-9]{40}", oid) is None:
                raise RuntimeError("Only ordinary committed source blobs are admitted")
            self.source_blobs[name] = oid
        if set(self.source_blobs) != set(self.source_hashes):
            raise RuntimeError("Tracked working inputs differ from the exact committed tree")
        write_json(self.args.work / "source-git-blobs.json", self.source_blobs)
        remote = self.api("remote-main-before", "git/ref/heads/main")
        remote_sha = remote.get("object", {}).get("sha")
        if remote_sha not in (REMOTE_BASE, REVISION):
            raise RuntimeError("Remote main is neither the approved base nor the selected exact revision")
        self.receipt["remoteRevisionBefore"] = remote_sha
        self.receipt["fullFoundationMayHaveStarted"] = True
        if remote_sha == REMOTE_BASE:
            self.receipt["pushAttempted"] = True
            self.save()
            self.git("authorized-exact-source-push", "push", "origin", REVISION + ":refs/heads/main")
        else:
            self.receipt["pushSkippedAlreadyExact"] = True
        remote = self.api("remote-main-after", "git/ref/heads/main")
        if remote.get("object", {}).get("sha") != REVISION:
            raise RuntimeError("Remote main did not confirm the selected exact revision")
        self.receipt["remoteRevisionAfter"] = REVISION
        find_deadline = min(WORK_CUTOFF, self.elapsed() + 20)
        while "automatic-foundation" not in self.owned:
            run = self.discover(FULL_WORKFLOW, "push", cutoff=find_deadline)
            if run:
                self.bind_run("automatic-foundation", run, FULL_WORKFLOW, "push")
                break
            self.pause(find_deadline)
        self.cancel("automatic-foundation", min(WORK_CUTOFF, self.elapsed() + 30))
        propagation_deadline = min(WORK_CUTOFF, self.elapsed() + 15)
        commit = self.api("remote-exact-commit", f"git/commits/{REVISION}", cutoff=propagation_deadline)
        if commit.get("sha") != REVISION:
            raise RuntimeError("Remote source commit identity differs")
        workflow = self.api("remote-workflow", f"contents/.github/workflows/{WORKFLOW}?ref={REVISION}", cutoff=propagation_deadline)
        if workflow.get("encoding") != "base64" or workflow.get("type") != "file":
            raise RuntimeError("Remote workflow content is not an ordinary encoded source file")
        encoded = "".join(workflow["content"].split())
        content = base64.b64decode(encoded, validate=True)
        canonical_path = self.command("canonical-workflow", [GIT, "-C", self.args.source,
                                                             "show", REVISION + ":.github/workflows/" + WORKFLOW])
        if content != canonical_path.read_bytes():
            raise RuntimeError("Remote dedicated workflow bytes differ from exact local source")
        self.receipt["remoteWorkflowSha256"] = hashlib.sha256(content).hexdigest()

    def dispatch_and_wait(self):
        # Keep145s for selected queue/work,10s for dispatch and the same35s
        # artifact/receive reserve enforced by the polling loop below.
        self.check(minimum=190)
        self.receipt["dispatchAttempted"] = True
        self.save()
        self.api("dispatch-owned-bundle", f"actions/workflows/{WORKFLOW}/dispatches",
                 payload={"ref": "main", "inputs": {"expected_revision": REVISION,
                                                       "operation_id": self.args.operation_id}})
        find_deadline = min(WORK_CUTOFF - 35, self.elapsed() + 20)
        while "bundle" not in self.owned:
            run = self.discover(WORKFLOW, "workflow_dispatch", nonce=self.args.operation_id, cutoff=find_deadline)
            if run:
                self.bind_run("bundle", run, WORKFLOW, "workflow_dispatch", self.args.operation_id)
                break
            self.pause(find_deadline)
        while True:
            self.check(WORK_CUTOFF, minimum=35)
            run = self.observe_run("bundle", WORK_CUTOFF - 25)
            print(f"{self.elapsed():.1f}s owned bundle {run['status']} {run.get('conclusion')}", flush=True)
            if run["status"] == "completed":
                if run.get("conclusion") != "success":
                    raise RuntimeError("Owned bundle run failed; no retry")
                return run
            self.pause(WORK_CUTOFF - 35, seconds=3)

    def safe_entries(self, archive, expected=None, maximum_total=256 * MIB):
        entries = archive.infolist()
        names = [entry.filename for entry in entries]
        if len(names) != len(set(name.casefold() for name in names)) or expected is not None and set(names) != expected:
            raise RuntimeError("Archive inventory differs or contains duplicate names")
        total = 0
        for entry in entries:
            self.check()
            if (entry.flag_bits & 1 or entry.is_dir() or entry.file_size > MAX_ARCHIVE
                    or (entry.external_attr >> 16) & 0o170000 == 0o120000
                    or "\\" in entry.filename or ":" in entry.filename
                    or any(part in ("", ".", "..") for part in entry.filename.split("/"))):
                raise RuntimeError("Archive contains an oversized, linked, encrypted or unsafe entry")
            total += entry.file_size
            if total > maximum_total:
                raise RuntimeError("Archive expansion exceeds the fixed bound")
        return entries

    def entry_bytes(self, archive, entry, maximum):
        if entry.file_size > maximum:
            raise RuntimeError("Archive entry exceeds its admitted byte bound")
        result = bytearray()
        with archive.open(entry) as stream:
            while block := stream.read(65536):
                self.check()
                result.extend(block)
                if len(result) > maximum:
                    raise RuntimeError("Archive entry expands beyond its bound")
        if len(result) != entry.file_size:
            raise RuntimeError("Archive entry length mismatch")
        return bytes(result)

    def verify_artifact(self, run):
        self.check(minimum=30)
        listing = self.api("owned-artifacts", f"actions/runs/{run['id']}/artifacts?per_page=25")
        selected = [item for item in listing.get("artifacts", []) if item.get("name") == STEM]
        if len(selected) != 1:
            raise RuntimeError("Exactly one named ARM64 bundle artifact is required")
        artifact = selected[0]
        origin = artifact.get("workflow_run", {})
        if (artifact.get("expired") is not False or not isinstance(artifact.get("id"), int)
                or origin.get("id") != run["id"] or origin.get("head_sha") != REVISION
                or origin.get("head_branch") != "main" or not 0 < artifact.get("size_in_bytes", 0) <= MAX_ARCHIVE
                or not re.fullmatch(r"sha256:[a-f0-9]{64}", artifact.get("digest", ""))):
            raise RuntimeError("Official artifact identity, digest or bound differs")
        path = self.api("download-owned-artifact", f"actions/artifacts/{artifact['id']}/zip",
                        binary=True, maximum_bytes=MAX_ARCHIVE)
        if path.stat().st_size != artifact["size_in_bytes"] or "sha256:" + sha256(path, self.check) != artifact["digest"]:
            raise RuntimeError("Downloaded artifact differs from official metadata")
        with zipfile.ZipFile(path) as outer:
            for entry in self.safe_entries(outer, {STEM + ".zip", STEM + ".receipt.json", "bundle-scope.json"}):
                limit = MAX_ARCHIVE if entry.filename.endswith(".zip") else MIB
                (self.args.work / entry.filename).write_bytes(self.entry_bytes(outer, entry, limit))
        receipt = read_json(self.args.work / (STEM + ".receipt.json"))
        scope = read_json(self.args.work / "bundle-scope.json")
        for value in (receipt, scope):
            if (value.get("SchemaVersion") != 1 or value.get("SourceRevision") != REVISION
                    or value.get("Version") != VERSION or value.get("Target") != TARGET
                    or str(value.get("RunId")) != str(run["id"]) or str(value.get("RunAttempt")) != "1"):
                raise RuntimeError("Bundle receipt/scope does not bind the selected genuine source/run")
        if (receipt.get("Repository") != "https://github.com/" + REPOSITORY
                or receipt.get("Toolchain") != TOOLCHAIN
                or scope.get("Kind") != "hosted-foundation-bundle-only"
                or scope.get("OperationId") != self.args.operation_id
                or scope.get("ReceiptControlsPassed") != 9 or scope.get("FinalSourceChecksPassed") is not True
                or scope.get("ProductionLinkingPassed") is not True
                or any(scope.get(key) is not False for key in ("FullFoundationQualified", "ModelAcceptanceQualified", "PerformanceQualified"))):
            raise RuntimeError("Bundle compiler, checks, operation or limited qualification scope differs")
        bundle = self.args.work / (STEM + ".zip")
        identity = receipt.get("Archive", {})
        if (identity.get("Name") != bundle.name or identity.get("Bytes") != bundle.stat().st_size
                or identity.get("Sha256") != sha256(bundle, self.check)):
            raise RuntimeError("Inner release archive differs from its receipt")
        inventory = receipt.get("Files", [])
        if len(inventory) != 9 or {item.get("Name") for item in inventory} != FILES:
            raise RuntimeError("Receipt must identify exactly five executables and four licenses")
        with zipfile.ZipFile(bundle) as archive:
            self.safe_entries(archive, FILES)
            for item in inventory:
                entry = archive.getinfo(item["Name"])
                data = self.entry_bytes(archive, entry, MAX_ARCHIVE)
                if not data or len(data) != item.get("Bytes") or hashlib.sha256(data).hexdigest() != item.get("Sha256"):
                    raise RuntimeError("Release entry length or SHA-256 differs")
                if item["Name"] in EXECUTABLES:
                    if len(data) < 64 or data[:2] != b"MZ":
                        raise RuntimeError("Release executable lacks a DOS header")
                    offset = struct.unpack_from("<I", data, 60)[0]
                    if offset < 64 or offset > len(data) - 6 or data[offset:offset + 6] != b"PE\0\0\x64\xaa":
                        raise RuntimeError("Release executable is not ARM64 PE")
        self.verified = {"schemaVersion": 1, "passed": True, "sourceRevision": REVISION,
                         "version": VERSION, "workflowFile": WORKFLOW, "runId": run["id"], "runAttempt": 1,
                         "runConclusion": "success", "operationId": self.args.operation_id,
                         "finalSourceChecksPassed": True, "productionLinkingPassed": True,
                         "fullFoundationQualified": False, "modelAcceptanceQualified": False,
                         "performanceQualified": False, "artifactId": artifact["id"],
                         "artifactArchivePath": str(self.args.retain / path.name),
                         "artifactArchiveSha256": sha256(path, self.check),
                         "bundleArchivePath": str(self.args.retain / bundle.name),
                         "bundleArchiveSha256": sha256(bundle, self.check),
                         "receiptPath": str(self.args.retain / (STEM + ".receipt.json")),
                         "scopePath": str(self.args.retain / "bundle-scope.json"),
                         "sourceArchivePath": None, "sourceArchiveSha256": None,
                         "sourceArchiveVerified": False,
                         "admission": "Also require final-clock.json below300 seconds and cleanupIncomplete=false."}

    def optional_source_archive(self):
        if self.elapsed() + 22 >= WORK_CUTOFF:
            self.receipt["sourceArchivePendingReason"] = "insufficient-original-time; no source retry"
            return
        url = f"https://github.com/{REPOSITORY}/archive/{REVISION}.zip"
        path = self.args.work / "source.zip"
        try:
            self.command("download-exact-source", [sys.executable, "-B", Path(__file__),
                                                   "--download-source", url, str(path)], maximum_bytes=MIB)
            prefix = f"prism-inference-{REVISION}/"
            with zipfile.ZipFile(path) as archive:
                entries = archive.infolist()
                names = [entry.filename for entry in entries]
                if len(entries) > 65536 or len(names) != len(set(name.casefold() for name in names)):
                    raise RuntimeError("Source archive has duplicate or excessive entries")
                total = 0
                for entry in entries:
                    self.check()
                    total += entry.file_size
                    if (not entry.filename.startswith(prefix) or "\\" in entry.filename or ":" in entry.filename
                            or any(part in (".", "..") for part in entry.filename.split("/"))
                            or (entry.external_attr >> 16) & 0o170000 == 0o120000
                            or entry.flag_bits & 1 or entry.file_size > MAX_ARCHIVE or total > 512 * MIB):
                        raise RuntimeError("Source archive inventory violates its fixed bounds")
                for name, expected in self.source_blobs.items():
                    entry = archive.getinfo(prefix + name)
                    data = self.entry_bytes(archive, entry, MAX_ARCHIVE)
                    git_object = b"blob " + str(len(data)).encode("ascii") + b"\0" + data
                    if hashlib.sha1(git_object, usedforsecurity=False).hexdigest() != expected:
                        raise RuntimeError("Source ZIP does not match every frozen committed Git blob")
            self.verified.update(sourceArchivePath=str(self.args.retain / path.name),
                                 sourceArchiveSha256=sha256(path, self.check),
                                 sourceArchiveBytes=path.stat().st_size, sourceArchiveUrl=url,
                                 sourceArchiveVerified=True, verifiedSourceFileCount=len(self.source_blobs),
                                 sourceComparison="Canonical Git blob identities; local checkout CRLF hashes retained separately.")
        except Exception as error:
            self.receipt["sourceArchivePendingReason"] = type(error).__name__ + ": " + str(error)
            # No second source download, new CI dispatch or wider allowance follows.

    def cleanup(self):
        if self.receipt["fullFoundationMayHaveStarted"] and "automatic-foundation" not in self.owned:
            try:
                run = self.discover(FULL_WORKFLOW, "push", cutoff=min(CLEANUP_CUTOFF, self.elapsed() + 12))
                if run:
                    self.bind_run("automatic-foundation", run, FULL_WORKFLOW, "push")
                else:
                    self.receipt["cleanupIncomplete"] = True
                    self.receipt["automaticFoundationCleanup"] = "unknown: authorized push run was not observable"
            except Exception as error:
                self.receipt["cleanupIncomplete"] = True
                self.receipt["automaticFoundationCleanup"] = type(error).__name__ + ": " + str(error)
        if self.receipt["dispatchAttempted"] and "bundle" not in self.owned:
            try:
                run = self.discover(WORKFLOW, "workflow_dispatch", nonce=self.args.operation_id,
                                    cutoff=min(CLEANUP_CUTOFF, self.elapsed() + 12))
                if run:
                    self.bind_run("bundle", run, WORKFLOW, "workflow_dispatch", self.args.operation_id)
                else:
                    self.receipt["cleanupIncomplete"] = True
                    self.receipt["bundleCleanup"] = "unknown: dispatched own nonce run was not observable"
            except Exception as error:
                self.receipt["cleanupIncomplete"] = True
                self.receipt["bundleCleanup"] = type(error).__name__ + ": " + str(error)
        for label in list(self.owned):
            try:
                self.cancel(label)
            except Exception as error:
                self.receipt["cleanupIncomplete"] = True
                self.owned[label]["cleanupError"] = type(error).__name__ + ": " + str(error)
        if any(not run["terminalConfirmed"] for run in self.owned.values()):
            self.receipt["cleanupIncomplete"] = True
        if self.receipt["cleanupIncomplete"]:
            self.verified = None
            print("Owned Foundation cleanup remains UNKNOWN; inspect retained owned run IDs.", flush=True)

    def retain(self):
        self.receipt["powerAfter"] = power()
        self.receipt["clockClosed"] = True
        self.save()
        if self.verified is not None and not self.receipt["cleanupIncomplete"] and self.elapsed() < 285:
            write_json(self.args.work / "verified-bundle.json", self.verified)
        self.args.retain.mkdir(parents=True, exist_ok=False)
        retention_complete = False
        try:
            for path in self.args.work.iterdir():
                if path.is_file():
                    self.check(MAXIMUM - 5)
                    shutil.copyfile(path, self.args.retain / path.name)
            for name in ("operate.py", "workload-plan.json"):
                self.check(MAXIMUM - 5)
                shutil.copyfile(Path(__file__).with_name(name), self.args.retain / name)
            manifest = {}
            for path in self.args.retain.iterdir():
                self.check(MAXIMUM - 3)
                manifest[path.name] = {"bytes": path.stat().st_size,
                                       "sha256": sha256(path, lambda: self.check(MAXIMUM - 3))}
            write_json(self.args.retain / "archive.json", {"files": manifest})
            retention_complete = True
        except Exception as error:
            self.receipt["retentionFailure"] = type(error).__name__ + ": " + str(error)
            self.receipt["status"] = "failed-or-incomplete"
            self.verified = None
            for directory in (self.args.work, self.args.retain):
                candidate = directory / "verified-bundle.json"
                if candidate.is_file():
                    candidate.unlink()
            self.save()
            write_json(self.args.retain / "receipt.json", self.receipt)
        clock = {"maximumAggregateSeconds": MAXIMUM, "maximumTotalSeconds": MAXIMUM,
                 "elapsedSecondsThroughRetention": self.elapsed(),
                 "withinDeclaredAllowance": self.elapsed() < MAXIMUM, "clockClosed": True,
                 "closed": True, "retentionComplete": retention_complete,
                 "cleanupIncomplete": self.receipt["cleanupIncomplete"], "retries": 0,
                 "cleanupConfirmed": not self.receipt["cleanupIncomplete"],
                 "powerAfterRetention": power(),
                 "scope": "Includes source push, queue, both owned runs, downloads, verification, cancellation and archive retention; excludes writing this small terminal receipt."}
        write_json(self.args.retain / "final-clock.json", clock)
        print(json.dumps({"status": self.receipt["status"], "retained": str(self.args.retain),
                          "verifiedBundleReceipt": (self.args.retain / "verified-bundle.json").is_file(),
                          "finalClock": clock}), flush=True)


def main():
    started = time.monotonic()
    before = power()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--retain", type=Path, required=True)
    parser.add_argument("--operation-id", required=True)
    args = parser.parse_args()
    if os.name != "nt" or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,47}", args.operation_id) is None:
        raise ValueError("Use native Windows and one fresh safe operation nonce")
    for path in (args.source, args.work, args.retain):
        if not path.is_absolute():
            raise ValueError("All paths must be expanded absolute paths")
    if args.work.exists() or args.retain.exists() or args.work == args.retain:
        raise ValueError("Working and retained output paths must be distinct and fresh; never retry")
    if not args.work.resolve().is_relative_to(Path(tempfile.gettempdir()).resolve()):
        raise ValueError("The working output must use a fresh external system-temp directory")
    if args.work.resolve().is_relative_to(args.retain.resolve()) or args.retain.resolve().is_relative_to(args.work.resolve()):
        raise ValueError("Working and retained evidence directories must not contain one another")
    args.work.mkdir(parents=True, exist_ok=False)
    operation = Operation(args, started, before)
    try:
        operation.source_intake_and_push()
        run = operation.dispatch_and_wait()
        operation.verify_artifact(run)
        operation.optional_source_archive()
        operation.receipt["status"] = "owned-bundle-verified"
    except BaseException as error:
        operation.receipt["status"] = "failed-or-incomplete"
        operation.receipt["failure"] = type(error).__name__ + ": " + str(error)
        operation.verified = None
        print("Operation stopped: " + operation.receipt["failure"], flush=True)
    finally:
        operation.cleanup()
        operation.retain()
    return 0 if operation.receipt["status"] == "owned-bundle-verified" and not operation.receipt["cleanupIncomplete"] else 1


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--download-source":
        expected = f"https://github.com/{REPOSITORY}/archive/{REVISION}.zip"
        if sys.argv[2] != expected:
            raise ValueError("Only the declared anonymous source URL is supported")
        source_download(expected, Path(sys.argv[3]))
    else:
        raise SystemExit(main())
