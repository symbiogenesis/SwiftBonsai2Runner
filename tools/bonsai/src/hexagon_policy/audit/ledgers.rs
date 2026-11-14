//! Checked integer derivation of original-PQ2 logical payloads and operation caps.
use super::{
    BUDGET, NATIVE, OPERATION, ROOT, TRAFFIC, add, align, bytes, contract, contracts, integer, mul,
    read, require, source, sources,
};
use crate::{Result, identity};
use serde_json::{Value, json};
use std::path::Path;

fn geometry(family: &Value) -> Result<(u64, u64, u64, u64, u64)> {
    let layers = integer(family, "layers")?;
    let columns = integer(family, "columns")?;
    let rows = integer(family, "weightRows")?;
    require(
        layers > 0
            && columns > 0
            && rows > 0
            && columns.is_multiple_of(256)
            && rows.is_multiple_of(32),
        "Positive complete Q8_K and 32-row extents required.",
    )?;
    let tiles = rows.div_ceil(17408);
    let operations = mul(mul(layers, columns / 128)?, tiles)?;
    require(
        operations == integer(family, "operationsPerToken")?
            && tiles == integer(family, "rowTiles")?,
        "Family operation geometry disagrees with historical ledger.",
    )?;
    Ok((layers, columns, rows, tiles, operations))
}
pub(super) fn traffic(root: &Path) -> Result<Value> {
    let ledger = read(root, OPERATION)?;
    require(
        ledger["artifact"]["tensorKind"] == 142
            && ledger["assumptions"]["originalBlockColumns"] == 128
            && ledger["assumptions"]["maximumWeightRowsPerOperation"] == 17408,
        "Original PQ2 scheduling contract changed.",
    )?;
    let mut families = Vec::new();
    for family in ledger["projectionFamilies"]
        .as_array()
        .ok_or("Projection families missing.")?
    {
        let (layers, columns, rows, _, operations) = geometry(family)?;
        let blocks = mul(mul(layers, rows)?, columns / 128)?;
        families.push(json!({"tensor":family["tensor"],"operations":operations,"original128Blocks":blocks,"originalWeightBytes":mul(blocks,34)?,"hmxRepackedWeightBytes":mul(blocks,36)?,"hmxActivationBytes":mul(operations,4096)?,"hmxReturnedPlaneBytes":mul(blocks,32)?,"guardedRepackedWeightBytes":mul(blocks,72)?,"guardedActivationBytes":mul(operations,2048)?,"guardedReturnedLimbBytes":mul(blocks,8)?}));
    }
    let first = families.first().ok_or("Empty projection inventory.")?;
    let mut totals = serde_json::Map::new();
    for key in first
        .as_object()
        .ok_or("Invalid projection object.")?
        .keys()
        .filter(|key| *key != "tensor")
    {
        let sum = families
            .iter()
            .try_fold(0, |sum, family| add(sum, integer(family, key)?))?;
        totals.insert(key.clone(), json!(sum));
    }
    let mut totals = Value::Object(totals);
    require(
        totals["operations"] == ledger["totals"]["evaluateOperationsPerToken"],
        "Complete operation total changed.",
    )?;
    for (prefix, result) in [
        ("hmx", "hmxSubmissionAndResultPayloadBytes"),
        ("guarded", "guardedSubmissionAndResultPayloadBytes"),
    ] {
        let weight = integer(&totals, &format!("{prefix}RepackedWeightBytes"))?;
        let activation = integer(&totals, &format!("{prefix}ActivationBytes"))?;
        let returned = integer(
            &totals,
            if prefix == "hmx" {
                "hmxReturnedPlaneBytes"
            } else {
                "guardedReturnedLimbBytes"
            },
        )?;
        totals[result] = json!(add(add(weight, activation)?, returned)?);
    }
    let packed = read(root, "tools/hexagon/packed-sources.json")?;
    for (field, control) in [
        (
            "hmxRepackedWeightBytes",
            "fullMFirstProjectionRepackedWeightTransferBytes",
        ),
        (
            "hmxActivationBytes",
            "fullMFirstProjectionActivationTransferBytes",
        ),
        (
            "hmxReturnedPlaneBytes",
            "fullMFirstProjectionPlaneReturnBytes",
        ),
    ] {
        let total = integer(first, field)?;
        require(
            total.is_multiple_of(64)
                && total / 64 == integer(&packed["hmxIntegerBitplaneProjectionControl"], control)?,
            "Whole-model payload differs from first-projection control.",
        )?;
    }
    let mut report = contract(contracts::TRAFFIC)?;
    report["scope"] = ledger["scope"].clone();
    report["projectionFamilies"] = json!(families);
    report["totals"] = totals;
    report["sources"] = sources(
        root,
        &[
            OPERATION,
            "tools/hexagon/packed_projection.rs",
            "tools/hexagon/q2_integer.rs",
            "tools/hexagon/q2_projection.rs",
            "tools/hexagon/hmx_projection.rs",
            "tools/hexagon/packed-sources.json",
            &format!("{ROOT}ledgers.rs"),
            &format!("{ROOT}contracts.rs"),
            &format!("{ROOT}mod.rs"),
        ],
    )?;
    Ok(report)
}
pub(super) fn budget(root: &Path) -> Result<Value> {
    let native = "tools/hexagon/native_rpc/projection.rs";
    let raw = source(root, native)?;
    let text = std::str::from_utf8(&raw).map_err(|error| error.to_string())?;
    let prefix = "pub const Q2_PROJECTION_MAX_OPERATIONS: u32 = ";
    let declarations = text.match_indices(prefix).collect::<Vec<_>>();
    require(
        declarations.len() == 1,
        "One enforced native operation cap is required.",
    )?;
    let tail = &text[declarations[0].0 + prefix.len()..];
    let literal = tail
        .split_once(';')
        .ok_or("Operation cap declaration incomplete.")?
        .0;
    require(
        !literal.is_empty()
            && literal
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'_'),
        "Operation cap must be an integer literal.",
    )?;
    let cap = literal
        .replace('_', "")
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    require(cap > 0, "Native operation cap must be positive.")?;
    let cap = u64::from(cap);
    let ledger = read(root, OPERATION)?;
    let families = ledger["projectionFamilies"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("Projection inventory missing.")?;
    let counts = families
        .iter()
        .map(|family| geometry(family).map(|v| v.4))
        .collect::<Result<Vec<_>>>()?;
    require(
        families
            .last()
            .is_some_and(|family| family["tensor"] == "output.weight"),
        "Final logits family must be explicit.",
    )?;
    let evaluate = counts.iter().try_fold(0, |sum, count| add(sum, *count))?;
    let hidden = evaluate - counts[counts.len() - 1];
    let first = counts[0] / integer(&families[0], "layers")?;
    let combined = add(hidden, evaluate)?;
    let mut report = contract(contracts::BUDGET)?;
    report["sources"] = sources(
        root,
        &[
            &format!("{ROOT}ledgers.rs"),
            &format!("{ROOT}contracts.rs"),
            &format!("{ROOT}mod.rs"),
            native,
            OPERATION,
        ],
    )?;
    for (key, value) in [
        (
            "historicalTerminalMaximumOperations",
            ledger["totals"]["terminalSessionMaximumOperations"].clone(),
        ),
        ("terminalMaximumOperations", json!(cap)),
        ("hiddenOperationsPerToken", json!(hidden)),
        ("evaluateOperationsPerToken", json!(evaluate)),
        ("oneAllEligibleEvaluationFits", json!(evaluate <= cap)),
        (
            "remainingAfterOneAllEligibleEvaluation",
            json!(cap.saturating_sub(evaluate)),
        ),
        ("oneHiddenPlusOneEvaluationOperations", json!(combined)),
        ("oneHiddenPlusOneEvaluationFits", json!(combined <= cap)),
        ("firstProjectionOperationsPerToken", json!(first)),
        (
            "maximumFirstProjectionEvaluationsByOperationsOnly",
            json!(cap / first),
        ),
    ] {
        report[key] = value;
    }
    Ok(report)
}
fn scenario(name: &str, fields: &[u64; 5], maximum: u64) -> Result<Value> {
    let [operations, original, weights, activation, results] = *fields;
    let base = align(
        add(
            add(align(add(1024, mul(17408, 36)?)?, 128)?, 4096)?,
            mul(17408, 32)?,
        )?,
        4096,
    )?;
    let fits = weights <= 67_108_864;
    let allocation = if fits {
        Some(align(add(base, weights)?, 4096)?)
    } else {
        None
    };
    let payload = add(add(weights, activation)?, results)?;
    let cases = [1,8,128].map(|tokens| Ok(json!({"tokens":tokens,"operations":mul(operations,tokens)?,"fitsTerminalOperationCap":mul(operations,tokens)?<=maximum,"streamingCpuPackedWeightBytes":mul(weights,tokens)?,"residentCpuPackedWeightBytesLowerBound":weights,"avoidedRepeatedCpuPackingBytesUpperBound":mul(weights,tokens-1)?,"kernelLogicalPayloadBytes":mul(payload,tokens)?}))).into_iter().collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"placement":name,"operationsPerToken":operations,"originalSelectedWeightBytes":original,"minimumPackedResidentArenaBytes":weights,"fitsMaximumResidentArena":fits,"minimumOriginalPlusResidentWeightBytes":add(original,weights)?,"minimumWeightsFitServiceAllowance":add(original,weights)?<=12_348_030_976,"baseNativeAllocationBytes":base,"nativeAllocationWithMinimumArenaBytes":allocation,"kernelLogicalPayloadPerTokenBytes":payload,"kernelWeightPayloadPerTokenBytes":weights,"activationPayloadPerTokenBytes":activation,"returnedPlanePayloadPerTokenBytes":results,"baselineDescriptorCoveredExtentPerTokenBytes":mul(operations,base)?,"residentDescriptorCoveredExtentPerTokenBytes":allocation.map(|value|mul(operations,value)).transpose()?,"repeatedProjectionCases":cases}),
    )
}
pub(super) fn resident(root: &Path) -> Result<Value> {
    let streaming = traffic(root)?;
    let budget = budget(root)?;
    require(
        read(root, TRAFFIC)? == streaming && read(root, BUDGET)? == budget,
        "Current streaming or terminal budget ledger changed.",
    )?;
    let first = &streaming["projectionFamilies"][0];
    require(
        first["tensor"] == "blk.*.ffn_up.weight",
        "First-projection family changed.",
    )?;
    let (mut one, mut all) = ([0; 5], [0; 5]);
    for (index, field) in [
        "operations",
        "originalWeightBytes",
        "hmxRepackedWeightBytes",
        "hmxActivationBytes",
        "hmxReturnedPlaneBytes",
    ]
    .iter()
    .enumerate()
    {
        let value = integer(first, field)?;
        require(
            value.is_multiple_of(64),
            "First-projection family must divide into 64 layers.",
        )?;
        one[index] = value / 64;
        all[index] = integer(&streaming["totals"], field)?;
    }
    let maximum = integer(&budget, "terminalMaximumOperations")?;
    let mut report = contract(contracts::RESIDENT)?;
    report["sources"] = sources(
        root,
        &[
            &format!("{ROOT}ledgers.rs"),
            &format!("{ROOT}contracts.rs"),
            &format!("{ROOT}mod.rs"),
            OPERATION,
            TRAFFIC,
            BUDGET,
        ],
    )?;
    report["assumptions"] = json!({"maximumWeightRowsPerOperation":17408,"terminalOperationCap":maximum,"maximumResidentArenaBytes":67_108_864,"residentRangeAlignmentBytes":128,"allocationAlignmentBytes":4096,"swiftServiceAllowanceBytes":12_348_030_976_u64,"fullAllocationQueueBufferFlags":"0x90","residentArena":"immutable packed weights in mapped host RAM; not DSP VTCM","allSelectedWeightsRetainedForEveryRepeatedToken":true,"cacheEviction":false});
    report["scenarios"] = json!([
        scenario("blk.0.ffn_up.weight", &one, maximum)?,
        scenario(
            "all-eligible-original-PQ2-plus-logits-idealized",
            &all,
            maximum
        )?
    ]);
    Ok(report)
}

