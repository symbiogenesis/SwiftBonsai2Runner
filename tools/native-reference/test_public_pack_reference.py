"""Small independent math, malformed-input and hosted-admission regressions."""

import hashlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

import math
import numpy as np

from public_pack_formats import Rejected, gguf_factors, safetensors_header, strict_json, tensor_array
from public_pack_reference import (
    BLOCKS, affine_f16, compare, config_sites, dequantize_f16, hosted,
    inverse_f32, receive, reconstruct,
)


def safetensors(value, payload=b"\0\0\0\0"):
    raw = json.dumps(value, separators=(",", ":")).encode()
    raw += b" " * (-len(raw) % 8)
    return struct.pack("<Q", len(raw)) + raw + payload


def one_tensor():
    return {"tensor": {"dtype": "F32", "shape": [1], "data_offsets": [0, 4]}}


def fixed_config():
    return {"schema_version": 2, "model_type": "prism_hadamard_qwen35", "base_model_type": "qwen3_5",
            "tensor_namespace": "mlx-vlm-qwen3_5", "gdn_activation_layout": "grouped",
            "quantization": {"bits": 2, "group_size": 128, "mode": "affine"},
            "modules": [{"path": f"model.layers.{block}.linear_attn.out_proj", "block": 1024,
                         "dtype": "float16", "embedding": False} for block in BLOCKS]}


def select_code(weight, row, position, code):
    word, shift = divmod(position, 16)
    shift *= 2
    weight[row, word] = (int(weight[row, word]) & (0xFFFFFFFF ^ (3 << shift))) | (code << shift)


