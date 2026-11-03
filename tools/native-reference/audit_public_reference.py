"""Offline retained public-reference integrity and independent A/B metric replay.

Uses only the Python standard library. The model pack is neither opened nor
downloaded; matrix reconstruction and producer authentication remain separate.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import struct
import sys

BLOCKS = tuple(block for block in range(64) if (block + 1) % 4 != 0)
ARITHMETIC = "public-mlx-f16-inverse1024-f16-f32-dot-v1"
REPOSITORY = "symbiogenesis/SwiftBonsai2Runner"
WORKFLOW = ".github/workflows/ptq1-public-authoring-reference.yml"
REPLAY_SOURCE_FILES = (
    "config/swift-lora.lock.json", "config/swift-lora.direction.json",
    "tools/native-reference/public-authoring-pins.json",
    "tools/native-reference/public_pack_reference.py",
    "tools/native-reference/public_pack_formats.py",
    "tools/native-reference/audit_public_reference.py",
)


class Rejected(ValueError):
    """Retained evidence differs from the independently selected contract."""


def require(condition, message):
    if not condition:
        raise Rejected(message)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def bounded(path, maximum, exact=None, sha256=None):
    with Path(path).open("rb") as source:
        raw = source.read(maximum + 1)
    require(len(raw) <= maximum and (exact is None or len(raw) == exact),
            "retained file byte extent differs")
    require(sha256 is None or digest(raw) == sha256, "retained file digest differs")
    return raw


def strict_json(raw):
    def distinct(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate retained JSON key")
            result[key] = value
        return result

    def finite(value):
        number = float(value)
        require(math.isfinite(number), "nonfinite retained JSON number")
        return number

    return json.loads(raw.decode("utf-8"), object_pairs_hook=distinct,
                      parse_float=finite, parse_constant=finite)


def json_file(path, maximum=1024 * 1024):
    return strict_json(bounded(path, maximum))


def replay_source(evidence, report, revision):
    """Read the sealed finite source selection as data; execute none of it."""
    fields = ("replaySourceManifestFile", "replaySourceManifestSha256")
    present = [field in report for field in fields]
    require(all(present) or not any(present), "incomplete retained source manifest binding")
    if not any(present):
        require(not (evidence / "replay-source").exists(), "retained source binding was removed")
        return None  # Historical reports require their original reviewer checkout.
    require(report[fields[0]] == "replay-source/source-manifest.json"
            and type(report[fields[1]]) is str
            and re.fullmatch(r"[0-9a-f]{64}", report[fields[1]]) is not None,
            "retained source manifest selection differs")
    bundle = evidence / "replay-source"
    require(bundle.resolve().is_relative_to(evidence.resolve()), "retained source escapes evidence")
    directories = {
        "": {"config": True, "tools": True, "source-manifest.json": False},
        "config": {"swift-lora.lock.json": False, "swift-lora.direction.json": False},
        "tools": {"native-reference": True},
        "tools/native-reference": {Path(name).name: False for name in REPLAY_SOURCE_FILES
                                   if name.startswith("tools/native-reference/")},
    }
    reparse = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    for relative, expected in directories.items():
        directory = bundle / relative
        metadata = directory.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and not stat.S_ISLNK(metadata.st_mode)
                and not (getattr(metadata, "st_file_attributes", 0) & reparse),
                "retained source directory is redirected or not a directory")
        actual = set()
        with os.scandir(directory) as entries:
            for entry in entries:
                require(entry.name in expected and entry.name not in actual,
                        "retained source contains an unexpected entry")
                actual.add(entry.name)
                metadata = entry.stat(follow_symlinks=False)
                require(not stat.S_ISLNK(metadata.st_mode)
                        and not (getattr(metadata, "st_file_attributes", 0) & reparse)
                        and (stat.S_ISDIR(metadata.st_mode) if expected[entry.name]
                             else stat.S_ISREG(metadata.st_mode)),
                        "retained source entry is redirected or has the wrong type")
        require(actual == set(expected), "retained source inventory is incomplete")
    manifest = strict_json(bounded(bundle / "source-manifest.json", 65536,
                                   sha256=report[fields[1]]))
    require(type(manifest) is dict and set(manifest) == {"schemaVersion", "kind", "sourceRevision", "files"}
            and manifest["schemaVersion"] == 1 and manifest["kind"] == "public-reference-replay-source"
            and manifest["sourceRevision"] == revision and type(manifest["files"]) is list
            and len(manifest["files"]) == len(REPLAY_SOURCE_FILES),
            "retained source manifest identity differs")
    sources = {}
    for item in manifest["files"]:
        require(type(item) is dict and set(item) == {"path", "bytes", "sha256"}
                and type(item["path"]) is str and item["path"] in REPLAY_SOURCE_FILES
                and item["path"] not in sources and type(item["bytes"]) is int
                and 0 < item["bytes"] <= 1024 * 1024 and type(item["sha256"]) is str
                and re.fullmatch(r"[0-9a-f]{64}", item["sha256"]) is not None,
                "retained source file declaration differs")
        sources[item["path"]] = bounded(bundle / item["path"], 1024 * 1024,
                                        item["bytes"], item["sha256"])
    require(set(sources) == set(REPLAY_SOURCE_FILES), "retained source manifest inventory differs")
    return sources


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def vector(raw, count):
    require(len(raw) == 4 * count, "retained F32 vector extent differs")
    values = struct.unpack(f"<{count}f", raw)
    require(all(math.isfinite(value) for value in values), "nonfinite retained F32 vector")
    return values


def metrics(actual, reference, absolute=1e-5, relative=1e-5):
    require(len(actual) == len(reference), "comparison vector lengths differ")
    differences = [abs(left - right) for left, right in zip(actual, reference, strict=True)]
    return {"comparedElements": len(reference),
            "mismatchedElements": sum(difference > math.fma(relative, abs(expected), absolute)
                                      for difference, expected in zip(differences, reference, strict=True)),
            "maximumAbsoluteDifference": max(differences, default=0.0)}


def retained_descriptors(evidence, report, pins):
    config = bounded(evidence / "config.json", pins["config"]["bytes"],
                     pins["config"]["bytes"], pins["config"]["sha256"])
    require(type(strict_json(config)) is dict, "retained config root differs")
    raw = bounded(evidence / "model-header.json", 4 * 1024 * 1024,
                  report["modelHeaderBytes"], report["modelHeaderSha256"])
    require(report["modelHeaderFile"] == "model-header.json", "retained header filename differs")
    header = strict_json(raw)
    require(type(header) is dict, "retained model header root differs")
    header.pop("__metadata__", None)
    require(len(header) == report["modelTensorCount"] and 0 < len(header) <= 8192,
            "retained model tensor inventory differs")
    widths = {"BOOL": 1, "U8": 1, "I8": 1, "U16": 2, "I16": 2, "F16": 2,
              "BF16": 2, "U32": 4, "I32": 4, "F32": 4, "U64": 8, "I64": 8, "F64": 8}
    base, extents, descriptors = len(raw) + 8, [], {}
    for name, item in header.items():
        require(type(item) is dict and set(item) == {"dtype", "shape", "data_offsets"},
                "retained model descriptor schema differs")
        shape, offsets, dtype = item["shape"], item["data_offsets"], item["dtype"]
        require(type(shape) is list and len(shape) <= 8
                and all(type(value) is int and 0 <= value <= 1_000_000_000 for value in shape)
                and type(offsets) is list and len(offsets) == 2
                and all(type(value) is int and value >= 0 for value in offsets)
                and dtype in widths, "retained model descriptor types differ")
        begin, end = offsets
        require(begin <= end and end - begin == math.prod(shape) * widths[dtype]
                and base + end <= pins["model"]["bytes"], "retained model descriptor extent differs")
        descriptors[name] = {"dtype": dtype, "shape": shape,
                             "absoluteByteBegin": base + begin, "absoluteByteEnd": base + end}
        if begin != end:
            extents.append((begin, end))
    cursor = 0
    for begin, end in sorted(extents):
        require(begin == cursor, "retained model payload hole or overlap")
        cursor = end
    require(base + cursor == pins["model"]["bytes"], "retained model payload tail differs")
    return descriptors


class Adapter:
    """Independent bounded F32 GGUF reader, without the producer's NumPy parser."""

    def __init__(self, raw):
        self.raw, self.position = raw, 0

    def take(self, count):
        require(0 <= count <= len(self.raw) - self.position, "truncated retained adapter")
        begin = self.position
        self.position += count
        return self.raw[begin:self.position]

    def number(self, fmt):
        return struct.unpack("<" + fmt, self.take(struct.calcsize("<" + fmt)))[0]

    def text(self):
        count = self.number("Q")
        require(count <= 4096, "oversized retained adapter text")
        return self.take(count).decode("utf-8")

    def metadata(self, kind, depth=0):
        require(depth <= 2, "retained adapter metadata nesting differs")
        formats = {0: "B", 1: "b", 2: "H", 3: "h", 4: "I", 5: "i", 6: "f",
                   10: "Q", 11: "q", 12: "d"}
        if kind in formats:
            return self.number(formats[kind])
        if kind == 7:
            value = self.number("B")
            require(value in (0, 1), "invalid retained adapter Boolean")
            return bool(value)
        if kind == 8:
            return self.text()
        require(kind == 9, "unknown retained adapter metadata type")
        element, count = self.number("I"), self.number("Q")
        require(count <= 65536, "retained adapter metadata count differs")
        return [self.metadata(element, depth + 1) for _ in range(count)]

    def factors(self):
        require(self.take(4) == b"GGUF" and self.number("I") == 3, "retained adapter format differs")
        count, metadata_count = self.number("Q"), self.number("Q")
        require(count == 258 and metadata_count <= 128, "retained adapter inventory differs")
        metadata = {}
        for _ in range(metadata_count):
            name = self.text()
            require(name not in metadata, "duplicate retained adapter metadata")
            metadata[name] = self.metadata(self.number("I"))
        require(metadata.get("general.alignment", 32) == 32, "retained adapter alignment differs")
        require(metadata.get("general.architecture") == "qwen35"
                and metadata.get("general.type") == "adapter" and metadata.get("adapter.type") == "lora"
                and metadata.get("adapter.lora_alpha") == 1.0, "retained adapter metadata differs")
        descriptors = {}
        for _ in range(count):
            name = self.text()
            require(name not in descriptors and self.number("I") == 2,
                    "retained adapter tensor name/rank differs")
            shape = (self.number("Q"), self.number("Q"))
            require(all(0 < extent <= 248320 for extent in shape) and self.number("I") == 0,
                    "retained adapter tensor shape/type differs")
            offset = self.number("Q")
            require(offset % 32 == 0, "retained adapter tensor alignment differs")
            descriptors[name] = (shape, offset)
        sites = {"token_embd.weight": (1, 248320)}
        for block in range(64):
            sites[f"blk.{block}.ffn_down.weight"] = (17408, 1)
            family = "attn_output" if (block + 1) % 4 == 0 else "ssm_out"
            sites[f"blk.{block}.{family}.weight"] = (6144, 1)
        expected = {site + suffix: shape if suffix == ".lora_a" else (1, 5120)
                    for site, shape in sites.items() for suffix in (".lora_a", ".lora_b")}
        require(set(descriptors) == set(expected), "retained adapter writer set differs")
        begin, factors, extents = (self.position + 31) // 32 * 32, {}, []
        for name, (shape, offset) in descriptors.items():
            start, end = begin + offset, begin + offset + math.prod(shape) * 4
            require(shape == expected[name] and end <= len(self.raw), "retained adapter writer extent differs")
            factors[name] = self.raw[start:end]
            extents.append((start, end))
        extents.sort()
        require(all(left[1] <= right[0] for left, right in zip(extents, extents[1:])),
                "retained adapter writers overlap")
        return factors