fn packed_contracts(root: &Path, manifest: &Value) -> Result<()> {
    for (name, expected, flags) in [
        (
            "hmxIntegerBitplaneProjectionControl",
            json!({"source":"hmx_projection.rs","independentControls":"hmx_projection_tests.rs","primitiveK":128,"primitiveN":8,"activationPlaneCoefficients":[1,2,4,8,16,32,64,-128],"activationPlaneValues":[0,1],"minimumPlaneDot":-128,"maximumPlaneDot":256,"weightRowsPerHostTile":32,"weightColumnsPerHostTile":64,"hostTileBytes":576,"hostTilesPer32WeightRows":2,"maximumColumns":5120,"maximumRows":17408,"fullMFirstProjectionOperations":40,"fullMFirstProjectionPeakRepackedStageBytes":626_688,"fullMFirstProjectionPreparedQ8KBytes":5840,"fullMFirstProjectionPlaneActivationBytes":4096,"fullMFirstProjectionPeakPlaneOutputBytes":557_056,"fullMFirstProjectionPrivateFinalOutputBytes":69632,"fullMFirstProjectionOwnedPayloadBytes":1_263_312,"fullMFirstProjectionRepackedWeightTransferBytes":25_067_520,"fullMFirstProjectionActivationTransferBytes":163_840,"fullMFirstProjectionPlaneReturnBytes":22_282_240}),
            &[
                "bitplaneRowsArePrefillTokens",
                "nativeHmxSessionImplemented",
                "effectiveHmxPlacementQualified",
                "PTQ1ArithmeticQualified",
                "profileEligible",
            ][..],
        ),
        (
            "ptq1HmxLaneControl",
            json!({"source":"ptq1_hmx.rs","independentControls":"ptq1_hmx_tests.rs","originalBlockWeights":128,"originalBlockBytes":28,"primitiveK":128,"primitiveN":8,"q8GroupsPerOriginalBlock":4,"sparseWeightRowsPerOriginalRow":4,"primitiveOperationsPerOriginalBlockAndRowTile":4,"integerProductsPerNativeLane":8,"planeDotMinimum":-8,"planeDotMaximum":8,"nativeLaneMinimum":-1024,"nativeLaneMaximum":1024,"laneAccumulatorCount":4,"paddedHostStageBytes":1152}),
            &[
                "deviceParityQualified",
                "actualModelParityQualified",
                "profileEligible",
            ][..],
        ),
    ] {
        let control = &manifest[name];
        for (key, value) in expected.as_object().ok_or("Invalid packed contract.")? {
            require(
                control[key] == *value,
                "Packed projection arithmetic or source contract changed.",
            )?;
        }
        for field in ["source", "independentControls"] {
            bytes(
                &root
                    .join("tools/hexagon")
                    .join(control[field].as_str().ok_or("Packed source missing.")?),
                1_048_576,
            )?;
        }
        for flag in flags {
            require(
                control[*flag] == false,
                "CPU controls cannot qualify device or model admission.",
            )?;
        }
    }
    Ok(())
}

