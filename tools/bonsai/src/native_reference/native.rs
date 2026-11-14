//! Complete native serialized-state decoding and fixed PTQ1 comparison.

use super::{
    ADAPTER, MODEL, compare_bytes, digest, formats::Reader, integer, inventory, path, read,
    read_json, require, text,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

const REVISION: &str = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";

fn geometry(request: &Value, name: &str, default: u64) -> crate::Result<u64> {
    request.get(name).map_or(Ok(default), |value| {
        value
            .as_u64()
            .filter(|n| *n > 0 && *n <= 1_000_000)
            .ok_or_else(|| "invalid decoder geometry".into())
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "The native sequence format is decoded in exact wire order with complete trailing-byte rejection."
)]
pub(super) fn components(path: &Path, position: u64, request: &Value) -> crate::Result<Value> {
    require(position <= 1, "decoder position outside bounded extent")?;
    let layers = geometry(request, "layers", 64)?;
    let width = geometry(request, "width", 1024)?;
    let convolution = geometry(request, "convolution", 30_720)?;
    let matrix = geometry(request, "matrix", 786_432)?;
    let selected = |key: &str, attention: bool| -> crate::Result<Vec<u64>> {
        if let Some(value) = request.get(key) {
            let values = value.as_array().ok_or("invalid decoder block inventory")?;
            let blocks = values
                .iter()
                .map(|item| {
                    item.as_u64()
                        .filter(|n| *n < layers)
                        .ok_or_else(|| "invalid decoder block".into())
                })
                .collect::<crate::Result<Vec<_>>>()?;
            require(
                blocks.iter().copied().collect::<BTreeSet<_>>().len() == blocks.len(),
                "duplicate decoder block",
            )?;
            Ok(blocks)
        } else {
            Ok((0..layers).filter(|n| (*n % 4 == 3) == attention).collect())
        }
    };
    let attention = selected("attention", true)?;
    let recurrent = selected("recurrent", false)?;
    let raw = read(path, 536_870_912)?;
    let mut reader = Reader::new(&raw);
    let count = position + 1;
    require(
        reader.u32()? == 0xaf14_3cd8
            && reader.u32()? == 0
            && reader.u32()? == 1
            && u64::from(reader.u32()?) == count,
        "native sequence metadata changed",
    )?;
    for token in 0..count {
        require(
            u64::from(reader.u32()?) == token
                && reader.u32()? == 1
                && u64::from(reader.u32()?) == token
                && u64::from(reader.u32()?) == token
                && reader.u32()? == 0,
            "native attention position/sequence changed",
        )?;
    }
    let transposed = reader.u32()?;
    require(
        transposed <= 1
            && usize::try_from(reader.u32()?).map_err(|error| error.to_string())?
                == attention.len(),
        "native V layout/layer count changed",
    )?;
    let mut components = Vec::new();
    for &block in &attention {
        require(
            reader.u32()? == 1 && reader.u64()? == width * 2,
            "native K row type/extent changed",
        )?;
        for token in 0..count {
            components.push(json!({"path":format!("block-{block}-key-{token}.f32le"),"offset":reader.cursor,"elements":width,"code":"f16","stride":2}));
            reader.skip(width * 2)?;
        }
    }
    for &block in &attention {
        require(reader.u32()? == 1, "native V is not binary16")?;
        if transposed == 1 {
            require(
                reader.u32()? == 2 && u64::from(reader.u32()?) == width,
                "native transposed V geometry changed",
            )?;
            for token in 0..count {
                components.push(json!({"path":format!("block-{block}-value-{token}.f32le"),"offset":u64::try_from(reader.cursor).map_err(|error|error.to_string())?+token*2,"elements":width,"code":"f16","stride":count*2}));
            }
            reader.skip(width * count * 2)?;
        } else {
            require(reader.u64()? == width * 2, "native V row size changed")?;
            for token in 0..count {
                components.push(json!({"path":format!("block-{block}-value-{token}.f32le"),"offset":reader.cursor,"elements":width,"code":"f16","stride":2}));
                reader.skip(width * 2)?;
            }
        }
    }
    require(
        reader.u32()? == 1
            && u64::from(reader.u32()?) == position
            && reader.u32()? == 0
            && reader.u32()? == 0
            && u64::from(reader.u32()?) == layers,
        "native recurrent metadata/schema changed",
    )?;
    for (name, elements) in [("convolution", convolution), ("recurrent", matrix)] {
        for &block in &recurrent {
            require(
                reader.u32()? == 0 && reader.u64()? == elements * 4,
                "native recurrent payload type/extent changed",
            )?;
            components.push(json!({"path":format!("block-{block}-{name}.f32le"),"offset":reader.cursor,"elements":elements,"code":"f32","stride":4}));
            reader.skip(elements * 4)?;
        }
    }
    require(
        reader.cursor == raw.len(),
        "native sequence state truncated or has trailing bytes",
    )?;
    Ok(components.into())
}

pub(super) fn controls(
    rust: &Path,
    capture: &Value,
    backend: &str,
    workers: u64,
) -> crate::Result<Value> {
    require(
        ["auto", "scalar", "neon"].contains(&backend) && [1, 4].contains(&workers),
        "unsupported Rust backend/row-worker policy",
    )?;
    let stdout = read(&rust.join("stdout.txt"), 65_536)?;
    let text = std::str::from_utf8(&stdout).map_err(|error| error.to_string())?;
    let records = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| super::decode(line.as_bytes()))
        .collect::<crate::Result<Vec<_>>>()?;
    let events = |name: &str| {
        records
            .iter()
            .filter(|r| r["event"] == name)
            .collect::<Vec<_>>()
    };
    let weights = events("weights-loaded");
    let steps = events("forward-step");
    let adapters = events("rank-one-adapter-loaded");
    require(
        events("error").is_empty()
            && weights.len() == 1
            && weights[0]["model_sha256"] == MODEL
            && weights[0]["weight_representation"] == "current-repacked-PTQ1_0",
        "Rust capture used a different artifact",
    )?;
    require(
        steps.len() == 1
            && integer(steps[0], "position")? == 0
            && integer(steps[0], "input_token")? == 248_045
            && steps[0]["backend_requested"] == backend,
        "Rust supplied tokens/backend differ",
    )?;
    let step = steps[0];
    let operations = integer(step, "parallel_row_operations")?;
    require(
        integer(step, "row_workers")? == workers
            && operations <= i64::MAX.unsigned_abs()
            && ((workers == 4 && operations > 0) || (workers == 1 && operations == 0)),
        "Rust row-worker execution differs",
    )?;
    if capture["adapterLoaded"] == true {
        let scale = capture["adapterScale"]
            .as_f64()
            .ok_or("invalid native scale")?;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "The admitted scale is exactly zero or 1.3 and the binary32 identity is part of the capture contract."
        )]
        let bits = format!("{:08x}", (scale as f32).to_bits());
        require(
            adapters.len() == 1
                && adapters[0]["adapter_sha256"] == ADAPTER
                && integer(adapters[0], "target_pairs")? == 129
                && adapters[0]["scale_bits"] == bits
                && adapters[0]["model_sha256"] == MODEL,
            "Rust/native adapter identity/scale differs",
        )?;
    } else {
        require(
            adapters.is_empty() && capture["adapterScale"].as_f64() == Some(0.0),
            "native/Rust baseline adapter differs",
        )?;
    }
    Ok(step.clone())
}

