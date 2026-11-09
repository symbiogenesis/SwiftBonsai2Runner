"""One authorized exact tag and GitHub prerelease delivery; no retry or deletion."""

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
import time

REPO = "symbiogenesis/prism-inference"
REVISION = "055c7430cb4771dfddb24ddaa30494af6ad182da"
VERSION, TAG, RUN = "0.1.23", "v0.1.23", 42721996680
CRATE_SHA = "c786a16b899588e23a205be47092d6c3b4be8a6c273a46681f28df23a9d87b73"
STEM = "prism-inference-0.1.23-aarch64-pc-windows-msvc"
GH = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\GitHub.cli_Microsoft.Winget.Source_8wekyb3d8bbwe\bin\gh.exe")
MAXIMUM, CUTOFF = 120, 105


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte), ("charge", ctypes.c_ubyte),
                    ("saver", ctypes.c_ubyte), ("remaining", ctypes.c_uint32), ("full", ctypes.c_uint32)]
    value, known = Status(), False
    try:
        query = ctypes.WinDLL("kernel32", use_last_error=True).GetSystemPowerStatus
        query.argtypes, query.restype = [ctypes.POINTER(Status)], ctypes.c_int
        known = query(ctypes.byref(value)) != 0
    except (OSError, AttributeError):
        pass
    return {"utc": datetime.now(timezone.utc).isoformat(), "source": "GetSystemPowerStatus",
            "acConnected": bool(value.ac) if known and value.ac in (0, 1) else None,
            "batterySaver": bool(value.saver) if known and value.saver in (0, 1) else None,
            "chargePercent": value.charge if known and value.charge <= 100 else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("bundle-proof", "bundle-clock", "crate-receipt", "notes", "work", "retain"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    started = time.monotonic()
    if not args.work.is_absolute() or not args.retain.is_absolute() or args.work.exists() or args.retain.exists():
        raise ValueError("Use fresh absolute work/retention directories; never retry")
    if args.work.resolve().is_relative_to(args.retain.resolve()) or args.retain.resolve().is_relative_to(args.work.resolve()):
        raise ValueError("Work and retention directories must be separate")
    args.work.mkdir(parents=True, exist_ok=False)
    receipt = {"schemaVersion": 1, "sourceRevision": REVISION, "version": VERSION, "tag": TAG,
               "maximumTotalSeconds": MAXIMUM, "workCutoffSeconds": CUTOFF, "status": "failed",
               "steps": [], "powerSamples": [], "powerBefore": power(), "tagCreationAttempted": False,
               "draftCreationAttempted": False, "assetUploadAttempted": False, "publicationAttempted": False,
               "partialPublicationState": "none", "ciActions": 0, "modelsExecuted": 0, "retries": 0}
    env = dict(os.environ)
    for name in ("GH_DEBUG", "DEBUG", "ACTIONS_STEP_DEBUG", "ACTIONS_RUNNER_DEBUG"):
        env.pop(name, None)
    env.update(GH_HOST="github.com", GH_PROMPT_DISABLED="1", GIT_TERMINAL_PROMPT="0")
    receipt["authSource"] = next((name + " environment variable" for name in ("GH_TOKEN", "GITHUB_TOKEN") if env.get(name)),
                                 "GitHub CLI credential store; verified only by authenticated command success")

    def save():
        (args.work / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def check(minimum=0):
        if time.monotonic() - started + minimum >= CUTOFF:
            raise TimeoutError("Original delivery allowance cannot fit further work")

    def run(name, arguments, allow_404=False):
        check(1)
        step = {"name": name, "arguments": [str(item) for item in arguments], "powerBefore": power(), "exitCode": None}
        receipt["steps"].append(step)
        output, error = args.work / f"{len(receipt['steps']):02d}-{name}.stdout.log", args.work / f"{len(receipt['steps']):02d}-{name}.stderr.log"
        step.update(stdout=output.name, stderr=error.name)
        began, child = time.monotonic(), None
        print("Starting " + name, flush=True)
        try:
            with output.open("wb") as stdout, error.open("wb") as stderr:
                child = subprocess.Popen(step["arguments"], env=env, cwd=args.work, stdout=stdout, stderr=stderr,
                                         creationflags=subprocess.CREATE_NO_WINDOW)
                deadline = min(started + CUTOFF, began + 20)
                while child.poll() is None:
                    receipt["powerSamples"].append(power())
                    if time.monotonic() >= deadline:
                        raise TimeoutError(name + " exceeded its original deadline")
                    try:
                        child.wait(timeout=min(0.5, max(0.001, deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
                step["exitCode"] = child.returncode
        finally:
            if child is not None and child.poll() is None:
                cleanup = {"treeTerminationReported": False, "parentExited": False}
                step["cleanup"] = cleanup
                try:
                    stopped = subprocess.run(["taskkill.exe", "/PID", str(child.pid), "/T", "/F"], timeout=5,
                                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                             creationflags=subprocess.CREATE_NO_WINDOW, check=False)
                    cleanup["treeTerminationReported"] = stopped.returncode == 0
                except (OSError, subprocess.TimeoutExpired):
                    pass
                finally:
                    if child.poll() is None:
                        child.kill()
                    try:
                        child.wait(timeout=3)
                        cleanup["parentExited"] = True
                    except subprocess.TimeoutExpired:
                        pass
            for path in (output, error):
                if path.exists():
                    text = path.read_text(encoding="utf-8", errors="replace")
                    for key in ("GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"):
                        if env.get(key):
                            text = text.replace(env[key], "[redacted credential]")
                    path.write_text(text, encoding="utf-8")
            step.update(powerAfter=power(), elapsedSeconds=time.monotonic() - began)
            save()
        text = output.read_text(encoding="utf-8")
        if step["exitCode"] and not allow_404:
            raise RuntimeError(name + " failed; no retry; inspect retained metadata/logs")
        return text, step

    def api(name, path, payload=None, absent=False):
        command = [GH, "api", "--hostname", "github.com", "-H", "X-GitHub-Api-Version: 2022-11-28", "--include"]
        if payload is not None:
            request = args.work / (name + "-request.json")
            request.write_text(json.dumps(payload) + "\n", encoding="utf-8")
            command += ["--method", "POST", "--input", request]
        text, step = run(name, [*command, f"repos/{REPO}/{path}"], allow_404=absent)
        match = re.match(r"HTTP/\S+\s+(\d{3})[^\n]*\n", text)
        if match is None:
            raise RuntimeError("Missing official HTTP status")
        status = int(match[1])
        body = re.split(r"\r?\n\r?\n", text, maxsplit=1)[-1]
        if absent and status == 404 and step["exitCode"] != 0:
            return None
        if step["exitCode"] != 0 or status not in (200, 201):
            raise RuntimeError(name + " did not return the required official status")
        value = json.loads(body)
        (args.work / (name + ".json")).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
        return value

    def release_metadata(name):
        releases = api(name + "-list", "releases?per_page=100")
        if not isinstance(releases, list) or len(releases) >= 100:
            raise RuntimeError("Release collision inventory is incomplete or malformed")
        matches = [item for item in releases if item.get("tag_name") == TAG]
        if len(matches) != 1:
            raise RuntimeError("Exactly one newly created release must match the tag")
        return api(name, "releases/" + str(matches[0]["id"]))

    def verify_release(value, draft, assets):
        if (value.get("tag_name") != TAG or value.get("target_commitish") != REVISION
                or value.get("draft") is not draft or value.get("prerelease") is not True
                or len(value.get("assets", [])) != 3):
            raise RuntimeError("Official prerelease source/state/inventory differs")
        for name, identity in assets.items():
            matches = [item for item in value["assets"] if item.get("name") == name]
            if (len(matches) != 1 or matches[0].get("size") != identity["bytes"]
                    or matches[0].get("digest") != "sha256:" + identity["sha256"] or matches[0].get("state") != "uploaded"
                    or not draft and matches[0].get("browser_download_url") != f"https://github.com/{REPO}/releases/download/{TAG}/{name}"):
                raise RuntimeError("Official uploaded asset differs from the verified local input")

    try:
        save()
        copies = {}
        for name, source in (("bundle-proof.json", args.bundle_proof), ("bundle-clock.json", args.bundle_clock),
                             ("crate-receipt.json", args.crate_receipt), ("github-release-notes.md", args.notes),
                             ("deliver.py", Path(__file__)), ("workload-plan.json", Path(__file__).with_name("workload-plan.json"))):
            check()
            destination = args.work / name
            shutil.copyfile(source, destination)
            copies[name] = digest(destination)
        proof = json.loads((args.work / "bundle-proof.json").read_bytes())
        clock = json.loads((args.work / "bundle-clock.json").read_bytes())
        crate = json.loads((args.work / "crate-receipt.json").read_bytes())
        if (proof.get("passed") is not True or proof.get("sourceRevision") != REVISION or proof.get("version") != VERSION
                or str(proof.get("runId")) != str(RUN) or str(proof.get("runAttempt")) != "1"
                or proof.get("workflowFile") != "foundation-bundle.yml" or proof.get("runConclusion") != "success"
                or proof.get("finalSourceChecksPassed") is not True or proof.get("productionLinkingPassed") is not True
                or proof.get("fullFoundationQualified") is not False):
            raise RuntimeError("Exact hosted bundle acceptance is missing")
        elapsed = clock.get("elapsedSecondsThroughRetention")
        if (clock.get("maximumTotalSeconds") != 360 or clock.get("clockClosed") is not True
                or clock.get("withinDeclaredAllowance") is not True or clock.get("retentionComplete") is not True
                or clock.get("cleanupConfirmed") is not True or clock.get("cleanupIncomplete", False) is not False
                or not isinstance(elapsed, (int, float)) or not math.isfinite(elapsed) or not 0 <= elapsed <= 360):
            raise RuntimeError("Hosted operator clock/cleanup is not accepted")
        if (crate.get("status") != "passed" or crate.get("publicationOutcome") != "verified-version-and-checksum"
                or crate.get("sourceRevision") != REVISION or crate.get("version") != VERSION
                or crate.get("publication", {}).get("checksum") != CRATE_SHA
                or crate.get("packageAudit", {}).get("sha256") != CRATE_SHA
                or crate.get("operatorInputSha256", {}).get("verified-bundle.json") != copies["bundle-proof.json"]):
            raise RuntimeError("Actual verified registry publication is missing")
        inputs = {STEM + ".zip": Path(proof["bundleArchivePath"]), STEM + ".receipt.json": Path(proof["receiptPath"]),
                  "bundle-scope.json": Path(proof["scopePath"])}
        assets = {}
        (args.work / "assets").mkdir()
        for name, source in inputs.items():
            check()
            target = args.work / "assets" / name
            shutil.copyfile(source, target)
            assets[name] = {"bytes": target.stat().st_size, "sha256": digest(target)}
        if assets[STEM + ".zip"]["sha256"] != proof["bundleArchiveSha256"]:
            raise RuntimeError("Actual ZIP changed after hosted verification")
        producer = json.loads((args.work / "assets" / (STEM + ".receipt.json")).read_bytes())
        scope = json.loads((args.work / "assets" / "bundle-scope.json").read_bytes())
        if (any(value.get("SourceRevision") != REVISION or str(value.get("RunId")) != str(RUN)
                or str(value.get("RunAttempt")) != "1" or value.get("Version") != VERSION
                or value.get("Target") != "aarch64-pc-windows-msvc" for value in (producer, scope))
                or producer.get("Repository") != "https://github.com/" + REPO
                or producer.get("Toolchain") != "nightly-2026-11-09-aarch64-pc-windows-msvc"
                or producer.get("Archive", {}).get("Sha256") != assets[STEM + ".zip"]["sha256"]
                or producer["Archive"].get("Bytes") != assets[STEM + ".zip"]["bytes"]
                or producer["Archive"].get("Name") != STEM + ".zip"
                or scope.get("ReceiptControlsPassed") != 9 or scope.get("FinalSourceChecksPassed") is not True
                or scope.get("ProductionLinkingPassed") is not True
                or any(scope.get(key) is not False for key in ("FullFoundationQualified", "ModelAcceptanceQualified", "PerformanceQualified"))):
            raise RuntimeError("Actual producer/scope receipt differs")
        receipt.update(inputSha256=copies, assets=assets, githubCli={"path": str(GH), "sha256": digest(GH)})
        if api("tag-before", "git/ref/tags/" + TAG, absent=True) is not None:
            raise RuntimeError("Existing tag collision; do not move it")
        releases = api("releases-before", "releases?per_page=100")
        if not isinstance(releases, list) or len(releases) >= 100 or any(item.get("tag_name") == TAG for item in releases):
            raise RuntimeError("Existing or ambiguous release collision")
        check(45)
        receipt.update(tagCreationAttempted=True, partialPublicationState="tag-creation-pending")
        save()
        tag = api("created-tag", "git/refs", {"ref": "refs/tags/" + TAG, "sha": REVISION})
        if tag.get("ref") != "refs/tags/" + TAG or tag.get("object", {}).get("sha") != REVISION or tag["object"].get("type") != "commit":
            raise RuntimeError("Created tag differs from the authorized exact commit")
        receipt.update(tagCreated=True, draftCreationAttempted=True, partialPublicationState="tag-created-draft-pending")
        save()
        run("create-draft", [GH, "release", "create", TAG, "--repo", REPO, "--verify-tag", "--target", REVISION,
                            "--draft", "--prerelease", "--latest=false", "--title", "Prism Inference 0.1.23 development prerelease",
                            "--notes-file", args.work / "github-release-notes.md"])
        receipt.update(draftCreated=True, assetUploadAttempted=True, partialPublicationState="draft-created-assets-pending")
        save()
        run("upload-assets", [GH, "release", "upload", TAG, "--repo", REPO, *[args.work / "assets" / name for name in assets]])
        draft = release_metadata("draft-release")
        verify_release(draft, True, assets)
        receipt["releaseId"] = draft["id"]
        check(15)
        receipt.update(publicationAttempted=True, partialPublicationState="verified-draft-publication-pending")
        save()
        run("publish-prerelease", [GH, "release", "edit", TAG, "--repo", REPO, "--draft=false", "--prerelease", "--latest=false"])
        published = api("published-release", "releases/" + str(draft["id"]))
        verify_release(published, False, assets)
        if not published.get("published_at"):
            raise RuntimeError("Published prerelease lacks its official publication time")
        receipt.update(status="passed", partialPublicationState="verified-published-prerelease",
                       publishedReleaseUrl=published["html_url"], publishedAt=published["published_at"])
    except Exception as error:
        receipt["failure"] = type(error).__name__ + ": " + str(error)
    finally:
        receipt.update(powerAfter=power(), clockClosed=True, clockRestarted=False,
                       continuousPowerStabilityEstablished=False, fullFoundationQualified=False, performanceQualified=False)
        receipt["cleanupConfirmed"] = all(step["cleanup"].get("treeTerminationReported") is True and step["cleanup"].get("parentExited") is True
                                           for step in receipt["steps"] if "cleanup" in step)
        save()
        shutil.copytree(args.work, args.retain)
        inventory = {str(path.relative_to(args.retain)).replace("\\", "/"): {"bytes": path.stat().st_size, "sha256": digest(path)}
                     for path in args.retain.rglob("*") if path.is_file()}
        (args.retain / "archive.json").write_text(json.dumps({"files": inventory}, indent=2) + "\n", encoding="utf-8")
        final = {"maximumTotalSeconds": MAXIMUM, "elapsedSecondsThroughRetention": time.monotonic() - started,
                 "clockClosed": True, "cleanupConfirmed": receipt["cleanupConfirmed"], "powerAfterRetention": power(), "retries": 0}
        final["withinDeclaredAllowance"] = final["elapsedSecondsThroughRetention"] <= MAXIMUM
        (args.retain / "final-clock.json").write_text(json.dumps(final, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"status": receipt["status"], "failure": receipt.get("failure"),
                          "partialPublicationState": receipt["partialPublicationState"], **final}), flush=True)
    return 0 if receipt["status"] == "passed" and receipt["cleanupConfirmed"] and final["withinDeclaredAllowance"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
