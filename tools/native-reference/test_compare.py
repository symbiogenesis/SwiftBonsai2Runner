"""Named synthetic guards for exact native-state decoding and comparison."""
import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("native_compare", Path(__file__).with_name("compare.py"))
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def fixture(transposed):
    layout = MODULE.Layout(attention=(3,), recurrent=(0,), layers=4, width=2, convolution=3, matrix=4)
    data = bytearray()
    def add(code, *values):
        data.extend(struct.pack("<" + code, *values))
    add("Ii", 0xAF143CD8, 0)
    add("II", 1, 2)
    for token in (0, 1):
        add("iIiii", token, 1, token, token, 0)
    add("II", transposed, 1)
    add("iQ", 1, 4)
    add("4e", 1.0, 2.0, 3.0, 4.0)
    if transposed:
        add("iII", 1, 2, 2)
        add("4e", 5.0, 7.0, 6.0, 8.0)
    else:
        add("iQ", 1, 4)
        add("4e", 5.0, 6.0, 7.0, 8.0)
    add("IiIII", 1, 1, 0, 0, 4)
    for values in ((9.0, 10.0, 11.0), (12.0, 13.0, 14.0, 15.0)):
        add("iQ", 0, len(values) * 4)
        add(str(len(values)) + "f", *values)
    return bytes(data), layout


