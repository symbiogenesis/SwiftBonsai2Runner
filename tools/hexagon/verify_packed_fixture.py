"""Check immutable packed operation-capture provenance without opening a device."""

import hashlib
import json
from pathlib import Path


def verify_hmx_control(root, manifest):
    """Keep arithmetic and source bounds distinct from native HMX eligibility."""
    control = manifest["hmxIntegerBitplaneProjectionControl"]
    source_names = ("hmx_projection.rs", "hmx_projection_tests.rs")
    if (control["source"], control["independentControls"]) != source_names:
        raise ValueError("HMX projection source inventory changed")
    if any(not (root / name).is_file() for name in source_names):
        raise ValueError("HMX projection or independent controls missing")
    expected = {
        "primitiveK": 128,
        "primitiveN": 8,
        "activationPlaneCoefficients": [1, 2, 4, 8, 16, 32, 64, -128],
        "activationPlaneValues": [0, 1],
        "minimumPlaneDot": -128,
        "maximumPlaneDot": 256,
        "weightRowsPerHostTile": 32,
        "weightColumnsPerHostTile": 64,
        "hostTileBytes": 576,
        "hostTilesPer32WeightRows": 2,
        "maximumColumns": 5120,
        "maximumRows": 17408,
        "fullMFirstProjectionOperations": 40,
        "fullMFirstProjectionPeakRepackedStageBytes": 626688,
        "fullMFirstProjectionPreparedQ8KBytes": 5840,
        "fullMFirstProjectionPlaneActivationBytes": 4096,
        "fullMFirstProjectionPeakPlaneOutputBytes": 557056,
        "fullMFirstProjectionPrivateFinalOutputBytes": 69632,
        "fullMFirstProjectionOwnedPayloadBytes": 1263312,
        "fullMFirstProjectionRepackedWeightTransferBytes": 25067520,
        "fullMFirstProjectionActivationTransferBytes": 163840,
        "fullMFirstProjectionPlaneReturnBytes": 22282240,
    }
    if any(control.get(name) != value for name, value in expected.items()):
        raise ValueError("HMX projection arithmetic or payload bounds changed")
    for name in ("bitplaneRowsArePrefillTokens", "nativeHmxSessionImplemented",
                 "effectiveHmxPlacementQualified", "PTQ1ArithmeticQualified",
                 "profileEligible"):
        if control.get(name) is not False:
            raise ValueError("HMX CPU controls cannot qualify device or model admission")


def verify_ptq1_control(root, manifest):
    """Bind the lane-preserving control to immutable independent native dots."""
    control = manifest["ptq1HmxLaneControl"]
    expected = {
        "source": "ptq1_hmx.rs",
        "independentControls": "ptq1_hmx_tests.rs",
        "originalBlockWeights": 128,
        "originalBlockBytes": 28,
        "primitiveK": 128,
        "primitiveN": 8,
        "q8GroupsPerOriginalBlock": 4,
        "sparseWeightRowsPerOriginalRow": 4,
        "primitiveOperationsPerOriginalBlockAndRowTile": 4,
        "integerProductsPerNativeLane": 8,
        "planeDotMinimum": -8,
        "planeDotMaximum": 8,
        "nativeLaneMinimum": -1024,
        "nativeLaneMaximum": 1024,
        "laneAccumulatorCount": 4,
        "paddedHostStageBytes": 1152,
    }
    if any(control.get(name) != value for name, value in expected.items()):
        raise ValueError("PTQ1 native-lane arithmetic contract changed")
    if any(not (root / control[name]).is_file() for name in ("source", "independentControls")):
        raise ValueError("PTQ1 lane control source missing")
    for name in ("deviceParityQualified", "actualModelParityQualified", "profileEligible"):
        if control.get(name) is not False:
            raise ValueError("PTQ1 CPU controls cannot qualify native/model admission")
    fixture = control["nativeDotFixture"]
    data = (root / fixture["path"]).read_bytes()
    expected_digest = "781fa202c4d23e2a78cf362e3d84e008dd647c31606ba92b7869f5c2168d439c"
    if (len(data) != 508 or fixture["bytes"] != 508
            or hashlib.sha256(data).hexdigest() != expected_digest
            or fixture["sha256"] != expected_digest):
        raise ValueError("PTQ1 independent native dot bytes changed")
    provenance = (root / fixture["provenancePath"]).read_bytes().replace(b"\r\n", b"\n")
    expected_provenance = "21885745d1603934a59ba3ba2b491911f07eb07362003e76a69545140f5b09ba"
    if (hashlib.sha256(provenance).hexdigest() != expected_provenance
            or fixture["provenanceSha256"] != expected_provenance
            or fixture["provenanceDigestEncoding"] != "UTF8-with-LF-newlines"):
        raise ValueError("PTQ1 native dot provenance changed")
    observation = json.loads(provenance)
    if (observation["fixture_sha256"] != expected_digest
            or observation["fixture_bytes"] != 508
            or observation["runtime_commit"] != "73bfe6cc485554822f80c473a3c0c4f25e80f41b"
            or observation["ggml_cpu_sha256"] != "701b354a38b25e5c6dc57a1559f71cd26b2d0f8d69c1576a0e77a59a4273d2ab"):
        raise ValueError("PTQ1 native dot capture identities disagree")


