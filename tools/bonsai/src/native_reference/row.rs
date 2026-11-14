//! Fixed serial/four-worker original-PQ2 comparison controls.

use super::{ADAPTER, digest, float, integer, inventory, path, read, require};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

pub(super) fn model() -> crate::Result<String> {
    // The pinned assessment model, rather than PTQ1's distinct byte identity.
    let lock: Value = super::decode(include_bytes!(
        "../../../../config/swift-assessment.lock.json"
    ))?;
    lock["model"]["sha256"]
        .as_str()
        .filter(|hash| crate::identity::is_lower_hex(hash, 64))
        .map(str::to_owned)
        .ok_or_else(|| "invalid compiled PQ2 model pin".into())
}

pub(super) fn controls(
    directory: &Path,
    workers: u64,
    vocabulary: u64,
    model: &str,
) -> crate::Result<Value> {
    let raw = read(&directory.join("stdout.txt"), 65_536)?;
    let text = std::str::from_utf8(&raw).map_err(|error| error.to_string())?;
    let records = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| super::decode(line.as_bytes()))
        .collect::<crate::Result<Vec<_>>>()?;
    let events = |event: &str| {
        records
            .iter()
            .filter(|r| r["event"] == event)
            .collect::<Vec<_>>()
    };
    let weights = events("weights-loaded");
    let adapters = events("rank-one-adapter-loaded");
    let steps = events("forward-step");
    require(
        events("error").is_empty()
            && weights.len() == 1
            && weights[0]["model_sha256"] == model
            && weights[0]["weight_representation"] == "PQ2_0",
        "PQ2 model control changed",
    )?;
    require(
        adapters.len() == 1
            && adapters[0]["adapter_sha256"] == ADAPTER
            && adapters[0]["model_sha256"] == model
            && integer(adapters[0], "target_pairs")? == 129
            && adapters[0]["scale_bits"] == "3fa66666",
        "adapter control changed",
    )?;
    require(steps.len() == 1, "expected exactly one fixed forward step")?;
    let step = steps[0];
    require(
        integer(step, "position")? == 0
            && integer(step, "input_token")? == 248_045
            && integer(step, "logit_count")? == vocabulary
            && step["backend_requested"] == "auto",
        "forward controls changed",
    )?;
    require(
        step["identity"]
            == format!(
                "{model}:qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1:rank-one-lora-v1:{ADAPTER}:3fa66666"
            ),
        "graph identity changed",
    )?;
    require(
        step["logits"].as_str().map(Path::new) == Some(directory.join("logits-0.f32le").as_path()),
        "emitted logits path changed",
    )?;
    require(
        (1..=12_348_030_976).contains(&integer(step, "checkpoint_bytes")?)
            && integer(step, "greedy_token")? < vocabulary
            && integer(step, "row_workers")? == workers,
        "forward controls changed",
    )?;
    let operations = integer(step, "parallel_row_operations")?;
    require(
        operations <= i64::MAX.unsigned_abs()
            && ((workers == 1 && operations == 0) || (workers == 4 && operations > 0)),
        "requested row dispatch was not observed",
    )?;
    let milliseconds = step["evaluation_milliseconds"]
        .as_f64()
        .ok_or("invalid fixed-step duration")?;
    require(
        milliseconds.is_finite() && milliseconds > 0.0 && milliseconds <= 240_000.0,
        "invalid fixed-step duration",
    )?;
    Ok(step.clone())
}