class PublicReferenceTests(unittest.TestCase):
    def test_safetensors_rejects_ambiguous_json_and_pre_read_header_bounds(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}', b'\xff', b'{}{}'):
            with self.assertRaises(Rejected):
                strict_json(raw)
        raw = struct.pack("<Q", 4 * 1024 * 1024 + 1)
        with self.assertRaises(Rejected):
            safetensors_header(io.BytesIO(raw), 9_000_000_000)
        for raw in (b"", struct.pack("<Q", 8) + b"{}"):
            with self.assertRaises(Rejected):
                safetensors_header(io.BytesIO(raw), len(raw))

    def test_safetensors_checks_every_extent_dtype_and_payload_tail(self):
        original = one_tensor()
        raw = safetensors(original, struct.pack("<f", 2.5))
        tensors, _ = safetensors_header(io.BytesIO(raw), len(raw))
        np.testing.assert_array_equal(tensor_array(raw, tensors["tensor"], (1,), ("F32",)), [2.5])
        for field, changed in (("dtype", "UNKNOWN"), ("shape", [-1]), ("shape", [True]),
                               ("shape", [1_000_000_001]), ("data_offsets", [0, 5])):
            value = one_tensor()
            value["tensor"][field] = changed
            broken = safetensors(value)
            with self.assertRaises(Rejected):
                safetensors_header(io.BytesIO(broken), len(broken))
        for value, payload in (
            ({"a": original["tensor"], "b": original["tensor"]}, b"\0" * 4),
            ({"a": {"dtype": "F32", "shape": [1], "data_offsets": [4, 8]}}, b"\0" * 8),
            (original, b"\0" * 5),
            ({"__metadata__": {"not-string": 1}, **original}, b"\0" * 4),
        ):
            broken = safetensors(value, payload)
            with self.assertRaises(Rejected):
                safetensors_header(io.BytesIO(broken), len(broken))
        with self.assertRaises(Rejected):
            tensor_array(raw, tensors["tensor"], (2,), ("F32",))

    def test_adapter_reader_rejects_version_inventory_ambiguous_metadata_and_truncation(self):
        def text(value):
            return struct.pack("<Q", len(value)) + value

        duplicate = text(b"general.type") + struct.pack("<I", 8) + text(b"adapter")
        for raw in (b"", b"GGUF" + struct.pack("<IQQ", 2, 258, 0),
                    b"GGUF" + struct.pack("<IQQ", 3, 257, 0),
                    b"GGUF" + struct.pack("<IQQ", 3, 258, 129),
                    b"GGUF" + struct.pack("<IQQ", 3, 258, 2) + duplicate * 2,
                    b"GGUF" + struct.pack("<IQQ", 3, 258, 1) + text(b"general.type") +
                    struct.pack("<IQ", 8, 1_048_577)):
            with self.assertRaises(Rejected):
                gguf_factors(raw)

    def test_affine_f16_matches_independent_fma_over_complete_accepted_f16_domain(self):
        scales = np.arange(65536, dtype=np.uint16).view(np.float16)
        scales = scales[np.isfinite(scales) & (np.abs(scales.astype(np.float32)) <= 32752)]
        for code in (0, 1, 2):
            expected = np.fromiter((math.fma(float(scale), code, -float(scale))
                                    for scale in scales), dtype=np.float64).astype(np.float16)
            actual = affine_f16(np.uint32(code), scales, np.negative(scales))
            np.testing.assert_array_equal(actual.view(np.uint16), expected.view(np.uint16))

    def test_bit_order_block_normalization_and_inverse_signs_match_closed_form(self):
        weight = np.full((2, 128), 0x55555555, dtype="<u4")
        select_code(weight, 0, 3, 2)
        select_code(weight, 1, 1024 + 5, 2)
        scales = np.ones((2, 16), dtype="<f2")
        signs = np.array([1 if index & 8 == 0 else -1 for index in range(2048)], dtype=np.float32)
        vector, _ = reconstruct(weight, scales, -scales, signs, np.array([0.25, -0.75], dtype=np.float32))
        expected = []
        for index in range(2048):
            position, amplitude = (3, 0.25) if index < 1024 else (5, -0.75)
            value = amplitude * (-1 if ((index % 1024) & position).bit_count() % 2 else 1)
            expected.append(value * float(signs[index]) / 32)
        np.testing.assert_array_equal(vector, np.array(expected, dtype=np.float32))

    def test_candidate_checkpoint_f16_round_is_observable_and_not_omitted(self):
        weight = np.full((1, 64), 0x55555555, dtype="<u4")
        select_code(weight, 0, 0, 2)
        select_code(weight, 0, 128, 2)
        scales = np.ones((1, 8), dtype="<f2")
        scales[0, 1] = 2**-11
        unrounded = (1 + 2**-11) / 32
        expected = struct.unpack("<e", struct.pack("<e", unrounded))[0]
        vector, _ = reconstruct(weight, scales, -scales, np.ones(1024, dtype=np.float32),
                                np.ones(1, dtype=np.float32))
        self.assertNotEqual(float(expected), unrounded)
        self.assertEqual(float(vector[0]), expected)

    def test_reconstruction_rejects_invalid_codes_biases_signs_and_finite_domain(self):
        weight = np.full((1, 64), 0x55555555, dtype="<u4")
        scales = np.ones((1, 8), dtype="<f2")
        for bad_scales, bad_bias in ((scales, scales),
                                     (np.full_like(scales, np.inf), -scales),
                                     (np.full_like(scales, 65504), np.full_like(scales, -65504))):
            with self.assertRaises(Rejected):
                dequantize_f16(weight, bad_scales, bad_bias)
        select_code(weight, 0, 10, 3)
        with self.assertRaises(Rejected):
            dequantize_f16(weight, scales, -scales)
        for signs in (np.zeros(1024, dtype=np.float32), np.ones(1023, dtype=np.float32)):
            with self.assertRaises(Rejected):
                inverse_f32(np.zeros((1, 1024), dtype=np.float32), signs)
        with self.assertRaises(Rejected):
            compare(np.array([np.nan]), np.array([0.0]))

    def test_existing_comparison_bound_has_no_tolerance_search(self):
        at_bound = np.array([1e-5], dtype=np.float64)
        self.assertEqual(compare(at_bound, np.array([0.0]))["mismatchedElements"], 0)
        beyond = np.nextafter(at_bound, np.inf)
        self.assertEqual(compare(beyond, np.array([0.0]))["mismatchedElements"], 1)
        self.assertEqual(compare(np.array([1.0]), np.array([0.0]))["mismatchedElements"], 1)

    def test_config_rejects_permutation_layout_missing_or_duplicate_sites(self):
        self.assertEqual(len(config_sites(json.dumps(fixed_config()).encode())), 48)
        for field, value in (("gdn_activation_layout", "ungrouped"), ("schema_version", 1)):
            changed = fixed_config()
            changed[field] = value
            with self.assertRaises(Rejected):
                config_sites(json.dumps(changed).encode())
        for mode in ("missing", "duplicate", "transform"):
            changed = fixed_config()
            if mode == "missing":
                changed["modules"].pop()
            elif mode == "duplicate":
                changed["modules"].append(changed["modules"][0])
            else:
                changed["modules"][0]["block"] = 512
            with self.assertRaises(Rejected):
                config_sites(json.dumps(changed).encode())

    def test_hosted_admission_rejects_owner_nonmain_and_wrong_thread_policy(self):
        environment = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                       "RUNNER_OS": "Windows", "RUNNER_ARCH": "ARM64",
                       "GITHUB_REPOSITORY": "symbiogenesis/SwiftBonsai2Runner",
                       "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                       "GITHUB_SHA": "a" * 40,
                       **{key: "1" for key in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS",
                                                "MKL_NUM_THREADS", "BLIS_NUM_THREADS")}}
        hosted(environment)
        for key, value in (("RUNNER_ENVIRONMENT", "self-hosted"), ("GITHUB_REF", "refs/heads/work"),
                           ("RUNNER_ARCH", "X64"), ("GITHUB_SHA", "main"), ("OPENBLAS_NUM_THREADS", "4")):
            with self.assertRaises(Rejected):
                hosted({**environment, key: value})

    def test_transfer_checks_complete_hash_length_before_publishing_input(self):
        class Reply(io.BytesIO):
            status = 200
            url = "https://example.invalid/reference"
            headers = {}

        raw = b"fixed synthetic input"
        for received in (raw, raw[:-1], raw + b"extra", b"x" * len(raw)):
            with tempfile.TemporaryDirectory() as temporary:
                pin = {"filename": "input.bin", "bytes": len(raw),
                       "sha256": hashlib.sha256(raw).hexdigest(), "url": Reply.url}
                with mock.patch("urllib.request.urlopen", return_value=Reply(received)):
                    if received == raw:
                        self.assertEqual(receive(Path(temporary), pin).read_bytes(), raw)
                    else:
                        with self.assertRaises(Rejected):
                            receive(Path(temporary), pin)
                        self.assertFalse((Path(temporary) / "input.bin").exists())


if __name__ == "__main__":
    unittest.main()
