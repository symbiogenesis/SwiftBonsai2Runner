"""One hosted-only independent all-48 public-pack A-vector reconstruction.

No model is instantiated, no downloaded source is imported, and no checkpoint is
written. Numerical disagreement is retained under the single predeclared recipe.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import json
import math
import mmap
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import sys
import time
import urllib.parse
import urllib.request
import uuid

STARTED = time.monotonic()

import numpy as np

from public_pack_formats import (
    Rejected, gguf_factors, require, safetensors_header, strict_json, tensor_array,
)

ARITHMETIC = "public-mlx-f16-inverse1024-f16-f32-dot-v1"
BLOCKS = tuple(block for block in range(64) if (block + 1) % 4 != 0)
ABSOLUTE = RELATIVE = 1e-5
WORK_SECONDS = 2400


def hosted(environment: dict[str, str]) -> None:
    expected = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                "RUNNER_OS": "Windows", "RUNNER_ARCH": "ARM64",
                "GITHUB_REPOSITORY": "symbiogenesis/SwiftBonsai2Runner",
                "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch"}
    require(all(environment.get(key) == value for key, value in expected.items()),
            "public reconstruction requires main-only GitHub-hosted Windows ARM64")
    require(re.fullmatch(r"[0-9a-f]{40}", environment.get("GITHUB_SHA", "")) is not None,
            "source revision is absent")
    require(all(environment.get(key) == "1" for key in
                ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "MKL_NUM_THREADS", "BLIS_NUM_THREADS")),
            "reference reduction thread policy differs")


def remaining() -> float:
    value = STARTED + WORK_SECONDS - time.monotonic()
    require(value > 0, "hosted reference absolute allowance expired")
    return value


def write_json(path: Path, value: object) -> None:
    raw = (json.dumps(value, indent=2, ensure_ascii=True, allow_nan=False) + "\n").encode()
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".partial")
    with temporary.open("xb") as handle:
        handle.write(raw)
    os.replace(temporary, path)


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def receive(directory: Path, pin: dict) -> Path:
    """One bounded transfer, complete raw hash before any mmap/header use."""
    remaining()
    require(set(pin) == {"filename", "bytes", "sha256", "url"}, "input pin schema differs")
    require(type(pin["bytes"]) is int and 0 < pin["bytes"] <= 9_000_000_000 and
            re.fullmatch(r"[A-Za-z0-9_.-]+", pin["filename"]) is not None and
            pin["filename"] not in (".", "..") and Path(pin["filename"]).name == pin["filename"] and
            re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]) is not None and
            urllib.parse.urlsplit(pin["url"]).scheme == "https", "input identity differs")
    target = directory / pin["filename"]
    partial = target.with_name(target.name + ".partial")
    count, digest = 0, hashlib.sha256()
    request = urllib.request.Request(pin["url"], headers={"Accept-Encoding": "identity"})
    with urllib.request.urlopen(request, timeout=min(30, remaining())) as response, partial.open("xb") as output:
        require(response.status == 200 and urllib.parse.urlsplit(response.url).scheme == "https"
                and response.headers.get("Content-Encoding") is None, "unsupported transfer response")
        declared = response.headers.get("Content-Length")
        require(declared is None or (declared.isascii() and declared.isdigit() and
                int(declared) == pin["bytes"]), "declared input length differs")
        while True:
            remaining()
            chunk = response.read(min(1024 * 1024, pin["bytes"] + 1 - count))
            if not chunk:
                break
            count += len(chunk)
            require(count <= pin["bytes"], "input exceeds pinned byte length")
            digest.update(chunk)
            output.write(chunk)
        require(count == pin["bytes"] and digest.hexdigest() == pin["sha256"],
                "complete input raw-byte identity differs")
        output.flush()
        os.fsync(output.fileno())
    os.replace(partial, target)
    return target


@contextlib.contextmanager
def verified_mapping(path: Path, pin: dict):
    with path.open("rb") as handle:
        before = os.fstat(handle.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_size == pin["bytes"],
                "input file type/length differs")
        digest = hashlib.sha256()
        while chunk := handle.read(1024 * 1024):
            remaining()
            digest.update(chunk)
        require(digest.hexdigest() == pin["sha256"], "pre-mmap full input hash differs")
        handle.seek(0)
        descriptors, header = safetensors_header(handle, before.st_size)
        with mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as mapping:
            yield mapping, descriptors, header
        after = os.fstat(handle.fileno())
        require((before.st_size, before.st_mtime_ns, before.st_ino) ==
                (after.st_size, after.st_mtime_ns, after.st_ino), "input endpoint identity changed")


def dequantize_f16(weight: np.ndarray, scales: np.ndarray, biases: np.ndarray) -> np.ndarray:
    rows, words = weight.shape
    width = words * 16
    require(weight.dtype == np.dtype("<u4") and width % 1024 == 0 and
            scales.shape == biases.shape == (rows, width // 128) and
            scales.dtype == biases.dtype == np.dtype("<f2"), "affine tensor geometry/dtype differs")
    require(np.isfinite(scales).all() and np.isfinite(biases).all() and
            np.max(np.abs(scales.astype(np.float32))) <= 32752.0, "scale is outside exact finite F16 domain")
    require(np.array_equal(biases.view(np.uint16), np.negative(scales).view(np.uint16)),
            "ternary affine bias is not the exact raw negated scale")
    shifts = np.arange(0, 32, 2, dtype=np.uint32)
    codes = ((weight[..., None] >> shifts) & np.uint32(3)).reshape(rows, width // 128, 128)
    require(np.max(codes) <= 2, "unused ternary code3 is present")
    # q=0,1,2 and bias=-s: products/additions are exact in the declared finite
    # F16 domain, so contraction cannot choose a different ternary value.
    values = affine_f16(codes, scales[..., None], biases[..., None])
    require(np.isfinite(values).all(), "nonfinite affine output")
    return values.reshape(rows, width).astype(np.float32)


def affine_f16(codes, scales, biases):
    values = np.multiply(codes, scales, dtype=np.float16)
    np.add(values, biases, out=values, dtype=np.float16)
    return values


def inverse_f32(values: np.ndarray, signs: np.ndarray) -> np.ndarray:
    require(values.dtype == np.float32 and values.ndim == 2 and values.shape[1] % 1024 == 0,
            "inverse transform geometry differs")
    require(signs.shape == (values.shape[1],) and np.isfinite(signs).all() and
            np.all((signs == 1) | (signs == -1)), "invalid per-site sign vector")
    require(np.isfinite(values).all(), "nonfinite transform input")
    result = values.copy().reshape(values.shape[0], -1, 1024)
    for span in (1, 2, 4, 8, 16, 32, 64, 128, 256, 512):
        groups = result.reshape(values.shape[0], -1, 1024 // (2 * span), 2 * span)
        left, right = groups[..., :span].copy(), groups[..., span:].copy()
        np.add(left, right, out=groups[..., :span], dtype=np.float32)
        np.subtract(left, right, out=groups[..., span:], dtype=np.float32)
    result *= np.float32(1.0 / 32.0)
    result = result.reshape(values.shape)
    result *= signs.astype(np.float32)
    require(np.isfinite(result).all(), "nonfinite inverse output")
    return result


def reconstruct(weight, scales, biases, signs, direction) -> tuple[np.ndarray, str]:
    unfolded = inverse_f32(dequantize_f16(weight, scales, biases), signs)
    # This storage round is a declared independent candidate, not a claim about
    # the missing author's original serialization implementation.
    stored = unfolded.astype("<f2")
    require(np.isfinite(stored).all(), "candidate F16 storage overflow")
    stored_digest = sha256(stored.tobytes(order="C"))
    matrix = stored.astype(np.float32)
    require(direction.dtype == np.float32 and direction.shape == (matrix.shape[0],),
            "direction/output dimension differs")
    vector = np.matmul(direction, matrix).astype("<f4")
    require(np.isfinite(vector).all(), "nonfinite reconstructed A")
    return vector, stored_digest


def compare(actual, expected, absolute=ABSOLUTE, relative=RELATIVE) -> dict:
    require(actual.shape == expected.shape and np.isfinite(actual).all() and
            np.isfinite(expected).all(), "comparison extent/finite values differ")
    difference = np.abs(actual.astype(np.float64) - expected.astype(np.float64))
    limits = np.fromiter((math.fma(relative, abs(float(value)), absolute)
                          for value in expected), dtype=np.float64, count=expected.size)
    return {"comparedElements": int(expected.size),
            "mismatchedElements": int(np.count_nonzero(difference > limits)),
            "maximumAbsoluteDifference": float(np.max(difference, initial=0))}


def config_sites(raw: bytes) -> list[str]:
    config = strict_json(raw)
    require(type(config) is dict and config.get("schema_version") == 2 and
            config.get("model_type") == "prism_hadamard_qwen35" and
            config.get("base_model_type") == "qwen3_5" and
            config.get("tensor_namespace") == "mlx-vlm-qwen3_5" and
            config.get("gdn_activation_layout") == "grouped" and
            config.get("quantization") == {"bits": 2, "group_size": 128, "mode": "affine"},
            "public model layout/schema differs")
    modules = config.get("modules")
    require(type(modules) is list and len(modules) <= 1024 and
            all(type(record) is dict and type(record.get("path")) is str for record in modules),
            "packed module inventory differs")
    paths = [record["path"] for record in modules]
    require(len(paths) == len(set(paths)), "duplicate packed module path")
    selected = [record for record in modules if record["path"].endswith(".linear_attn.out_proj")]
    expected = {f"model.layers.{block}.linear_attn.out_proj" for block in BLOCKS}
    require({record["path"] for record in selected} == expected and len(selected) == 48 and all(
        record.get("block") == 1024 and record.get("dtype") == "float16" and
        record.get("embedding") is False for record in selected), "exact SSM module set differs")
    return [f"language_model.model.layers.{block}.linear_attn.out_proj" for block in BLOCKS]


def audit(data: Path, evidence: Path) -> bool:
    hosted(dict(os.environ))
    require(sys.version_info[:3] == (3, 14, 0) and np.__version__ == "2.5.3" and
            sys.byteorder == "little" and sys.maxsize > 2**32, "reference toolchain differs")
    temporary = Path(os.environ["RUNNER_TEMP"]).resolve()
    require(data.is_absolute() and evidence.is_absolute() and data != evidence and
            data.resolve().is_relative_to(temporary) and evidence.resolve().is_relative_to(temporary),
            "reference data/output must be separate external runner-temp directories")
    data.mkdir(parents=True, exist_ok=False)
    evidence.mkdir(parents=True, exist_ok=True)
    require(not (evidence / "attempt.json").exists(), "retain earlier reference attempts")
    pins_path = Path(__file__).with_name("public-authoring-pins.json")
    pins_raw = pins_path.read_bytes()
    pins = strict_json(pins_raw)
    require(pins["schemaVersion"] == 1 and pins["arithmeticId"] == ARITHMETIC and
            pins["python"] == "3.14.0" and pins["numpy"] == "2.5.3" and
            pins["absoluteTolerance"] == ABSOLUTE and pins["relativeTolerance"] == RELATIVE,
            "reference recipe or fixed bounds differ")
    repository = Path(__file__).resolve().parents[2]
    for filename, name, field in (("swift-lora.lock.json", "adapter", "adapter"),
                                  ("swift-lora.direction.json", "direction", "direction")):
        selected = strict_json((repository / "config" / filename).read_bytes())[field]
        require(all(pins[name][key] == selected[key] for key in ("filename", "bytes", "sha256", "url")),
                "approved adapter/direction repository lock differs")
    report = {"schemaVersion": 1, "kind": "independent-public-pack-A-reference",
              "status": "running", "stage": "inputs", "arithmeticId": ARITHMETIC,
              "referencePinsSha256": sha256(pins_raw), "inputPins": pins,
              "repository": os.environ["GITHUB_REPOSITORY"],
              "workflowPath": ".github/workflows/ptq1-public-authoring-reference.yml",
              "sourceRevision": os.environ["GITHUB_SHA"], "runId": os.environ["GITHUB_RUN_ID"],
              "runAttempt": os.environ["GITHUB_RUN_ATTEMPT"], "sites": [],
              "runnerEnvironment": os.environ["RUNNER_ENVIRONMENT"],
              "authorOriginalProvenanceQualified": False, "nativeWriterBasisQualified": False,
              "logitsOrStateQualified": False, "defaultMayChange": False,
              "ownerDeviceTrials": 0, "modelInferenceCommands": 0,
              "fullCheckpointWritten": False, "all48PublicAComparisonPassed": False}
    attempt = evidence / "attempt.json"
    try:
        write_json(attempt, report)
        configuration = io.StringIO()
        with contextlib.redirect_stdout(configuration):
            np.show_config()
        write_json(evidence / "toolchain.json", {"python": sys.version, "numpy": np.__version__,
                   "platform": platform.platform(), "numpyConfiguration": configuration.getvalue(),
                   "blasThreads": 1, "referenceScriptSha256": sha256(Path(__file__).read_bytes()),
                   "formatScriptSha256": sha256(Path(__file__).with_name("public_pack_formats.py").read_bytes())})
        paths = {}
        for name in ("config", "adapter", "direction"):
            report["stage"] = name + "-intake"
            write_json(attempt, report)
            paths[name] = receive(data, pins[name])
        config_raw = paths["config"].read_bytes()
        sites = config_sites(config_raw)
        (evidence / "config.json").write_bytes(config_raw)
        direction = np.frombuffer(paths["direction"].read_bytes(), dtype="<f4").copy()
        require(direction.shape == (5120,) and np.isfinite(direction).all(), "direction geometry differs")
        norm = np.linalg.norm(direction)
        require(norm.dtype == np.dtype("float32") and np.isfinite(norm) and abs(float(norm) - 1) <= 1e-5,
                "selected direction is not unit-normalized F32")
        direction = np.divide(direction, norm, dtype=np.float32)
        factors = gguf_factors(paths["adapter"].read_bytes())
        b_results = [{"tensor": name, "sha256": sha256(vector.astype("<f4").tobytes()),
                      **compare(vector, -direction, 1e-6, 1e-6)} for name, vector in factors.items()
                     if name.endswith(".lora_b")]
        write_json(evidence / "direction-comparison.json", {"sites": b_results})
        require(len(b_results) == 129 and all(item["mismatchedElements"] == 0 for item in b_results),
                "selected normalized direction differs from approved B factors")
        report["directionF32NormBeforeNormalization"] = float(norm)
        report["normalizedDirectionSha256"] = sha256(direction.astype("<f4").tobytes())
        report["all129BComparisonsPassed"] = True
        report["stage"] = "model-intake"
        write_json(attempt, report)
        require(shutil.disk_usage(data).free >= pins["model"]["bytes"] + 1024**3,
                "hosted input storage cannot retain the model plus one GiB reserve")
        paths["model"] = receive(data, pins["model"])
        report["stage"] = "model-full-rehash-and-header"
        write_json(attempt, report)
        with verified_mapping(paths["model"], pins["model"]) as (mapping, tensors, header):
            (evidence / "model-header.json").write_bytes(header)
            report["modelHeaderSha256"] = sha256(header)
            report["modelHeaderBytes"] = len(header)
            report["modelHeaderFile"] = "model-header.json"
            report["modelTensorCount"] = len(tensors)
            for block, site in zip(BLOCKS, sites, strict=True):
                remaining()
                report["stage"] = f"reconstruct-ssm-{block}"
                write_json(attempt, report)
                wanted = {suffix: tensors.get(site + "." + suffix)
                          for suffix in ("weight", "scales", "biases", "signs")}
                require(all(item is not None for item in wanted.values()), "missing SSM tensor")
                weight = tensor_array(mapping, wanted["weight"], (5120, 384), ("U32",))
                scales = tensor_array(mapping, wanted["scales"], (5120, 48), ("F16",))
                biases = tensor_array(mapping, wanted["biases"], (5120, 48), ("F16",))
                signs = tensor_array(mapping, wanted["signs"], (6144,), ("F16", "F32"))
                vector, stored_digest = reconstruct(weight, scales, biases, signs, direction)
                result = compare(factors[f"blk.{block}.ssm_out.weight.lora_a"], vector)
                output_name = f"ssm-{block:02}.f32le"
                output = vector.astype("<f4").tobytes()
                (evidence / output_name).write_bytes(output)
                report["sites"].append({"block": block, "publicTensorPrefix": site,
                    "candidateF16MatrixSha256": stored_digest, "authoringAFile": output_name,
                    "authoringASha256": sha256(output), "authoringABytes": len(output),
                    "shippedASha256": sha256(factors[f"blk.{block}.ssm_out.weight.lora_a"].astype("<f4").tobytes()),
                    "inputTensorDescriptors": {suffix: {
                        "dtype": descriptor.dtype, "shape": list(descriptor.shape),
                        "absoluteByteBegin": descriptor.begin, "absoluteByteEnd": descriptor.end,
                    } for suffix, descriptor in wanted.items()},
                    "inputTensorSha256": {suffix: sha256(array.tobytes(order="C")) for suffix, array in
                        (("weight", weight), ("scales", scales), ("biases", biases), ("signs", signs))}, **result})
                write_json(attempt, report)
        remaining()
        passed = len(report["sites"]) == 48 and all(site["mismatchedElements"] == 0 for site in report["sites"])
        report["all48PublicAComparisonPassed"] = passed
        report["status"] = "complete-public-reference-match" if passed else "rejected-public-A-comparison"
        report["stage"] = "complete"
        write_json(evidence / "report.json", report)
        return passed
    except Exception as error:
        report["status"] = "failed"
        report["failureClass"] = type(error).__name__
        raise
    finally:
        write_json(attempt, report)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-directory", required=True, type=Path)
    parser.add_argument("--evidence-directory", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        return 0 if audit(arguments.data_directory, arguments.evidence_directory) else 1
    except Exception as error:
        print("Public reference rejected: " + type(error).__name__, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
