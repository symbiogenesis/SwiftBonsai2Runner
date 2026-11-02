"""Derive resident-host payload bounds without opening a model or device."""

import argparse
import hashlib
import json

from verify_operation_traffic import ROOT, derive as derive_streaming, read


RETAINED = "docs/evidence/hexagon-resident-host-ledger.json"
SOURCES = (
    "tools/hexagon/verify_resident_traffic.py",
    "tools/hexagon/verify_operation_traffic.py",
    "docs/evidence/hexagon-swift-operation-ledger.json",
    "docs/evidence/hexagon-swift-traffic-ledger.json",
)
MAXIMUM_ROWS = 17_408
MAXIMUM_OPERATIONS = 21_760
MAXIMUM_RESIDENT_ARENA_BYTES = 64 * 1024 * 1024
SERVICE_BYTES = 12_348_030_976


def align(value, boundary):
    """Round a nonnegative byte extent upward without floating arithmetic."""
    return (value + boundary - 1) // boundary * boundary


def scenario(name, operations, original, weights, activation, results):
    """Separate repeated CPU packing, kernel payloads and mapped descriptor size."""
    # The existing HMX command, scratch weight tile, activation planes and
    # results remain present before the optional append-only host arena.
    base_allocation = align(
        align(1024 + MAXIMUM_ROWS * 36, 128) + 4096 + MAXIMUM_ROWS * 32,
        4096,
    )
    resident_fits = weights <= MAXIMUM_RESIDENT_ARENA_BYTES
    allocation = align(base_allocation + weights, 4096) if resident_fits else None
    return {
        "placement": name,
        "operationsPerToken": operations,
        "originalSelectedWeightBytes": original,
        "minimumPackedResidentArenaBytes": weights,
        "fitsMaximumResidentArena": resident_fits,
        "minimumOriginalPlusResidentWeightBytes": original + weights,
        "minimumWeightsFitServiceAllowance": original + weights <= SERVICE_BYTES,
        "baseNativeAllocationBytes": base_allocation,
        "nativeAllocationWithMinimumArenaBytes": allocation,
        "kernelLogicalPayloadPerTokenBytes": weights + activation + results,
        "kernelWeightPayloadPerTokenBytes": weights,
        "activationPayloadPerTokenBytes": activation,
        "returnedPlanePayloadPerTokenBytes": results,
        "baselineDescriptorCoveredExtentPerTokenBytes": operations * base_allocation,
        "residentDescriptorCoveredExtentPerTokenBytes": (
            operations * allocation if allocation is not None else None
        ),
        "repeatedProjectionCases": [
            {
                "tokens": tokens,
                "operations": operations * tokens,
                "fitsTerminalOperationCap": operations * tokens <= MAXIMUM_OPERATIONS,
                "streamingCpuPackedWeightBytes": weights * tokens,
                "residentCpuPackedWeightBytesLowerBound": weights,
                "avoidedRepeatedCpuPackingBytesUpperBound": weights * (tokens - 1),
                "kernelLogicalPayloadBytes": (weights + activation + results) * tokens,
            }
            for tokens in (1, 8, 128)
        ],
    }


def derive():
    """Reproduce conditional arithmetic, with every unqualified boundary explicit."""
    streaming = derive_streaming()
    if json.loads(read("docs/evidence/hexagon-swift-traffic-ledger.json")) != streaming:
        raise ValueError("streaming source identities or logical payloads changed")
    first = streaming["projectionFamilies"][0]
    if first["tensor"] != "blk.*.ffn_up.weight":
        raise ValueError("first-projection family changed")
    fields = (
        "operations", "originalWeightBytes", "hmxRepackedWeightBytes",
        "hmxActivationBytes", "hmxReturnedPlaneBytes",
    )
    # The first family covers all 64 layers; the selected prototype is one
    # immutable blk.0.ffn_up.weight projection, repeated for several tokens.
    if any(first[field] % 64 for field in fields):
        raise ValueError("first-projection family no longer divides into 64 layers")
    totals = streaming["totals"]
    return {
        "schemaVersion": 1,
        "kind": "static-original-pq2-resident-host-payload-bounds",
        "sourceIdentityEncoding": "UTF8-with-LF-newlines",
        "sources": [
            {"path": path, "sha256": hashlib.sha256(read(path)).hexdigest()}
            for path in SOURCES
        ],
        "assumptions": {
            "maximumWeightRowsPerOperation": MAXIMUM_ROWS,
            "terminalOperationCap": MAXIMUM_OPERATIONS,
            "maximumResidentArenaBytes": MAXIMUM_RESIDENT_ARENA_BYTES,
            "residentRangeAlignmentBytes": 128,
            "allocationAlignmentBytes": 4096,
            "swiftServiceAllowanceBytes": SERVICE_BYTES,
            "fullAllocationQueueBufferFlags": "0x90",
            "residentArena": "immutable packed weights in mapped host RAM; not DSP VTCM",
            "allSelectedWeightsRetainedForEveryRepeatedToken": True,
            "cacheEviction": False,
        },
        "scenarios": [
            scenario("blk.0.ffn_up.weight", *(first[field] // 64 for field in fields)),
            scenario("all-eligible-original-PQ2-plus-logits-idealized", *(
                totals[field] for field in fields
            )),
        ],
        "limitations": [
            "Conditional byte arithmetic, not runtime admission, actual cache occupancy, measured bandwidth or inference speedup.",
            "The first scenario assumes its complete weight payload is resident and reused; metadata, scales, temporary packing, allocator, CPU graph and DSP resources are additional.",
            "The all-eligible scenario exceeds both the 64MiB arena and the unchanged terminal operation cap even for one token; it is not executable under this contract.",
            "Resident mapped host weights remove repeated CPU packing/copy only when hits occur; they do not reduce the signed kernel's declared weight or returned-plane payloads.",
            "The full expanded allocation remains covered by each queue descriptor with unchanged 0x90 flags; actual cache maintenance may grow and is unmeasured.",
            "Descriptor-covered extent is not bytes transferred, cache-line work, DRAM traffic or simultaneous memory occupancy.",
            "No DSP weight residency, fused kernel, multitoken HMX prefill, persistent service, PTQ1 qualification or production backend is established.",
            "Repeated-token cases are arithmetic examples within operation bounds only, without a duration, numerical, model-state, resource or device acceptance claim.",
        ],
        "newOwnerDeviceExecutionMilliseconds": 0,
        "deviceOpened": False,
        "modelLoaded": False,
        "profileEligible": False,
        "measuredInferenceSpeedup": None,
    }


def main():
    """Verify the retained arithmetic by default; regeneration is explicit."""
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--emit", action="store_true", help="print derived JSON")
    group.add_argument("--write", action="store_true", help="replace retained static JSON")
    args = parser.parse_args()
    expected = derive()
    if args.emit or args.write:
        encoded = json.dumps(expected, indent=2) + "\n"
        if args.write:
            (ROOT / RETAINED).write_text(encoded, encoding="utf-8", newline="\n")
        else:
            print(encoded, end="")
    elif json.loads(read(RETAINED)) != expected:
        raise ValueError("retained resident-host bounds differ from source-derived arithmetic")
    else:
        print("Resident-host payload bounds verified; no device, model or speed measurement.")


if __name__ == "__main__":
    main()
