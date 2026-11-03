"""Independent read-only inspection of already downloaded release ZIP data.

No extraction, child process, executable loading, model, or device query occurs.
The expected outer archive hashes come from authenticated GitHub artifact metadata.
License Git blob IDs were independently read from exact SOURCE using git ls-tree.
"""

import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import zipfile

SOURCE = "9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57"
ROOT = Path(r"C:\tmp\Prism-0114-Release-20261102")
TARGETS = {
    "x64": ("x86_64-pc-windows-msvc", 0x8664, 3452431,
            "a87faae5d19e581a2165af235fe69a9c9593e5cd0501cafc7b09ebbfa8d13782"),
    "arm64": ("aarch64-pc-windows-msvc", 0xAA64, 3184773,
              "bfa0d94aaa6c45d21217581b6c2b5d683839957a01164512292c3a471640dac7"),
}
LICENSES = {
    "LICENSE": "d645695673349e3947e8e5ae42332d0ac3164cd7",
    "NOTICE": "da29014a060851551ccb45abeebbaf3826c7a83b",
    "PRISM-LICENSE.txt": "e7dca554bcb802f98408383a864404e3aa4eacca",
    "CAMELID-LICENSE.txt": "850ad9b5087eb7acbf69624004bf1d515de590dc",
}
EXECUTABLES = {"prism-inference.exe", "bonsai-token-worker.exe", "bonsai-swift-worker.exe",
               "prism-inference-server.exe", "bonsai-gguf.exe"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def main():
    label = sys.argv[1]
    target, machine, expected_size, expected_sha = TARGETS[label]
    directory = ROOT / label
    stem = "prism-inference-0.1.14-" + target
    outer_path = directory / "github-artifact.zip"
    outer = outer_path.read_bytes()
    require((len(outer), sha(outer)) == (expected_size, expected_sha), "GitHub archive differs")
    with zipfile.ZipFile(io.BytesIO(outer)) as zipped:
        require(len(zipped.infolist()) == 2 and set(zipped.namelist()) ==
                {stem + ".zip", stem + ".receipt.json"}, "outer member inventory differs")
        archive = zipped.read(stem + ".zip")
        receipt_raw = zipped.read(stem + ".receipt.json")
    require(archive == (directory / (stem + ".zip")).read_bytes(), "inner disk ZIP differs")
    require(receipt_raw == (directory / (stem + ".receipt.json")).read_bytes(), "disk receipt differs")
    receipt = json.loads(receipt_raw, object_pairs_hook=unique)
    require(receipt["SchemaVersion"] == 1 and receipt["SourceRevision"] == SOURCE
            and receipt["Version"] == "0.1.14" and receipt["Target"] == target
            and receipt["Repository"] == "https://github.com/symbiogenesis/prism-inference"
            and receipt["Toolchain"] == "nightly-2026-10-22-" + target
            and receipt["RunId"] == "41580496421" and receipt["RunAttempt"] == "1",
            "receipt identity differs")
    require(receipt["Archive"] == {"Name": stem + ".zip", "Bytes": len(archive), "Sha256": sha(archive)},
            "inner archive receipt differs")
    records = {record["Name"]: record for record in receipt["Files"]}
    require(len(receipt["Files"]) == 9 and set(records) == EXECUTABLES | LICENSES.keys(),
            "receipt inventory differs")
    observations = []
    with zipfile.ZipFile(io.BytesIO(archive)) as zipped:
        require(len(zipped.infolist()) == 9 and len(set(zipped.namelist())) == 9
                and set(zipped.namelist()) == set(records), "inner member inventory differs")
        for name in sorted(records):
            data = zipped.read(name)
            require(records[name] == {"Name": name, "Bytes": len(data), "Sha256": sha(data)},
                    "raw member digest differs")
            observation = {"name": name, "bytes": len(data), "sha256": sha(data)}
            if name in EXECUTABLES:
                pe = struct.unpack_from("<I", data, 0x3C)[0]
                require(data[:2] == b"MZ" and data[pe:pe+4] == b"PE\0\0"
                        and struct.unpack_from("<H", data, pe+4)[0] == machine
                        and struct.unpack_from("<H", data, pe+24)[0] == 0x20B,
                        "PE identity differs")
                observation["peMachine"] = hex(machine)
            else:
                require(b"\r\n" in data and b"\r" not in data.replace(b"\r\n", b"")
                        and b"\n" not in data.replace(b"\r\n", b""), "non-strict checkout newline conversion")
                source_data = data.replace(b"\r\n", b"\n")
                source_blob = hashlib.sha1(b"blob " + str(len(source_data)).encode() + b"\0" + source_data).hexdigest()
                require(source_blob == LICENSES[name], "source Git license blob differs")
                observation["sourceGitBlobSha1AfterStrictCrLfToLf"] = source_blob
            observations.append(observation)
    require(outer_path.read_bytes() == outer, "outer archive changed during inspection")
    producer = json.loads((directory / "data-review.json").read_bytes(), object_pairs_hook=unique)
    require(producer["status"] == "passed" and producer["sourceRevision"] == SOURCE
            and producer["archive"]["Sha256"] == sha(archive), "producer report identity differs")
    require(all(any(item["name"] == observation["name"] and item["bytes"] == observation["bytes"]
                    and item["sha256"] == observation["sha256"] for item in producer["files"])
                for observation in observations), "producer member result differs")
    print(json.dumps({"status": "passed", "kind": "independent-data-only-release-inspection",
                      "sourceRevision": SOURCE, "target": target,
                      "githubOuterBytes": len(outer), "githubOuterSha256": sha(outer),
                      "archiveBytes": len(archive), "archiveSha256": sha(archive),
                      "receiptBytes": len(receipt_raw), "receiptSha256": sha(receipt_raw),
                      "files": observations, "producerDataReportSha256": sha((directory / "data-review.json").read_bytes()),
                      "archiveExecutablesRun": 0, "modelCommands": 0, "deviceQueries": 0,
                      "originalOwnerClockReopened": False,
                      "scope": "Read-only archive/receipt/member/PE/source-license inspection. CI provenance is independently reviewed separately; no model qualification or binary-to-source attestation."}, indent=2))


if __name__ == "__main__":
    main()