def verify():
    """Check exact retained independent bytes, rather than regenerating controls."""
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / "packed-sources.json").read_text(encoding="utf-8"))
    verify_hmx_control(root, manifest)
    verify_ptq1_control(root, manifest)
    capture = manifest["independentNativeCapture"]
    fixture = capture["preparationFixture"]
    # Hard pins prevent updating an expected field and its payload together from
    # silently replacing the independent native observation.
    expected = "86d985b8a60fde3241daeeb1712dac45a0cc8e4684ac962dcb488ef17bb1fba5"
    data = (root / fixture["path"]).read_bytes()
    if len(data) != 584 or fixture["bytes"] != 584:
        raise ValueError("native Q8_K fixture extent changed")
    if hashlib.sha256(data).hexdigest() != expected or fixture["sha256"] != expected:
        raise ValueError("native Q8_K fixture digest changed")
    provenance_bytes = (root / fixture["provenancePath"]).read_bytes().replace(b"\r\n", b"\n")
    provenance_digest = "131155bb668a4ca3c4897ff5b270a6f9560e83da173fd6d14cf1dd3228b0c7cd"
    if (hashlib.sha256(provenance_bytes).hexdigest() != provenance_digest
            or fixture["provenanceSha256"] != provenance_digest
            or fixture["provenanceDigestEncoding"] != "UTF8-with-LF-newlines"):
        raise ValueError("native Q8_K provenance digest changed")
    provenance = json.loads(provenance_bytes)
    if provenance["runtime_commit"] != "73bfe6cc485554822f80c473a3c0c4f25e80f41b":
        raise ValueError("native runtime identity changed")
    if provenance["ggml_cpu_sha256"] != "701b354a38b25e5c6dc57a1559f71cd26b2d0f8d69c1576a0e77a59a4273d2ab":
        raise ValueError("native binary identity changed")
    if provenance["fixture_sha256"] != expected or provenance["fixture_bytes"] != len(data):
        raise ValueError("native capture identities disagree")
    for index, case in enumerate(provenance["cases"]):
        block = data[index * 292:(index + 1) * 292]
        if len(block) != 292 or hashlib.sha256(block).hexdigest() != case["native_block_sha256"]:
            raise ValueError("native case digest changed")
    if len(provenance["cases"]) != 2:
        raise ValueError("native case inventory changed")
    print("Retained native Q8_K/PTQ1 fixture identities verified; device/Swift gates remain pending.")


if __name__ == "__main__":
    verify()