class NativeStateTests(unittest.TestCase):
    def test_selected_adapter_condition_binds_loading_scale_and_rust_identity(self):
        records = [{"event": "weights-loaded", "model_sha256": MODULE.MODEL,
                    "weight_representation": "current-repacked-PTQ1_0"},
                   {"event": "forward-step", "position": 0, "input_token": 248045,
                    "backend_requested": "scalar", "row_workers": 1, "parallel_row_operations": 0}]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            for condition in MODULE.ADAPTER_CONDITIONS:
                capture = {"adapterLoaded": condition != "no-adapter",
                           "adapterScale": 1.3 if condition == "standard" else 0}
                adapter = [{"event": "rank-one-adapter-loaded", "model_sha256": MODULE.MODEL,
                            "adapter_sha256": MODULE.ADAPTER, "target_pairs": 129,
                            "scale_bits": "3fa66666" if condition == "standard" else "00000000"}]
                selected = records + (adapter if capture["adapterLoaded"] else [])
                (path / "stdout.txt").write_text("\n".join(map(json.dumps, selected)), encoding="utf-8")
                MODULE.validate_adapter_condition(capture, condition)
                self.assertEqual(len(MODULE.validate_rust_controls(path, capture, "scalar")), 1)
                for other in (*MODULE.ADAPTER_CONDITIONS, "unknown"):
                    if other != condition:
                        with self.assertRaises(ValueError):
                            MODULE.validate_adapter_condition(capture, other)
                if capture["adapterLoaded"]:
                    for field, value in (("adapter_sha256", "0" * 64), ("scale_bits", "80000000"),
                                         ("model_sha256", "0" * 64), ("target_pairs", 128)):
                        changed = [*records, {**adapter[0], field: value}]
                        (path / "stdout.txt").write_text("\n".join(map(json.dumps, changed)), encoding="utf-8")
                        with self.assertRaises(ValueError):
                            MODULE.validate_rust_controls(path, capture, "scalar")
            for value in (True, None, "0", -0.0, float("nan"), float("inf")):
                with self.assertRaises(ValueError):
                    MODULE.validate_adapter_condition({"adapterLoaded": True, "adapterScale": value}, "loaded-zero")

    def test_exact_chunks_reject_nonfinite_values_and_preserve_signed_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "actual"
            for code in ("e", "f"):
                # Span the chunk boundary and a partial tail, including distinct
                # signed zeros, subnormals and the largest finite binary16.
                values = [0.0, -0.0, 2 ** -24, -65504.0] * 1024 + [1.0]
                reference = struct.pack(f"<{len(values)}{code}", *values)
                path.write_bytes(struct.pack(f"<{len(values)}f", *values))
                result = MODULE.compare_payload(reference, code, path)
                self.assertTrue(result["byteExact"])
                self.assertEqual(result["elements"], len(values))
                self.assertEqual(result["failedElements"], 0)
                self.assertEqual(result["maximumAbsoluteError"], 0.0)
                # A sign-only zero change is within tolerance, but not exact.
                observed = bytearray(path.read_bytes())
                observed[7] ^= 0x80
                path.write_bytes(observed)
                result = MODULE.compare_payload(reference, code, path)
                self.assertFalse(result["byteExact"])
                self.assertEqual(result["failedElements"], 0)
                for special in (float("nan"), float("inf"), -float("inf")):
                    for index in (0, 4095, 4096):
                        values[index] = special
                        reference = struct.pack(f"<{len(values)}{code}", *values)
                        path.write_bytes(struct.pack(f"<{len(values)}f", *values))
                        with self.assertRaises(ValueError):
                            MODULE.compare_payload(reference, code, path)
                        values[index] = 1.0

    def test_explicit_backend_controls_reject_substitution_and_unknown_backends(self):
        records = [{"event": "weights-loaded", "model_sha256": MODULE.MODEL,
                    "weight_representation": "current-repacked-PTQ1_0"},
                   {"event": "rank-one-adapter-loaded", "model_sha256": MODULE.MODEL,
                    "adapter_sha256": MODULE.ADAPTER, "target_pairs": 129, "scale_bits": "3fa66666"},
                   {"event": "forward-step", "position": 0, "input_token": 248045,
                    "backend_requested": "auto", "row_workers": 1, "parallel_row_operations": 0}]
        capture = {"adapterLoaded": True, "adapterScale": 1.3}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            for observed in MODULE.BACKENDS:
                records[-1]["backend_requested"] = observed
                (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
                self.assertEqual(len(MODULE.validate_rust_controls(path, capture, observed)), 1)
                for requested in (*MODULE.BACKENDS, "unknown"):
                    if requested != observed:
                        with self.assertRaises(ValueError):
                            MODULE.validate_rust_controls(path, capture, requested)
            records[-1]["backend_requested"] = "unknown"
            (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            with self.assertRaises(ValueError):
                MODULE.validate_rust_controls(path, capture, "unknown")

    def test_control_binding_rejects_changed_adapter_scale_tokens_and_backend(self):
        records = [{"event": "weights-loaded", "model_sha256": MODULE.MODEL,
                    "weight_representation": "current-repacked-PTQ1_0"},
                   {"event": "rank-one-adapter-loaded", "model_sha256": MODULE.MODEL,
                    "adapter_sha256": MODULE.ADAPTER, "target_pairs": 129, "scale_bits": "3fa66666"}]
        records += [{"event": "forward-step", "position": i, "input_token": token,
                     "backend_requested": "auto", "row_workers": 1, "parallel_row_operations": 0}
                    for i, token in enumerate((248045,))]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            self.assertEqual(len(MODULE.validate_rust_controls(path, {"adapterLoaded": True, "adapterScale": 1.3}, "auto")), 1)
            for capture, backend in (({"adapterLoaded": True, "adapterScale": 0}, "auto"),
                                     ({"adapterLoaded": False, "adapterScale": 0}, "auto"),
                                     ({"adapterLoaded": True, "adapterScale": 1.3}, "scalar")):
                with self.assertRaises(ValueError):
                    MODULE.validate_rust_controls(path, capture, backend)
            records[-1]["input_token"] = 198
            (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            with self.assertRaises(ValueError):
                MODULE.validate_rust_controls(path, {"adapterLoaded": True, "adapterScale": 1.3}, "auto")

    def test_row_worker_controls_reject_cross_worker_and_missing_parallel_execution(self):
        records = [{"event": "weights-loaded", "model_sha256": MODULE.MODEL,
                    "weight_representation": "current-repacked-PTQ1_0"},
                   {"event": "rank-one-adapter-loaded", "model_sha256": MODULE.MODEL,
                    "adapter_sha256": MODULE.ADAPTER, "target_pairs": 129, "scale_bits": "3fa66666"},
                   {"event": "forward-step", "position": 0, "input_token": 248045,
                    "backend_requested": "auto", "row_workers": 1, "parallel_row_operations": 0}]
        capture = {"adapterLoaded": True, "adapterScale": 1.3}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            def write():
                (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            for workers, operations in ((1, 0), (4, 123)):
                for backend in MODULE.BACKENDS:
                    records[-1].update(row_workers=workers, parallel_row_operations=operations,
                                       backend_requested=backend)
                    write()
                    self.assertEqual(len(MODULE.validate_rust_controls(path, capture, backend, workers)), 1)
                    with self.assertRaises(ValueError):
                        MODULE.validate_rust_controls(path, capture, backend, 4 if workers == 1 else 1)
            for field, value in (("row_workers", None), ("row_workers", True), ("row_workers", "4"),
                                 ("row_workers", 1), ("parallel_row_operations", None),
                                 ("parallel_row_operations", False), ("parallel_row_operations", 0),
                                 ("parallel_row_operations", -1), ("parallel_row_operations", 1.0)):
                records[-1].update(row_workers=4, parallel_row_operations=123, backend_requested="auto")
                if value is None:
                    records[-1].pop(field)
                else:
                    records[-1][field] = value
                write()
                with self.assertRaises(ValueError):
                    MODULE.validate_rust_controls(path, capture, "auto", 4)
            records[-1].update(row_workers=1, parallel_row_operations=1)
            write()
            with self.assertRaises(ValueError):
                MODULE.validate_rust_controls(path, capture, "auto", 1)
            for unsupported in (0, 2, 16, True, "4"):
                with self.assertRaises(ValueError):
                    MODULE.validate_rust_controls(path, capture, "auto", unsupported)

    def test_transposed_v_and_complete_recurrent_inventory_match_logical_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            results = []
            for transposed in (0, 1):
                data, layout = fixture(transposed)
                path = Path(directory) / str(transposed)
                path.write_bytes(data)
                values = {name: [x[0] for x in struct.iter_unpack("<" + code, raw)]
                          for name, raw, code in MODULE.native_components(path, 1, layout)}
                self.assertEqual(values["block-3-value-0.f32le"], [5.0, 6.0])
                self.assertEqual(values["block-3-value-1.f32le"], [7.0, 8.0])
                self.assertEqual(values["block-0-convolution.f32le"], [9.0, 10.0, 11.0])
                self.assertEqual(values["block-0-recurrent.f32le"], [12.0, 13.0, 14.0, 15.0])
                self.assertEqual(len(values), 6)
                results.append(values)
            self.assertEqual(*results)

    def test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema(self):
        data, layout = fixture(1)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state"
            for bad in (data[:-1], data + b"extra", b"\x02\0\0\0" + data[4:],
                        data[:4] + b"\x01\0\0\0" + data[8:],
                        data[:8] + b"\x02\0\0\0" + data[12:]):
                path.write_bytes(bad)
                with self.assertRaises(ValueError):
                    list(MODULE.native_components(path, 1, layout))

    def test_capture_pre_read_bounds_reject_oversized_json_and_logits(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input"
            path.write_bytes(b"x" * 17)
            with self.assertRaises(ValueError):
                MODULE.read_bounded(path, 16)

    def test_comparator_enforces_finite_values_exact_extents_and_original_tolerance(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "actual"
            reference = struct.pack("2e", 1.0, -0.0)
            path.write_bytes(struct.pack("2f", 1.001, -0.0))
            self.assertEqual(MODULE.compare_payload(reference, "e", path)["failedElements"], 0)
            path.write_bytes(struct.pack("2f", 1.003, -0.0))
            self.assertEqual(MODULE.compare_payload(reference, "e", path)["failedElements"], 1)
            path.write_bytes(struct.pack("2f", float("nan"), 0.0))
            with self.assertRaises(ValueError):
                MODULE.compare_payload(reference, "e", path)
            path.write_bytes(struct.pack("f", 1.0))
            with self.assertRaises(ValueError):
                MODULE.compare_payload(reference, "e", path)


if __name__ == "__main__":
    unittest.main()
