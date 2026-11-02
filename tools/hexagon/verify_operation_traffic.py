"""Reproduce original-PQ2 bridge payload accounting without models or devices."""

import argparse
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OPERATIONS = "docs/evidence/hexagon-swift-operation-ledger.json"
TRAFFIC = "docs/evidence/hexagon-swift-traffic-ledger.json"
SOURCES = (
    OPERATIONS,
    "tools/hexagon/packed_projection.rs",
    "tools/hexagon/q2_integer.rs",
    "tools/hexagon/q2_projection.rs",
    "tools/hexagon/hmx_projection.rs",
    "tools/hexagon/packed-sources.json",
)


def read(path):
    """Bound source/evidence reads and normalize only checkout line endings."""
    with (ROOT / path).open("rb") as stream:
        data = stream.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("source/evidence exceeds one MiB")
    return data.replace(b"\r\n", b"\n")


def derive():
    """Count logical submission/result payloads, never measured DRAM traffic."""
    ledger = json.loads(read(OPERATIONS))
    assumptions = ledger["assumptions"]
    if (ledger["artifact"]["tensorKind"], assumptions["originalBlockColumns"],
            assumptions["maximumWeightRowsPerOperation"]) != (142, 128, 17408):
        raise ValueError("original PQ2 scheduling contract changed")
    families = []
    for family in ledger["projectionFamilies"]:
        layers, columns, rows = (family[key] for key in ("layers", "columns", "weightRows"))
        if any(type(value) is not int or value <= 0 for value in (layers, columns, rows)):
            raise ValueError("positive integer family extents required")
        if columns % 256 or rows % 32:
            raise ValueError("this ledger requires complete Q8_K and unpadded 32-row tiles")
        row_tiles = (rows + 17407) // 17408
        operations = layers * (columns // 128) * row_tiles
        if operations != family["operationsPerToken"] or row_tiles != family["rowTiles"]:
            raise ValueError("family operation geometry disagrees with source ledger")
        blocks = layers * rows * (columns // 128)
        families.append({
            "tensor": family["tensor"],
            "operations": operations,
            "original128Blocks": blocks,
            "originalWeightBytes": blocks * 34,
            "hmxRepackedWeightBytes": blocks * 36,
            "hmxActivationBytes": operations * 4096,
            "hmxReturnedPlaneBytes": blocks * 32,
            "guardedRepackedWeightBytes": blocks * 72,
            "guardedActivationBytes": operations * 2048,
            "guardedReturnedLimbBytes": blocks * 8,
        })
    totals = {
        key: sum(family[key] for family in families)
        for key in families[0] if key != "tensor"
    }
    if totals["operations"] != ledger["totals"]["evaluateOperationsPerToken"]:
        raise ValueError("complete evaluate operation total changed")
    totals["hmxSubmissionAndResultPayloadBytes"] = sum(totals[key] for key in (
        "hmxRepackedWeightBytes", "hmxActivationBytes", "hmxReturnedPlaneBytes"))
    totals["guardedSubmissionAndResultPayloadBytes"] = sum(totals[key] for key in (
        "guardedRepackedWeightBytes", "guardedActivationBytes", "guardedReturnedLimbBytes"))
    # The independently retained first-projection ledger anchors the per-block
    # factors before scaling them to all model projection geometries.
    packed_sources = json.loads(read("tools/hexagon/packed-sources.json"))
    control = packed_sources["hmxIntegerBitplaneProjectionControl"]
    first = families[0]
    for field, retained in (
        ("hmxRepackedWeightBytes", "fullMFirstProjectionRepackedWeightTransferBytes"),
        ("hmxActivationBytes", "fullMFirstProjectionActivationTransferBytes"),
        ("hmxReturnedPlaneBytes", "fullMFirstProjectionPlaneReturnBytes"),
    ):
        if first[field] // 64 != control[retained]:
            raise ValueError("whole-model derivation disagrees with first-projection control")
    return {
        "schemaVersion": 1,
        "kind": "static-original-pq2-bridge-logical-payload-ledger",
        "scope": ledger["scope"],
        "sourceIdentityEncoding": "UTF8-with-LF-newlines",
        "sources": [{"path": path, "sha256": hashlib.sha256(read(path)).hexdigest()} for path in SOURCES],
        "factors": {
            "original128BlockBytes": 34,
            "hostTileRows": 32,
            "hostTileBytes": 576,
            "hmxHostTilesPer32Rows": 2,
            "guardedHostTilesPer32Rows": 4,
            "hmxPlanesPerOriginalBlock": 8,
            "guardedLimbsPerOriginalBlock": 2,
            "f32Bytes": 4,
            "hmxActivationBytesPerOperation": 8 * 128 * 4,
            "guardedActivationBytesPerOperation": 2 * 256 * 4,
        },
        "projectionFamilies": families,
        "totals": totals,
        "limitations": [
            "All listed original-PQ2 projections selected once, including final logits; PTQ1 is outside this derivation.",
            "Payloads follow existing source packing and result layouts, with no persistent device weight reuse.",
            "Every listed weight-row extent is a multiple of 32, so mapped row padding adds no payload here.",
            "Device/DRAM/bus traffic, bandwidth, latency, energy and speedup are unmeasured.",
            "Excludes source reads, extra host copies, scale scans, preparation, reduction, queue descriptors, cache traffic, mappings and native/DSP resources.",
            "Aggregate per-token payload is not simultaneous memory residency or a capacity admission amount.",
            "Smaller row tiles may increase activation payload and operation count; partial placement changes all totals.",
        ],
        "newOwnerDeviceExecutionMilliseconds": 0,
        "deviceOpened": False,
        "modelLoaded": False,
        "profileEligible": False,
        "measuredInferenceSpeedup": None,
    }


def main():
    """Verify the retained artifact by default; generation is explicit."""
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--emit", action="store_true", help="print the derived JSON")
    group.add_argument("--write", action="store_true", help="replace the retained static JSON")
    args = parser.parse_args()
    expected = derive()
    if args.emit or args.write:
        encoded = json.dumps(expected, indent=2) + "\n"
        if args.write:
            (ROOT / TRAFFIC).write_text(encoded, encoding="utf-8", newline="\n")
        else:
            print(encoded, end="")
    elif json.loads(read(TRAFFIC)) != expected:
        raise ValueError("retained traffic ledger differs from source-derived payloads/identities")
    else:
        print("Original-PQ2 logical payload ledger verified; no device, model or performance measurement.")


if __name__ == "__main__":
    main()
