"""Derive the current terminal operation budget, retaining historical ledgers."""

import argparse
import hashlib
import json
import re

from verify_operation_traffic import OPERATIONS, ROOT, read


RETAINED = "docs/evidence/hexagon-session-operation-budget.json"
NATIVE = "tools/hexagon/native_rpc/projection.rs"
SOURCES = ("tools/hexagon/verify_operation_budget.py", NATIVE, OPERATIONS)


def derive():
    """Recompute capacity from the enforced source cap and original geometry."""
    native = read(NATIVE).decode("utf-8")
    declarations = re.findall(
        r"pub const Q2_PROJECTION_MAX_OPERATIONS: u32 = ([0-9_]+);", native
    )
    if len(declarations) != 1:
        raise ValueError("one enforced native operation cap is required")
    cap = int(declarations[0].replace("_", ""))
    if not 0 < cap <= 2**32 - 1:
        raise ValueError("native cap exceeds the operation counter")
    ledger = json.loads(read(OPERATIONS))
    families = ledger["projectionFamilies"]
    counts = []
    for family in families:
        layers, columns, rows = (family[key] for key in ("layers", "columns", "weightRows"))
        if any(type(value) is not int or value <= 0 for value in (layers, columns, rows)):
            raise ValueError("positive integer family extents required")
        if columns % 256 or rows % 32:
            raise ValueError("original Q8_K geometry required")
        count = layers * (columns // 128) * ((rows + 17407) // 17408)
        if count != family["operationsPerToken"]:
            raise ValueError("historical family geometry no longer agrees")
        counts.append(count)
    if families[-1]["tensor"] != "output.weight":
        raise ValueError("final logits family must be explicit")
    evaluate, logits = sum(counts), counts[-1]
    hidden = evaluate - logits
    first = counts[0] // families[0]["layers"]
    return {
        "schemaVersion": 1,
        "kind": "current-original-pq2-terminal-operation-budget",
        "sourceIdentityEncoding": "UTF8-with-LF-newlines",
        "sources": [
            {"path": path, "sha256": hashlib.sha256(read(path)).hexdigest()}
            for path in SOURCES
        ],
        "historicalLedgerUnchanged": True,
        "historicalTerminalMaximumOperations": ledger["totals"]["terminalSessionMaximumOperations"],
        "terminalMaximumOperations": cap,
        "hiddenOperationsPerToken": hidden,
        "evaluateOperationsPerToken": evaluate,
        "oneAllEligibleEvaluationFits": evaluate <= cap,
        "remainingAfterOneAllEligibleEvaluation": max(0, cap - evaluate),
        "oneHiddenPlusOneEvaluationOperations": hidden + evaluate,
        "oneHiddenPlusOneEvaluationFits": hidden + evaluate <= cap,
        "firstProjectionOperationsPerToken": first,
        "maximumFirstProjectionEvaluationsByOperationsOnly": cap // first,
        "sessionMaximumSeconds": 120,
        "cleanupReserveSeconds": 3,
        "completeRequestAdmissionRequired": True,
        "resetReplenishesCapacity": False,
        "deviceOpened": False,
        "modelLoaded": False,
        "profileEligible": False,
        "measuredInferenceSpeedup": None,
        "limitations": [
            "Static original-PQ2 counts; PTQ1, smaller row tiles and partial placement need independent accounting.",
            "One evaluation fitting does not admit a prompt and generated reply or establish a persistent serving lifetime.",
            "The complete unreused prompt, output reserve and prior session attempts must fit before inference.",
            "An operation count establishes no device arithmetic, available resources, duration or performance benefit.",
        ],
    }


def main():
    """Verify current evidence by default; explicit generation leaves history intact."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="write current static budget")
    args = parser.parse_args()
    expected = derive()
    if args.write:
        (ROOT / RETAINED).write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8", newline="\n")
    elif json.loads(read(RETAINED)) != expected:
        raise ValueError("current terminal operation budget differs from source")
    else:
        print("Current operation budget verified; no model or device execution.")


if __name__ == "__main__":
    main()