pub(super) fn packed(root: &Path) -> Result<Value> {
    let manifest = read(root, "tools/hexagon/packed-sources.json")?;
    packed_contracts(root, &manifest)?;
    for (fixture, size, digest, provenance) in [
        (
            &manifest["ptq1HmxLaneControl"]["nativeDotFixture"],
            508_u64,
            "781fa202c4d23e2a78cf362e3d84e008dd647c31606ba92b7869f5c2168d439c",
            "21885745d1603934a59ba3ba2b491911f07eb07362003e76a69545140f5b09ba",
        ),
        (
            &manifest["independentNativeCapture"]["preparationFixture"],
            584,
            "86d985b8a60fde3241daeeb1712dac45a0cc8e4684ac962dcb488ef17bb1fba5",
            "131155bb668a4ca3c4897ff5b270a6f9560e83da173fd6d14cf1dd3228b0c7cd",
        ),
    ] {
        let relative = |field: &str| {
            fixture[field]
                .as_str()
                .map(|name| format!("tools/hexagon/{name}"))
                .ok_or_else(|| "Fixture path missing.".to_owned())
        };
        let data = bytes(&root.join(relative("path")?), 1_048_576)?;
        require(
            u64::try_from(data.len()).map_err(|error| error.to_string())? == size
                && fixture["bytes"] == size
                && identity::sha256(&data) == digest
                && fixture["sha256"] == digest,
            "Independent native fixture bytes changed.",
        )?;
        let raw = source(root, &relative("provenancePath")?)?;
        require(
            identity::sha256(&raw) == provenance
                && fixture["provenanceSha256"] == provenance
                && fixture["provenanceDigestEncoding"] == "UTF8-with-LF-newlines",
            "Native provenance changed.",
        )?;
        let observed = super::super::json::parse_bounded(
            std::str::from_utf8(&raw).map_err(|error| error.to_string())?,
            1_048_576,
            64,
        )?;
        require(
            observed["fixture_sha256"] == digest
                && observed["fixture_bytes"] == size
                && observed["runtime_commit"] == NATIVE
                && observed["ggml_cpu_sha256"]
                    == "701b354a38b25e5c6dc57a1559f71cd26b2d0f8d69c1576a0e77a59a4273d2ab",
            "Native capture identities disagree.",
        )?;
        if size == 584 {
            let cases = observed["cases"]
                .as_array()
                .filter(|cases| cases.len() == 2)
                .ok_or("Native case inventory changed.")?;
            for (chunk, case) in data.as_chunks::<292>().0.iter().zip(cases) {
                require(
                    identity::sha256(chunk) == case["native_block_sha256"],
                    "Native case digest changed.",
                )?;
            }
        }
    }
    Ok(json!({"Valid":true,"DeviceOpened":false,"ModelLoaded":false,"ProfileEligible":false}))
}
