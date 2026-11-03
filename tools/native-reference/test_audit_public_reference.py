"""Small model-free controls for the independent retained-public-result replay."""

import copy
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import audit_public_reference as audit


def encoded(value):
    return (json.dumps(value, separators=(",", ":")) + "\n").encode()


def text(value):
    raw = value.encode()
    return struct.pack("<Q", len(raw)) + raw


def synthetic_adapter():
    sites = {"token_embd.weight": (1, 248320)}
    for block in range(64):
        sites[f"blk.{block}.ffn_down.weight"] = (17408, 1)
        family = "attn_output" if (block + 1) % 4 == 0 else "ssm_out"
        sites[f"blk.{block}.{family}.weight"] = (6144, 1)
    descriptors, payload = [], bytearray()
    for site, shape in sites.items():
        for suffix in (".lora_a", ".lora_b"):
            dimensions = shape if suffix == ".lora_a" else (1, 5120)
            payload.extend(b"\0" * (-len(payload) % 32))
            descriptors.append(text(site + suffix) + struct.pack("<IQQIQ", 2, *dimensions, 0, len(payload)))
            if suffix == ".lora_b":
                payload.extend(struct.pack("<f", -1.0) + b"\0" * (5119 * 4))
            else:
                payload.extend(b"\0" * (dimensions[0] * dimensions[1] * 4))
    metadata = b"".join(text(name) + struct.pack("<I", 8) + text(value) for name, value in (
        ("general.architecture", "qwen35"), ("general.type", "adapter"), ("adapter.type", "lora")))
    metadata += text("adapter.lora_alpha") + struct.pack("<If", 6, 1.0)
    header = b"GGUF" + struct.pack("<IQQ", 3, 258, 4) + metadata + b"".join(descriptors)
    return header + b"\0" * (-len(header) % 32) + payload


class PublicReplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.directory = Path(cls.temporary.name)
        cls.source = cls.directory / "checkout" / "tools" / "native-reference"
        cls.source.mkdir(parents=True)
        cls.evidence = cls.directory / "evidence"
        cls.evidence.mkdir()
        cls.adapter = cls.directory / "adapter.gguf"
        adapter_raw = synthetic_adapter()
        cls.adapter.write_bytes(adapter_raw)
        cls.direction = cls.directory / "direction.bin"
        direction_raw = struct.pack("<f", 1.0) + b"\0" * (5119 * 4)
        cls.direction.write_bytes(direction_raw)
        header, offset, sites = {}, 0, []
        for block in audit.BLOCKS:
            prefix = f"language_model.model.layers.{block}.linear_attn.out_proj"
            for suffix, shape, dtype, width in (("weight", [5120, 384], "U32", 4),
                                               ("scales", [5120, 48], "F16", 2),
                                               ("biases", [5120, 48], "F16", 2),
                                               ("signs", [6144], "F32", 4)):
                end = offset + shape[0] * (shape[1] if len(shape) == 2 else 1) * width
                header[prefix + "." + suffix] = {"dtype": dtype, "shape": shape,
                                                  "data_offsets": [offset, end]}
                offset = end
            sites.append({"block": block, "publicTensorPrefix": prefix,
                          "candidateF16MatrixSha256": "a" * 64,
                          "authoringAFile": f"ssm-{block:02}.f32le", "authoringABytes": 24576,
                          "authoringASha256": audit.digest(b"\0" * 24576),
                          "shippedASha256": audit.digest(b"\0" * 24576),
                          "inputTensorDescriptors": {},
                          "inputTensorSha256": {suffix: "b" * 64 for suffix in ("weight", "scales", "biases", "signs")},
                          "comparedElements": 6144, "mismatchedElements": 0, "maximumAbsoluteDifference": 0.0})
            (cls.evidence / f"ssm-{block:02}.f32le").write_bytes(b"\0" * 24576)
        header_raw = encoded(header)
        base = len(header_raw) + 8
        for site in sites:
            for suffix in ("weight", "scales", "biases", "signs"):
                descriptor = header[site["publicTensorPrefix"] + "." + suffix]
                begin, end = descriptor["data_offsets"]
                site["inputTensorDescriptors"][suffix] = {
                    "dtype": descriptor["dtype"], "shape": descriptor["shape"],
                    "absoluteByteBegin": base + begin, "absoluteByteEnd": base + end}
        cls.pins = {"adapter": {"filename": "adapter.gguf", "url": "https://example.invalid/adapter",
                                "bytes": len(adapter_raw), "sha256": audit.digest(adapter_raw)},
                    "direction": {"filename": "direction.bin", "url": "https://example.invalid/direction",
                                  "bytes": len(direction_raw), "sha256": audit.digest(direction_raw)},
                    "config": {"bytes": 3, "sha256": audit.digest(b"{}\n")},
                    "model": {"bytes": base + offset}, "numpy": "synthetic", "python": "3.14.0"}
        pins_raw = encoded(cls.pins)
        (cls.source / "public-authoring-pins.json").write_bytes(pins_raw)
        configuration = cls.source.parents[1] / "config"
        configuration.mkdir()
        for filename, field in (("swift-lora.lock.json", "adapter"), ("swift-lora.direction.json", "direction")):
            (configuration / filename).write_bytes(encoded({field: cls.pins[field]}))
        (cls.evidence / "config.json").write_bytes(b"{}\n")
        (cls.evidence / "model-header.json").write_bytes(header_raw)
        for filename in ("public_pack_reference.py", "public_pack_formats.py"):
            (cls.source / filename).write_bytes(b"synthetic source")
        (cls.source / "audit_public_reference.py").write_bytes(b"synthetic replay source")
        (cls.evidence / "toolchain.json").write_bytes(encoded({
            "referenceScriptSha256": audit.digest(b"synthetic source"),
            "formatScriptSha256": audit.digest(b"synthetic source"), "numpy": "synthetic", "blasThreads": 1,
            "python": "3.14.0 synthetic fixture"}))
        factors = audit.Adapter(adapter_raw).factors()
        (cls.evidence / "direction-comparison.json").write_bytes(encoded({"sites": [
            {"tensor": name, "sha256": audit.digest(raw), "comparedElements": 5120,
             "mismatchedElements": 0, "maximumAbsoluteDifference": 0.0}
            for name, raw in factors.items() if name.endswith(".lora_b")]}))
        cls.report = {"schemaVersion": 1, "kind": "independent-public-pack-A-reference",
                      "arithmeticId": audit.ARITHMETIC, "stage": "complete", "inputPins": cls.pins,
                      "referencePinsSha256": audit.digest(pins_raw), "repository": audit.REPOSITORY,
                      "workflowPath": audit.WORKFLOW, "sourceRevision": "a" * 40, "runId": "123",
                      "runAttempt": "1", "runnerEnvironment": "github-hosted", "ownerDeviceTrials": 0,
                      "modelInferenceCommands": 0, "directionF32NormBeforeNormalization": 1.0,
                      "normalizedDirectionSha256": audit.digest(direction_raw),
                      "all129BComparisonsPassed": True, "all48PublicAComparisonPassed": True,
                      "status": "complete-public-reference-match", "sites": sites,
                      "modelHeaderFile": "model-header.json", "modelHeaderBytes": len(header_raw),
                      "modelHeaderSha256": audit.digest(header_raw), "modelTensorCount": len(header),
                      **{flag: False for flag in ("authorOriginalProvenanceQualified", "nativeWriterBasisQualified",
                                                 "logitsOrStateQualified", "defaultMayChange", "fullCheckpointWritten")}}

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def replay(self, mutate=None, run_id="123"):
        report = copy.deepcopy(self.report)
        if mutate:
            mutate(report)
        for name in ("report.json", "attempt.json"):
            (self.evidence / name).write_bytes(encoded(report))
        with mock.patch.object(audit, "__file__", str(self.source / "audit_public_reference.py")):
            return audit.audit(self.evidence, self.adapter, self.direction, "a" * 40, run_id, "1")

    def test_complete_scalar_replay_retains_qualification_boundaries(self):
        result = self.replay()
        self.assertEqual(len(result["sites"]), 48)
        self.assertTrue(result["all48PublicAComparisonPassed"])
        for flag in ("producerAuthenticated", "matrixReconstructionReexecuted",
                     "authorOriginalProvenanceQualified", "nativeWriterBasisQualified", "defaultMayChange"):
            self.assertIs(result[flag], False)

    def test_valid_losing_result_is_verified_without_relabelling_as_a_match(self):
        path = self.evidence / "ssm-00.f32le"
        losing = struct.pack("<f", 1.0) + b"\0" * (24576 - 4)
        path.write_bytes(losing)
        try:
            def lose(report):
                report.update(status="rejected-public-A-comparison", all48PublicAComparisonPassed=False)
                report["sites"][0].update(authoringASha256=audit.digest(losing),
                                          mismatchedElements=1, maximumAbsoluteDifference=1.0)
            result = self.replay(lose)
            self.assertEqual(result["status"], "retained-result-verified")
            self.assertFalse(result["all48PublicAComparisonPassed"])
            self.assertEqual(result["sites"][0]["mismatchedElements"], 1)
            with self.assertRaises(audit.Rejected):
                self.replay()
        finally:
            path.write_bytes(b"\0" * 24576)

    def test_fabricated_metrics_missing_duplicate_reordered_and_path_sites_reject(self):
        for mutate in (
            lambda report: report["sites"][0].update(mismatchedElements=1),
            lambda report: report["sites"].pop(),
            lambda report: report["sites"].__setitem__(1, copy.deepcopy(report["sites"][0])),
            lambda report: report["sites"].reverse(),
            lambda report: report["sites"][0].update(authoringAFile="../ssm-00.f32le"),
            lambda report: report["sites"][0]["inputTensorDescriptors"]["weight"].update(absoluteByteBegin=0),
            lambda report: report.update(defaultMayChange=True),
        ):
            with self.subTest(mutation=mutate), self.assertRaises(audit.Rejected):
                self.replay(mutate)

    def test_independent_producer_selection_and_source_bytes_reject_substitution(self):
        with self.assertRaises(audit.Rejected):
            self.replay(run_id="124")
        source = self.source / "public_pack_formats.py"
        source.write_bytes(b"different source")
        try:
            with self.assertRaises(audit.Rejected):
                self.replay()
        finally:
            source.write_bytes(b"synthetic source")

    def test_missing_or_extra_vector_files_reject(self):
        extra = self.evidence / "ssm-03.f32le"
        extra.write_bytes(b"\0" * 24576)
        try:
            with self.assertRaises(audit.Rejected):
                self.replay()
        finally:
            extra.unlink()

    def test_duplicate_nonfinite_json_truncation_and_misaligned_adapter_reject(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}'):
            with self.assertRaises(audit.Rejected):
                audit.strict_json(raw)
        for raw in (b"", b"GGUF" + struct.pack("<IQQ", 3, 258, 0)):
            with self.assertRaises(audit.Rejected):
                audit.Adapter(raw).factors()


if __name__ == "__main__":
    unittest.main()