def audit(evidence, adapter_path, direction_path, revision, run_id, run_attempt):
    require(re.fullmatch(r"[0-9a-f]{40}", revision) is not None and
            re.fullmatch(r"[1-9][0-9]*", run_id) is not None and
            re.fullmatch(r"[1-9][0-9]*", run_attempt) is not None,
            "independent producer selection is invalid")
    evidence = Path(evidence)
    report = json_file(evidence / "report.json")
    sources = replay_source(evidence, report, revision)
    repository = Path(__file__).resolve().parents[2]
    def source_raw(relative, maximum=1024 * 1024):
        raw = sources[relative] if sources is not None else bounded(repository / relative, maximum)
        require(0 < len(raw) <= maximum, "retained reviewer source exceeds its byte bound")
        return raw

    pins_raw = source_raw("tools/native-reference/public-authoring-pins.json", 65536)
    pins = strict_json(pins_raw)
    approved_pins = json_file(repository / "tools/native-reference/public-authoring-pins.json", 65536)
    require(pins == approved_pins, "reviewer pins differ from independently selected recipe")
    for filename, field in (("swift-lora.lock.json", "adapter"), ("swift-lora.direction.json", "direction")):
        locked = strict_json(source_raw("config/" + filename))[field]
        approved = json_file(repository / "config" / filename)[field]
        require(all(pins[field][key] == locked[key] == approved[key]
                    for key in ("filename", "bytes", "sha256", "url")),
                "reviewer pins differ from approved repository lock")
    require(report == json_file(evidence / "attempt.json"), "terminal attempt differs from report")
    require(report["schemaVersion"] == 1 and report["kind"] == "independent-public-pack-A-reference"
            and report["arithmeticId"] == ARITHMETIC and report["stage"] == "complete"
            and report["referencePinsSha256"] == digest(pins_raw) and report["inputPins"] == pins,
            "retained recipe/input identity differs")
    require(report["repository"] == REPOSITORY and report["workflowPath"] == WORKFLOW
            and report["sourceRevision"] == revision and report["runId"] == run_id
            and report["runAttempt"] == run_attempt and report["runnerEnvironment"] == "github-hosted",
            "retained producer differs from independent selection")
    for flag in ("authorOriginalProvenanceQualified", "nativeWriterBasisQualified",
                 "logitsOrStateQualified", "defaultMayChange", "fullCheckpointWritten"):
        require(report[flag] is False, "retained result overstates qualification")
    require(report["ownerDeviceTrials"] == report["modelInferenceCommands"] == 0,
            "retained execution scope differs")
    toolchain = json_file(evidence / "toolchain.json")
    for field, filename in (("referenceScriptSha256", "public_pack_reference.py"),
                            ("formatScriptSha256", "public_pack_formats.py")):
        require(toolchain[field] == digest(source_raw("tools/native-reference/" + filename)),
                "retained producer script differs from reviewer source")
    require(toolchain["numpy"] == pins["numpy"] and toolchain["blasThreads"] == 1
            and toolchain["python"].split(maxsplit=1)[0] == pins["python"],
            "retained producer toolchain differs")
    descriptors = retained_descriptors(evidence, report, pins)
    raw = bounded(adapter_path, pins["adapter"]["bytes"], pins["adapter"]["bytes"], pins["adapter"]["sha256"])
    factors = Adapter(raw).factors()
    direction_raw = bounded(direction_path, 20480, 20480, pins["direction"]["sha256"])
    direction = vector(direction_raw, 5120)
    norm = report["directionF32NormBeforeNormalization"]
    require(type(norm) is float and math.isfinite(norm) and norm > 0 and abs(norm - 1) <= 1e-5
            and abs(norm - math.sqrt(math.fsum(value * value for value in direction))) <= 1e-6,
            "retained direction norm is inconsistent")
    normalized = tuple(f32(value / norm) for value in direction)
    require(digest(struct.pack("<5120f", *normalized)) == report["normalizedDirectionSha256"],
            "retained normalized direction digest differs")
    b_report = json_file(evidence / "direction-comparison.json")["sites"]
    expected_b = {name for name in factors if name.endswith(".lora_b")}
    require(len(b_report) == 129 and {item["tensor"] for item in b_report} == expected_b,
            "retained B comparison inventory differs")
    for item in b_report:
        raw = factors[item["tensor"]]
        result = metrics(vector(raw, 5120), tuple(-value for value in normalized), 1e-6, 1e-6)
        require(item["sha256"] == digest(raw) and all(item[key] == value for key, value in result.items())
                and result["mismatchedElements"] == 0, "retained B comparison replay differs")
    require(report["all129BComparisonsPassed"] is True, "retained B verdict differs")
    sites = report["sites"]
    require(len(sites) == 48 and [item["block"] for item in sites] == list(BLOCKS),
            "retained A comparison inventory differs")
    require({path.name for path in evidence.glob("ssm-*.f32le")} ==
            {f"ssm-{block:02}.f32le" for block in BLOCKS}, "retained A file inventory differs")
    results = []
    for item in sites:
        block = item["block"]
        name = f"ssm-{block:02}.f32le"
        prefix = f"language_model.model.layers.{block}.linear_attn.out_proj"
        require(item["publicTensorPrefix"] == prefix
                and set(item["inputTensorDescriptors"]) == {"weight", "scales", "biases", "signs"}
                and set(item["inputTensorSha256"]) == {"weight", "scales", "biases", "signs"},
                "retained source tensor inventory differs")
        for suffix, shape, dtypes in (("weight", [5120, 384], ("U32",)),
                                      ("scales", [5120, 48], ("F16",)),
                                      ("biases", [5120, 48], ("F16",)),
                                      ("signs", [6144], ("F16", "F32"))):
            descriptor = descriptors[prefix + "." + suffix]
            require(descriptor == item["inputTensorDescriptors"][suffix]
                    and descriptor["shape"] == shape and descriptor["dtype"] in dtypes
                    and re.fullmatch(r"[0-9a-f]{64}", item["inputTensorSha256"][suffix]) is not None,
                    "retained source tensor descriptor differs")
        require(re.fullmatch(r"[0-9a-f]{64}", item["candidateF16MatrixSha256"]) is not None,
                "retained candidate matrix digest is invalid")
        require(item["authoringAFile"] == name and item["authoringABytes"] == 24576,
                "retained A filename/extent differs")
        reference = bounded(evidence / name, 24576, 24576, item["authoringASha256"])
        shipped = factors[f"blk.{block}.ssm_out.weight.lora_a"]
        result = metrics(vector(shipped, 6144), vector(reference, 6144))
        require(item["shippedASha256"] == digest(shipped)
                and all(item[key] == value for key, value in result.items()),
                "retained A comparison replay differs")
        results.append({"block": block, "authoringASha256": digest(reference), **result})
    passed = all(item["mismatchedElements"] == 0 for item in results)
    expected_status = "complete-public-reference-match" if passed else "rejected-public-A-comparison"
    require(report["all48PublicAComparisonPassed"] is passed and report["status"] == expected_status,
            "retained A verdict differs")
    return {"schemaVersion": 1, "kind": "offline-retained-public-reference-replay",
            "status": "retained-result-verified", "sourceRevision": revision,
            "runId": run_id, "runAttempt": run_attempt, "reportSha256": digest(bounded(evidence / "report.json", 1024 * 1024)),
            "referencePinsSha256": digest(pins_raw),
            "replaySourceManifestSha256": report.get("replaySourceManifestSha256"),
            "retainedReplaySourceVerified": sources is not None,
            "reviewScriptSha256": digest(bounded(Path(__file__), 1024 * 1024)),
            "adapterSha256": pins["adapter"]["sha256"], "directionSha256": pins["direction"]["sha256"],
            "all48PublicAComparisonPassed": passed, "all129BComparisonsPassed": True, "sites": results,
            "producerAuthenticated": False, "matrixReconstructionReexecuted": False,
            "authorOriginalProvenanceQualified": False, "nativeWriterBasisQualified": False,
            "defaultMayChange": False, "modelInferenceCommands": 0,
            "scope": "Independent raw-file and scalar A/B comparison replay only. Authenticate the selected producer/artifact separately. This replay does not open the model pack, reproduce its matrix arithmetic or qualify authoring provenance, writer basis, logits/state, quality or promotion."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-directory", required=True, type=Path)
    parser.add_argument("--adapter", required=True, type=Path)
    parser.add_argument("--direction", required=True, type=Path)
    parser.add_argument("--expected-source-revision", required=True)
    parser.add_argument("--expected-run-id", required=True)
    parser.add_argument("--expected-run-attempt", required=True)
    arguments = parser.parse_args()
    try:
        result = audit(arguments.evidence_directory, arguments.adapter, arguments.direction,
                       arguments.expected_source_revision, arguments.expected_run_id, arguments.expected_run_attempt)
        print(json.dumps(result, indent=2, allow_nan=False))
        return 0
    except (OSError, ValueError, KeyError, TypeError, OverflowError, RecursionError) as error:
        print("Retained public reference rejected: " + type(error).__name__, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