pub(super) fn adapter_condition(capture: &Value, condition: &str) -> crate::Result<Value> {
    require(
        ["standard", "loaded-zero", "no-adapter"].contains(&condition),
        "unsupported adapter condition",
    )?;
    let expected = if condition == "standard" {
        1.3_f64
    } else {
        0.0_f64
    };
    require(
        capture["adapterLoaded"].as_bool() == Some(condition != "no-adapter")
            && capture["adapterScale"]
                .as_f64()
                .is_some_and(|value| value.to_bits() == expected.to_bits()),
        "native adapter condition differs",
    )?;
    Ok(json!({"validated":true}))
}

#[expect(
    clippy::too_many_lines,
    reason = "The historical comparator receipt binds complete state and every explicit fixed execution control."
)]
pub(super) fn compare(request: &Value) -> crate::Result<Value> {
    let native = path(request, "native")?;
    let rust = path(request, "rust")?;
    let backend = text(request, "backend")?;
    let workers = request
        .get("rowWorkers")
        .map_or(Ok(1), |_| integer(request, "rowWorkers"))?;
    let condition = request
        .get("adapterCondition")
        .map_or(Ok("standard"), |_| text(request, "adapterCondition"))?;
    require(
        inventory(&native, true)?
            == BTreeSet::from(["logits-0.f32le".into(), "state-0.native-seq".into()]),
        "native payload inventory changed",
    )?;
    let capture = read_json(&native.join("native-capture.json"), 65_536)?;
    require(
        capture["nativeCommit"] == REVISION
            && capture["modelSha256"] == MODEL
            && integer(&capture, "contextTokens")? == 8192
            && integer(&capture, "threads")? == 4,
        "native capture identity/controls changed",
    )?;
    let steps = capture["steps"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or("native trajectory/logits geometry changed")?;
    let step = &steps[0];
    require(
        integer(step, "inputToken")? == 248_045
            && integer(step, "position")? == 0
            && integer(step, "logitsCount")? == 248_320,
        "native trajectory/logits geometry changed",
    )?;
    adapter_condition(&capture, condition)?;
    let rust_step = controls(&rust, &capture, backend, workers)?;
    let mut comparisons = Vec::new();
    let mut expected = BTreeSet::new();
    let logits = native.join("logits-0.f32le");
    let actual = rust.join("logits-0.f32le");
    let expected_logits = read(&logits, 248_320 * 4)?;
    require(
        expected_logits.len() == 248_320 * 4,
        "native logits extent changed",
    )?;
    let mut compared = compare_bytes(
        &expected_logits,
        &read(&actual, 248_320 * 4)?,
        &json!({"elements":248_320,"offset":0,"stride":4,"code":"f32"}),
        0.001,
        0.001,
    )?;
    compared["path"] = "logits-0.f32le".into();
    compared["nativeSha256"] = digest(&logits)?.into();
    compared["rustSha256"] = digest(&actual)?.into();
    comparisons.push(compared);
    expected.insert("logits-0.f32le".to_owned());
    let state_path = native.join("state-0.native-seq");
    let state = read(&state_path, 536_870_912)?;
    require(
        u64::try_from(state.len()).map_err(|error| error.to_string())?
            == integer(step, "sequenceStateBytes")?,
        "native serialized state receipt extent changed",
    )?;
    for component in components(&state_path, 0, &json!({}))?
        .as_array()
        .ok_or("invalid decoded components")?
    {
        let relative = format!("state-0/{}", text(component, "path")?);
        let actual = rust.join(&relative);
        let mut metric = compare_bytes(
            &state,
            &read(&actual, 128 * 1024 * 1024)?,
            component,
            0.001,
            0.001,
        )?;
        metric["path"] = relative.clone().into();
        metric["rustSha256"] = digest(&actual)?.into();
        comparisons.push(metric);
        expected.insert(relative);
    }
    require(
        expected == inventory(&rust, false)?,
        "Rust complete-state inventory differs from exact native extent",
    )?;
    let failed = comparisons
        .iter()
        .map(|v| integer(v, "failedElements"))
        .collect::<crate::Result<Vec<_>>>()?
        .iter()
        .sum::<u64>();
    let elements = comparisons
        .iter()
        .map(|v| integer(v, "elements"))
        .collect::<crate::Result<Vec<_>>>()?
        .iter()
        .sum::<u64>();
    let maximum = comparisons
        .iter()
        .filter_map(|v| v["maximumAbsoluteError"].as_f64())
        .fold(0.0_f64, f64::max);
    let greedy = integer(&rust_step, "greedy_token")? == integer(step, "greedyToken")?;
    Ok(
        json!({"schemaVersion":1,"status":if failed > 0 || !greedy {"failed"} else {"passed-bounded-one-token-smoke"},"nativeCommit":REVISION,"modelSha256":MODEL,"absoluteTolerance":0.001,"relativeTolerance":0.001,"files":comparisons.len(),"elements":elements,"failedElements":failed,"byteExactFiles":comparisons.iter().filter(|v|v["byteExact"] == true).count(),"nativeCaptureSha256":digest(&native.join("native-capture.json"))?,"nativeStateSha256":[digest(&state_path)?],"maximumAbsoluteError":maximum,"comparisons":comparisons,"backendRequested":backend,"rustRowWorkers":workers,"greedyTokensMatch":greedy,"adapterCondition":condition,"adapterLoaded":capture["adapterLoaded"],"adapterScale":capture["adapterScale"],"adapterSha256":if condition == "no-adapter" {Value::Null} else {ADAPTER.into()},"parallelRowOperations":[rust_step["parallel_row_operations"]],"rustStdoutSha256":digest(&rust.join("stdout.txt"))?,"writerBasisQualified":false,"qualityQualified":false,"defaultMayChange":false,"loadedZeroIdentityQualified":false,"nonzeroApplicationQualified":false,"pendingControls":["no-adapter","loaded-zero-identity","nonzero-change","scalar","explicit-neon","writer-authoring-basis","quality","performance","full-context"]}),
    )
}