#[expect(
    clippy::too_many_lines,
    reason = "The fixed paired capture binds inventory, execution controls, every scalar bit and its non-qualification receipt."
)]
pub(super) fn compare(request: &Value) -> crate::Result<Value> {
    let serial = path(request, "serial")?;
    let parallel = path(request, "parallel")?;
    let geometry=request.get("geometry").cloned().unwrap_or_else(||json!({"layers":64,"width":1024,"convolution":30_720,"recurrent":786_432,"vocabulary":248_320}));
    let layers = integer(&geometry, "layers")?;
    let width = integer(&geometry, "width")?;
    let convolution = integer(&geometry, "convolution")?;
    let recurrent = integer(&geometry, "recurrent")?;
    let vocabulary = integer(&geometry, "vocabulary")?;
    require(
        (1..=64).contains(&layers)
            && [width, convolution, recurrent, vocabulary]
                .iter()
                .all(|n| (1..=1_000_000).contains(n)),
        "invalid row fixture geometry",
    )?;
    let model = model()?;
    let mut expected = vec![("logits-0.f32le".to_owned(), vocabulary)];
    for block in 0..layers {
        for (name, count) in if block % 4 == 3 {
            [("key-0", width), ("value-0", width)]
        } else {
            [("convolution", convolution), ("recurrent", recurrent)]
        } {
            expected.push((format!("state-0/block-{block}-{name}.f32le"), count));
        }
    }
    for directory in [&serial, &parallel] {
        require(
            inventory(directory, false)? == expected.iter().map(|(path, _)| path.clone()).collect(),
            "complete logical state inventory changed",
        )?;
        let state = directory.join("state-0");
        super::directory(&state)?;
        for (root, limit) in [(directory, 32), (&state, 128)] {
            let mut count = 0;
            for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
                count += 1;
                require(count <= limit, "capture directory entry bound exceeded")?;
                let entry = entry.map_err(|error| error.to_string())?;
                let metadata =
                    fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
                require(!super::redirected(&metadata), "reparse capture entry")?;
                if metadata.is_dir() {
                    require(
                        root == directory && entry.path() == state,
                        "unexpected nested capture directory",
                    )?;
                } else if root == &state {
                    require(
                        entry
                            .path()
                            .extension()
                            .is_some_and(|extension| extension == "f32le"),
                        "unexpected state component",
                    )?;
                }
            }
        }
    }
    let first = controls(&serial, 1, vocabulary, &model)?;
    let second = controls(&parallel, 4, vocabulary, &model)?;
    require(
        first["identity"] == second["identity"]
            && first["greedy_token"] == second["greedy_token"]
            && first["checkpoint_bytes"] == second["checkpoint_bytes"],
        "serial/parallel graph, greedy token or checkpoint extent changed",
    )?;
    let mut comparisons = Vec::new();
    let mut total = 0;
    for (relative, elements) in expected {
        let left = read(&serial.join(&relative), elements * 4)?;
        let right = read(&parallel.join(&relative), elements * 4)?;
        require(
            u64::try_from(left.len()).map_err(|error| error.to_string())? == elements * 4
                && left.len() == right.len(),
            "component extent changed",
        )?;
        let mut indices = [0_usize; 2];
        for (side, bytes) in [&left, &right].into_iter().enumerate() {
            let mut maximum = f32::NEG_INFINITY;
            for (index, chunk) in bytes.as_chunks::<4>().0.iter().enumerate() {
                let value = float(chunk);
                require(value.is_finite(), "nonfinite compared value")?;
                if value > maximum {
                    maximum = value;
                    indices[side] = index;
                }
            }
        }
        require(left == right, "serial/parallel arithmetic bits differ")?;
        if relative == "logits-0.f32le" {
            require(
                integer(&first, "greedy_token")?
                    == u64::try_from(indices[0]).map_err(|error| error.to_string())?
                    && integer(&second, "greedy_token")?
                        == u64::try_from(indices[1]).map_err(|error| error.to_string())?,
                "greedy token does not match compared logits",
            )?;
        }
        comparisons.push(
            json!({"path":relative,"elements":elements,"sha256":digest(&serial.join(&relative))?}),
        );
        total += elements;
    }
    let serial_ms = first["evaluation_milliseconds"]
        .as_f64()
        .ok_or("invalid serial duration")?;
    let parallel_ms = second["evaluation_milliseconds"]
        .as_f64()
        .ok_or("invalid parallel duration")?;
    Ok(
        json!({"status":"fixed-step-serial-parallel-byte-exact","modelSha256":model,"adapterSha256":ADAPTER,"adapterScale":1.3,"suppliedTokens":[248_045],"rowWorkers":[1,4],"backend":"auto","files":comparisons.len(),"elements":total,"serialEvaluationMilliseconds":first["evaluation_milliseconds"],"parallelEvaluationMilliseconds":second["evaluation_milliseconds"],"observedEvaluationSpeedup":serial_ms/parallel_ms,"parallelRowOperations":second["parallel_row_operations"],"timingScope":"One fixed forward evaluation per condition; excludes loading and capture I/O. No application TPS, statistical benefit, prefill or performance qualification.","performanceGatePassed":false,"defaultMayChange":false,"comparisons":comparisons}),
    )
}
