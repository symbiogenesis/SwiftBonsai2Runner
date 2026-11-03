"""Hosted-only integrity and comparator replay for one retained numerical capture."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import tempfile
import urllib.parse
import urllib.request
import zipfile

REPOSITORY = "symbiogenesis/SwiftBonsai2Runner"
REVISION = "469f57b233898fd10b97d5ebc10aaef78b940e30"
RUNTIME_REVISION = "cae16fa6998dac58a947eba9ae4398ed6a9fb863"
RUN = 41568281287
ARTIFACT = 13200218351
ARCHIVE_BYTES = 294092332
ARCHIVE_SHA = "c31de3e837f4d4d43df5d144892c0b5c4b36d747ab763799abe0556b419af8d8"
COMPARATOR_SHA = "b8372072b3d77a918c5c6fa7038da95f51a50701f6927fb4b4931a591b056c08"
WORKFLOW = ".github/workflows/native-ptq1-numerics.yml"
ARTIFACT_NAME = f"native-ptq1-numerics-{RUN}-1"


class AuditError(ValueError):
    """A fixed, credential-free rejection reason."""


def require(condition, message):
    if not condition:
        raise AuditError(message)


def sha(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def validate_producer(run, artifact, jobs):
    require(run["id"] == RUN and run["head_sha"] == REVISION and run["head_branch"] == "main"
            and run["event"] == "workflow_dispatch" and run["run_attempt"] == 1
            and run["path"] == WORKFLOW and run["status"] == "completed"
            and run["conclusion"] == "success" and run["repository"]["full_name"] == REPOSITORY,
            "inexact original producer")
    require(artifact["id"] == ARTIFACT and artifact["name"] == ARTIFACT_NAME
            and artifact["size_in_bytes"] == ARCHIVE_BYTES and artifact["expired"] is False
            and artifact["digest"] == "sha256:" + ARCHIVE_SHA
            and artifact["workflow_run"]["id"] == RUN
            and artifact["workflow_run"]["head_sha"] == REVISION
            and artifact["workflow_run"]["head_branch"] == "main"
            and artifact["workflow_run"]["repository_id"] == run["repository"]["id"],
            "inexact original artifact")
    require(jobs["total_count"] == len(jobs["jobs"]) == 2
            and {job["name"] for job in jobs["jobs"]} == {"prepare", "numerics"}
            and all(job["status"] == "completed" and job["conclusion"] == "success"
                    and job["run_id"] == RUN and job["head_sha"] == REVISION for job in jobs["jobs"]),
            "original jobs did not complete successfully")
    numerical = next(job for job in jobs["jobs"] if job["name"] == "numerics")
    require(numerical["labels"] == [f"ptq1-numerics-{RUN}"], "inexact original owner label")


class ArtifactRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        target = urllib.parse.urlsplit(url)
        require(target.scheme == "https" and target.hostname
                and target.hostname.endswith((".blob.core.windows.net", ".githubusercontent.com"))
                and not target.username and not target.password, "unapproved artifact redirect")
        redirected = super().redirect_request(request, response, code, message, headers, url)
        redirected.remove_header("Authorization")
        return redirected


def fetch(endpoint, token, destination=None):
    request = urllib.request.Request(f"https://api.github.com/repos/{REPOSITORY}/{endpoint}",
                                     headers={"Authorization": "Bearer " + token,
                                              "Accept": "application/vnd.github+json",
                                              "X-GitHub-Api-Version": "2022-11-28"})
    with urllib.request.build_opener(ArtifactRedirect()).open(request, timeout=120) as response:
        if destination is None:
            data = response.read(1048577)
            require(len(data) <= 1048576, "oversized API metadata")
            return json.loads(data)
        total, digest = 0, hashlib.sha256()
        with destination.open("xb") as output:
            while chunk := response.read(1048576):
                total += len(chunk)
                require(total <= ARCHIVE_BYTES, "oversized evidence archive")
                digest.update(chunk)
                output.write(chunk)
        require(total == ARCHIVE_BYTES and digest.hexdigest() == ARCHIVE_SHA,
                "evidence archive identity differs")
    return None


def checked_inventory(path):
    inventory = json.loads(Path(path).read_text(encoding="utf-8"))
    require(inventory["sourceRunId"] == str(RUN) and inventory["sourceRevision"] == REVISION,
            "expected inventory producer differs")
    files = inventory["files"]
    require(len(files) == 155 and len({file["path"] for file in files}) == 155
            and sum(file["bytes"] for file in files) == 316267936, "expected inventory extent differs")
    return {file["path"]: file for file in files}


def extract_checked(archive, destination, expected):
    manifest, seen = [], set()
    with zipfile.ZipFile(archive) as bundle:
        require(len(bundle.infolist()) == len(expected), "archive member count differs")
        for member in bundle.infolist():
            name = member.filename
            relative = PurePosixPath(name)
            require(name == member.orig_filename and len(name) <= 200 and "\\" not in name
                    and ":" not in name and not relative.is_absolute() and str(relative) == name
                    and all(part not in (".", "..") for part in relative.parts)
                    and name in expected and name.casefold() not in seen,
                    "unsafe, unexpected or duplicate archive member")
            seen.add(name.casefold())
            require(not member.is_dir() and not member.flag_bits & 1
                    and not member.external_attr & 0x10
                    and stat.S_IFMT(member.external_attr >> 16) in (0, stat.S_IFREG)
                    and member.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
                    and member.file_size == expected[name]["bytes"]
                    and 0 <= member.file_size <= 536870912, "invalid archive member type or extent")
            target = destination.joinpath(*relative.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            total, digest = 0, hashlib.sha256()
            with bundle.open(member) as source, target.open("xb") as output:
                while chunk := source.read(1048576):
                    total += len(chunk)
                    require(total <= member.file_size, "expanded member exceeds declared extent")
                    digest.update(chunk)
                    output.write(chunk)
            require(total == member.file_size and digest.hexdigest() == expected[name]["sha256"],
                    "retained member hash differs")
            manifest.append({"path": name, "bytes": total, "sha256": digest.hexdigest()})
    require({file["path"] for file in manifest} == set(expected), "missing retained member")
    return sorted(manifest, key=lambda file: file["path"])


def replay(root, comparator):
    require(sha(comparator) == COMPARATOR_SHA, "original comparator source differs")
    original = json.loads((root / "comparison.json").read_bytes())
    owner = json.loads((root / "numerics.json").read_bytes())
    require(owner["Revision"] == REVISION and str(owner["RunAttempt"]) == "1"
            and owner["RunUrl"].endswith(f"/actions/runs/{RUN}")
            and owner["Status"] == "completed-one-token-smoke" and owner["Phase"] == "completed"
            and owner["Comparison"] == original and owner["DefaultMayChange"] is False
            and owner["Plan"]["RustSource"]["commit"] == RUNTIME_REVISION
            and owner["Plan"]["RustBackend"] == "neon" and owner["Plan"]["RustRowWorkers"] == 4
            and owner["Plan"]["AdapterCondition"] == "standard" and owner["Plan"]["AdapterScale"] == 1.3,
            "retained owner/comparison binding differs")
    require(original["status"] == "passed-bounded-one-token-smoke" and original["files"] == 129
            and original["elements"] == 39504384 and original["byteExactFiles"] == 129
            and original["failedElements"] == 0 and original["defaultMayChange"] is False
            and original["backendRequested"] == "neon" and original["rustRowWorkers"] == 4
            and original["adapterCondition"] == "standard" and original["adapterScale"] == 1.3,
            "retained selected comparison differs")
    spec = importlib.util.spec_from_file_location("original_native_compare", comparator)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    recomputed = module.compare(root / "native", root / "neon", "neon", 4, "standard")
    require(recomputed == original, "replayed comparison differs from original report")
    return recomputed


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--comparator", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    arguments.output.mkdir(parents=True, exist_ok=False)
    audit = {"schemaVersion": 1, "status": "failed", "stage": "hosted-guard",
             "sourceRunId": str(RUN), "sourceRevision": REVISION, "artifactId": str(ARTIFACT),
             "artifactSha256": ARCHIVE_SHA, "comparatorSourceSha256": COMPARATOR_SHA,
             "auditRevision": os.environ.get("GITHUB_SHA"), "auditRunId": os.environ.get("GITHUB_RUN_ID"),
             "ownerDeviceExecutedByAudit": False, "modelExecutedByAudit": False, "defaultMayChange": False}
    try:
        require(os.environ.get("GITHUB_ACTIONS") == "true"
                and os.environ.get("GITHUB_REPOSITORY") == REPOSITORY
                and os.environ.get("GITHUB_REF") == "refs/heads/main"
                and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted", "hosted main only")
        token = os.environ["GH_TOKEN"]
        audit["stage"] = "producer-metadata"
        run = fetch(f"actions/runs/{RUN}", token)
        artifact = fetch(f"actions/artifacts/{ARTIFACT}", token)
        jobs = fetch(f"actions/runs/{RUN}/jobs?filter=all&per_page=100", token)
        validate_producer(run, artifact, jobs)
        write_json(arguments.output / "producer.json", {"run": run, "artifact": artifact, "jobs": jobs})
        with tempfile.TemporaryDirectory(prefix="ptq1-retained-", dir=os.environ["RUNNER_TEMP"]) as scratch:
            archive, extracted = Path(scratch) / "evidence.zip", Path(scratch) / "extracted"
            audit["stage"] = "archive-download"
            fetch(f"actions/artifacts/{ARTIFACT}/zip", token, archive)
            audit["stage"] = "retained-member-integrity"
            inventory = Path(__file__).with_name("fixtures") / f"retained-{RUN}.inventory.json"
            manifest = extract_checked(archive, extracted, checked_inventory(inventory))
            write_json(arguments.output / "manifest.json", {"sourceRunId": str(RUN), "sourceRevision": REVISION,
                                                           "files": manifest})
            audit["stage"] = "original-comparator-replay"
            comparison = replay(extracted, arguments.comparator)
            (arguments.output / "original-comparison.json").write_bytes((extracted / "comparison.json").read_bytes())
            write_json(arguments.output / "replayed-comparison.json", comparison)
            audit.update(retainedFiles=len(manifest), retainedBytes=sum(file["bytes"] for file in manifest),
                         comparedFiles=comparison["files"], comparedValues=comparison["elements"],
                         byteExactFiles=comparison["byteExactFiles"], runtimeRevision=RUNTIME_REVISION,
                         modelSha256=comparison["modelSha256"], adapterSha256=comparison["adapterSha256"],
                         nativeCaptureSha256=comparison["nativeCaptureSha256"],
                         nativeStateSha256=comparison["nativeStateSha256"], rustStdoutSha256=comparison["rustStdoutSha256"],
                         originalExecutionUnchanged=True, fullContextQualified=False,
                         scope="Retained data integrity and replay only; no new model/device execution, default promotion, adapter provenance, quality or full-context acceptance.")
            audit["stage"] = "cloud-scratch-cleanup"
        audit.update(status="passed-retained-capture-integrity-and-replay", stage="complete")
    except Exception as error:  # Fixed classification excludes signed URLs and credentials.
        audit["status"] = "failed"
        audit["failureClass"] = type(error).__name__
        if isinstance(error, AuditError):
            audit["failureReason"] = str(error)
    write_json(arguments.output / "audit.json", audit)
    print(f"Retained evidence audit: {audit['status']}; stage: {audit['stage']}")
    return 0 if audit["status"].startswith("passed-") else 1


if __name__ == "__main__":
    raise SystemExit(main())
