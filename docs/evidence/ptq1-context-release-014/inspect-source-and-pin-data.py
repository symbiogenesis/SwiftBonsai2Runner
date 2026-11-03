"""Compare retained source ZIP and prepared pin to Git tree and reviewed receipts.

Pure file/ZIP/JSON hashing only; no extraction, subprocess, executable loading,
source execution, model command, network access, or device query.
"""

import hashlib
import json
from pathlib import Path
import stat
import zipfile

SOURCE = "9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57"
EVIDENCE = Path(__file__).parent
ROOT = Path(r"C:\tmp\Prism-0114-Release-20261102")
PIN_PATH = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\ptq1-close-contract\config\prism-inference.lock.json")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    archive = ROOT / "source" / (SOURCE + ".zip")
    data = archive.read_bytes()
    require((len(data), sha(data)) ==
            (4431974, "4f92deb95eb4905b2c9c5558a4607843ae4db61c2d686ec95ad667470d03896e"),
            "source ZIP identity differs")
    tree = {}
    tree_raw = (EVIDENCE / "source-git-tree.txt").read_bytes()
    for line in tree_raw.decode("utf-8").splitlines():
        identity, path = line.split("\t")
        mode, kind, blob = identity.split(" ")
        require(mode in {"100644", "100755"} and kind == "blob" and path not in tree,
                "unexpected Git tree record")
        tree[path] = blob
    prefix = "prism-inference-" + SOURCE + "/"
    observed = {}
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        names = [entry.filename for entry in entries]
        require(len(entries) == 775 and len(set(names)) == 775
                and len({name.casefold() for name in names}) == 775, "entry inventory differs")
        for entry in entries:
            require(entry.filename.startswith(prefix) and "\\" not in entry.filename
                    and not any(part in {".", ".."} for part in entry.filename.split("/")),
                    "unsafe or wrong-prefix source ZIP path")
            require(not stat.S_ISLNK(entry.external_attr >> 16), "source ZIP symlink")
            if entry.is_dir():
                continue
            relative = entry.filename[len(prefix):]
            require(relative in tree and entry.file_size <= 128 * 1024 * 1024,
                    "unexpected or oversized source file")
            content = zipped.read(entry)
            blob = hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()
            require(blob == tree[relative], "source ZIP file differs from exact Git blob: " + relative)
            observed[relative] = blob
    require(len(observed) == len(tree) == 693 and set(observed) == set(tree), "incomplete source tree")
    require(archive.read_bytes() == data, "source ZIP changed during inspection")
    pin_raw = PIN_PATH.read_bytes()
    pin = json.loads(pin_raw)
    arm = json.loads((EVIDENCE / "arm64-receipt.json").read_bytes())
    require(pin["Source"] == {
        "Bytes": len(data), "Commit": SOURCE, "Repository": "https://github.com/symbiogenesis/prism-inference",
        "Sha256": sha(data), "Url": "https://github.com/symbiogenesis/prism-inference/archive/" + SOURCE + ".zip"
    }, "prepared source pin differs")
    require(pin["schemaVersion"] == 1 and pin["version"] == "0.1.14" and pin["release"] == "v0.1.14"
            and pin["commit"] == SOURCE and pin["package"] == "prism-inference"
            and pin["target"] == "aarch64-pc-windows-msvc"
            and pin["repository"] == "https://github.com/symbiogenesis/prism-inference", "prepared pin identity differs")
    require(pin["asset"] == {
        "bytes": arm["Archive"]["Bytes"], "filename": arm["Archive"]["Name"],
        "sha256": arm["Archive"]["Sha256"],
        "url": "https://github.com/symbiogenesis/prism-inference/releases/download/v0.1.14/" + arm["Archive"]["Name"]
    }, "prepared release asset pin differs")
    expected_files = {item["Name"]: (item["Bytes"], item["Sha256"]) for item in arm["Files"]}
    locked_files = {item["name"]: (item["bytes"], item["sha256"]) for item in pin["files"]}
    require(len(pin["files"]) == 9 and expected_files == locked_files, "prepared member pin differs")
    print(json.dumps({
        "schemaVersion": 1, "kind": "independent-source-archive-and-prepared-pin-data-review",
        "status": "passed-data-checks-publication-and-owning-ci-required", "sourceRevision": SOURCE,
        "sourceArchiveBytes": len(data), "sourceArchiveSha256": sha(data),
        "sourceArchiveEntries": 775, "sourceFilesMatchedToExactGitBlob": len(observed),
        "sourceGitTreeReceiptSha256": sha(tree_raw), "preparedPinSha256": sha(pin_raw),
        "preparedPinBytes": len(pin_raw), "lockedReleaseMembersMatched": 9,
        "releasePublishedByThisReview": False, "publicationVerifiedByThisReview": False,
        "sourceOrReleaseFilesExecuted": 0, "modelCommands": 0, "deviceQueries": 0,
        "originalOwnerClockReopened": False, "historicalQualificationTransferred": False,
        "scope": "Read-only comparison to exact Git object IDs and already independently reviewed ARM64 receipt; publication, owning hosted CI and later Runner integration require separate review."
    }, indent=2))


if __name__ == "__main__":
    main()
