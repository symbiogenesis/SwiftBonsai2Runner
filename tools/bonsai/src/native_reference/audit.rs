//! Independent retained scalar replay; producer code and matrix reconstruction are never executed.

use super::public::{ARITHMETIC, NATIVE_SOURCES, ORIGINAL_SOURCES, blocks};
use super::{decode, digest, float, integer, path, read, read_json, read_match, require, text};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) fn unhex(text: &str) -> crate::Result<Vec<u8>> {
    require(text.len() % 2 == 0, "invalid retained byte encoding")?;
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or("invalid retained byte encoding")? * 16
                + digit(pair[1]).ok_or("invalid retained byte encoding")?)
        })
        .collect()
}

pub(super) fn vector(raw: &[u8], count: usize) -> crate::Result<Vec<f32>> {
    require(raw.len() == count * 4, "retained F32 vector extent differs")?;
    let values = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| float(chunk))
        .collect::<Vec<_>>();
    require(
        values.iter().all(|value| value.is_finite()),
        "nonfinite retained F32 vector",
    )?;
    Ok(values)
}

pub(super) fn metrics(
    actual: &[f32],
    reference: &[f32],
    absolute: f64,
    relative: f64,
) -> crate::Result<Value> {
    require(
        actual.len() == reference.len()
            && absolute.is_finite()
            && relative.is_finite()
            && absolute >= 0.0
            && relative >= 0.0,
        "comparison controls differ",
    )?;
    let mut failed = 0;
    let mut maximum = 0.0_f64;
    for (&observed, &expected) in actual.iter().zip(reference) {
        require(
            observed.is_finite() && expected.is_finite(),
            "nonfinite comparison value",
        )?;
        let difference = (f64::from(observed) - f64::from(expected)).abs();
        maximum = maximum.max(difference);
        failed += u64::from(difference > relative.mul_add(f64::from(expected).abs(), absolute));
    }
    Ok(
        json!({"comparedElements":reference.len(),"mismatchedElements":failed,"maximumAbsoluteDifference":maximum}),
    )
}

fn keys(value: &Value, wanted: &[&str]) -> crate::Result<()> {
    let object = value.as_object().ok_or("retained object schema differs")?;
    require(
        object.len() == wanted.len() && wanted.iter().all(|name| object.contains_key(*name)),
        "retained object schema differs",
    )
}

// This decoder deliberately has independent descriptor/storage policy from the
// producer's formats::adapter. Only generic JSON and hash primitives are shared.
struct AuditReader<'a> {
    raw: &'a [u8],
    position: usize,
}
impl<'a> AuditReader<'a> {
    fn bytes(&mut self, count: usize) -> crate::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or("retained adapter extent overflow")?;
        let bytes = self
            .raw
            .get(self.position..end)
            .ok_or("truncated retained adapter")?;
        self.position = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> crate::Result<u32> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?
                .try_into()
                .map_err(|_| "truncated retained integer")?,
        ))
    }
    fn u64(&mut self) -> crate::Result<u64> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?
                .try_into()
                .map_err(|_| "truncated retained integer")?,
        ))
    }
    fn string(&mut self) -> crate::Result<String> {
        let length = self.u64()?;
        require(length <= 4096, "retained adapter text exceeds bound")?;
        String::from_utf8(
            self.bytes(usize::try_from(length).map_err(|error| error.to_string())?)?
                .to_vec(),
        )
        .map_err(|error| error.to_string())
    }
    fn metadata(&mut self, kind: u32, depth: u32) -> crate::Result<Value> {
        require(depth <= 2, "retained adapter nesting exceeds bound")?;
        Ok(match kind {
            0 => u64::from(self.bytes(1)?[0]).into(),
            1 => i64::from(i8::from_le_bytes([self.bytes(1)?[0]])).into(),
            2 => u64::from(u16::from_le_bytes(
                self.bytes(2)?
                    .try_into()
                    .map_err(|_| "truncated retained integer")?,
            ))
            .into(),
            3 => i64::from(i16::from_le_bytes(
                self.bytes(2)?
                    .try_into()
                    .map_err(|_| "truncated retained integer")?,
            ))
            .into(),
            4 => self.u32()?.into(),
            5 => i32::from_le_bytes(
                self.bytes(4)?
                    .try_into()
                    .map_err(|_| "truncated retained integer")?,
            )
            .into(),
            6 => {
                let value = f32::from_bits(self.u32()?);
                require(value.is_finite(), "nonfinite retained metadata")?;
                Value::from(value)
            }
            7 => {
                let value = self.bytes(1)?[0];
                require(value <= 1, "invalid retained adapter Boolean")?;
                (value != 0).into()
            }
            8 => self.string()?.into(),
            9 => {
                let kind = self.u32()?;
                let count = self.u64()?;
                require(count <= 65_536, "retained metadata count differs")?;
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(self.metadata(kind, depth + 1)?);
                }
                values.into()
            }
            10 => self.u64()?.into(),
            11 => i64::from_le_bytes(
                self.bytes(8)?
                    .try_into()
                    .map_err(|_| "truncated retained integer")?,
            )
            .into(),
            12 => {
                let value = f64::from_bits(self.u64()?);
                require(value.is_finite(), "nonfinite retained metadata")?;
                value.into()
            }
            _ => return Err("unknown retained metadata type".into()),
        })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Independent fixed GGUF reader keeps its complete descriptor and factor-extent contract together."
)]
pub(super) fn adapter(raw: &[u8]) -> crate::Result<BTreeMap<String, Vec<u8>>> {
    let mut reader = AuditReader { raw, position: 0 };
    require(
        reader.bytes(4)? == b"GGUF" && reader.u32()? == 3,
        "retained adapter format differs",
    )?;
    let count = reader.u64()?;
    let metadata_count = reader.u64()?;
    require(
        count == 258 && metadata_count <= 128,
        "retained adapter inventory differs",
    )?;
    let mut metadata = BTreeMap::new();
    for _ in 0..metadata_count {
        let name = reader.string()?;
        let kind = reader.u32()?;
        let value = reader.metadata(kind, 0)?;
        require(
            metadata.insert(name, value).is_none(),
            "duplicate retained adapter metadata",
        )?;
    }
    let field = |name: &str| metadata.get(name).unwrap_or(&Value::Null);
    require(
        (field("general.alignment").is_null() || field("general.alignment") == 32)
            && field("general.architecture") == "qwen35"
            && field("general.type") == "adapter"
            && field("adapter.type") == "lora"
            && field("adapter.lora_alpha").as_f64() == Some(1.0),
        "retained adapter metadata differs",
    )?;
    let mut descriptors = BTreeMap::new();
    for _ in 0..count {
        let name = reader.string()?;
        require(
            reader.u32()? == 2,
            "retained adapter tensor name/rank differs",
        )?;
        let shape = [reader.u64()?, reader.u64()?];
        require(
            shape.iter().all(|n| (1..=248_320).contains(n)) && reader.u32()? == 0,
            "retained adapter tensor shape/type differs",
        )?;
        let offset = reader.u64()?;
        require(
            offset % 32 == 0,
            "retained adapter tensor alignment differs",
        )?;
        require(
            descriptors.insert(name, (shape, offset)).is_none(),
            "duplicate retained adapter tensor",
        )?;
    }
    let mut expected = BTreeMap::from([
        ("token_embd.weight.lora_a".to_owned(), [1_u64, 248_320]),
        ("token_embd.weight.lora_b".to_owned(), [1, 5120]),
    ]);
    for block in 0..64 {
        let family = if block % 4 == 3 {
            "attn_output"
        } else {
            "ssm_out"
        };
        for (site, count) in [
            (format!("blk.{block}.ffn_down.weight"), 17_408),
            (format!("blk.{block}.{family}.weight"), 6144),
        ] {
            expected.insert(site.clone() + ".lora_a", [count, 1]);
            expected.insert(site + ".lora_b", [1, 5120]);
        }
    }
    require(
        descriptors.len() == expected.len(),
        "retained adapter writer set differs",
    )?;
    let base = reader
        .position
        .checked_add(31)
        .ok_or("retained adapter base overflow")?
        & !31;
    let mut factors = BTreeMap::new();
    let mut ranges = Vec::new();
    for (name, (shape, offset)) in descriptors {
        require(
            expected.get(&name) == Some(&shape),
            "retained adapter writer shape differs",
        )?;
        let begin = base
            .checked_add(usize::try_from(offset).map_err(|error| error.to_string())?)
            .ok_or("retained adapter extent overflow")?;
        let count = usize::try_from(shape[0] * shape[1]).map_err(|error| error.to_string())?;
        let end = begin
            .checked_add(count * 4)
            .ok_or("retained adapter extent overflow")?;
        let bytes = raw
            .get(begin..end)
            .ok_or("retained adapter writer extent differs")?
            .to_vec();
        vector(&bytes, count)?;
        factors.insert(name, bytes);
        ranges.push((begin, end));
    }
    ranges.sort_unstable();
    let mut cursor = base;
    for (begin, end) in ranges {
        require(begin >= cursor, "retained adapter writers overlap")?;
        cursor = end;
    }
    Ok(factors)
}

pub(super) fn descriptors(evidence: &Path, report: &Value, pins: &Value) -> crate::Result<Value> {
    decode(&read_match(
        &evidence.join("config.json"),
        integer(&pins["config"], "bytes")?,
        Some(integer(&pins["config"], "bytes")?),
        Some(text(&pins["config"], "sha256")?),
    )?)?;
    require(
        report["modelHeaderFile"] == "model-header.json",
        "retained header filename differs",
    )?;
    let raw = read_match(
        &evidence.join("model-header.json"),
        4_194_304,
        Some(integer(report, "modelHeaderBytes")?),
        Some(text(report, "modelHeaderSha256")?),
    )?;
    let mut header = decode(&raw)?;
    let object = header
        .as_object_mut()
        .ok_or("invalid retained model header")?;
    object.remove("__metadata__");
    require(
        u64::try_from(object.len()).map_err(|error| error.to_string())?
            == integer(report, "modelTensorCount")?
            && !object.is_empty()
            && object.len() <= 8192,
        "retained model tensor inventory differs",
    )?;
    let mut descriptors = serde_json::Map::new();
    let mut ranges = Vec::new();
    let base = u64::try_from(raw.len()).map_err(|error| error.to_string())? + 8;
    let file_bytes = integer(&pins["model"], "bytes")?;
    for (name, item) in object {
        keys(item, &["dtype", "shape", "data_offsets"])?;
        let width = match text(item, "dtype")? {
            "BOOL" | "U8" | "I8" => 1,
            "U16" | "I16" | "F16" | "BF16" => 2,
            "U32" | "I32" | "F32" => 4,
            "U64" | "I64" | "F64" => 8,
            _ => return Err("retained model descriptor dtype differs".into()),
        };
        let shape = item["shape"]
            .as_array()
            .filter(|a| a.len() <= 8)
            .ok_or("retained model descriptor shape differs")?;
        let dimensions = shape
            .iter()
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n <= 1_000_000_000)
                    .ok_or("retained model shape differs")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let product = if dimensions.contains(&0) {
            0
        } else {
            dimensions.iter().try_fold(1_u64, |product, &n| {
                product
                    .checked_mul(n)
                    .ok_or("retained model shape overflow")
            })?
        };
        let offsets = item["data_offsets"]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("retained model descriptor offsets differ")?;
        let begin = offsets[0].as_u64().ok_or("invalid retained offset")?;
        let end = offsets[1].as_u64().ok_or("invalid retained offset")?;
        require(
            begin <= end
                && end <= file_bytes
                && end - begin
                    == product
                        .checked_mul(width)
                        .ok_or("retained descriptor byte overflow")?
                && base.checked_add(end).is_some_and(|n| n <= file_bytes),
            "retained model descriptor extent differs",
        )?;
        descriptors.insert(name.clone(),json!({"dtype":item["dtype"],"shape":shape,"absoluteByteBegin":base+begin,"absoluteByteEnd":base+end}));
        if begin != end {
            ranges.push((begin, end));
        }
    }
    ranges.sort_unstable();
    let mut cursor = 0;
    for (begin, end) in ranges {
        require(begin == cursor, "retained model payload hole or overlap")?;
        cursor = end;
    }
    require(
        base + cursor == file_bytes,
        "retained model payload tail differs",
    )?;
    Ok(Value::Object(descriptors))
}

pub(super) fn site_descriptor(descriptor: &Value, suffix: &str) -> crate::Result<()> {
    let (shape, types) = match suffix {
        "weight" => (json!([5120, 384]), &["U32"][..]),
        "scales" | "biases" => (json!([5120, 48]), &["F16"][..]),
        "signs" => (json!([6144]), &["F16", "F32"][..]),
        _ => return Err("unknown source tensor family".into()),
    };
    require(
        descriptor["shape"] == shape
            && descriptor["dtype"]
                .as_str()
                .is_some_and(|kind| types.contains(&kind)),
        "retained source tensor descriptor differs",
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "A sealed source inventory must be validated in full before any archived bytes become replay inputs."
)]
pub(super) fn sources(
    evidence: &Path,
    report: &Value,
    revision: &str,
) -> crate::Result<Option<BTreeMap<String, Vec<u8>>>> {
    let have_file = report.get("replaySourceManifestFile").is_some();
    let have_hash = report.get("replaySourceManifestSha256").is_some();
    require(
        have_file == have_hash,
        "incomplete retained source manifest binding",
    )?;
    let bundle = evidence.join("replay-source");
    if !have_file {
        require(!bundle.exists(), "retained source binding removed")?;
        return Ok(None);
    }
    require(
        report["replaySourceManifestFile"] == "replay-source/source-manifest.json"
            && crate::identity::is_lower_hex(text(report, "replaySourceManifestSha256")?, 64),
        "retained source manifest selection differs",
    )?;
    super::directory(&bundle)?;
    let manifest = decode(&read_match(
        &bundle.join("source-manifest.json"),
        65_536,
        None,
        Some(text(report, "replaySourceManifestSha256")?),
    )?)?;
    keys(
        &manifest,
        &["schemaVersion", "kind", "sourceRevision", "files"],
    )?;
    let wanted = match integer(&manifest, "schemaVersion")? {
        1 => ORIGINAL_SOURCES.to_vec(),
        2 => ORIGINAL_SOURCES
            .iter()
            .chain(NATIVE_SOURCES)
            .copied()
            .collect(),
        _ => return Err("unsupported retained source schema".into()),
    };
    let files = manifest["files"]
        .as_array()
        .ok_or("retained source manifest identity differs")?;
    require(
        manifest["kind"] == "public-reference-replay-source"
            && manifest["sourceRevision"] == revision
            && files.len() == wanted.len(),
        "retained source manifest identity differs",
    )?;
    let mut expected = wanted
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<BTreeSet<_>>();
    expected.insert("source-manifest.json".into());
    let mut actual = BTreeSet::new();
    let mut pending = vec![(bundle.clone(), 0_u32)];
    while let Some((root, depth)) = pending.pop() {
        for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            require(
                !super::redirected(&metadata),
                "retained source entry redirected",
            )?;
            let relative = path
                .strip_prefix(&bundle)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.is_dir() {
                require(
                    depth < 6
                        && expected
                            .iter()
                            .any(|name| name.starts_with(&(relative.clone() + "/"))),
                    "unexpected retained source directory",
                )?;
                pending.push((path, depth + 1));
            } else {
                require(
                    metadata.is_file() && expected.contains(&relative) && actual.insert(relative),
                    "unexpected retained source entry",
                )?;
            }
        }
    }
    require(actual == expected, "retained source inventory incomplete")?;
    let mut sources = BTreeMap::new();
    for item in files {
        keys(item, &["path", "bytes", "sha256"])?;
        let name = text(item, "path")?;
        let count = integer(item, "bytes")?;
        let hash = text(item, "sha256")?;
        require(
            wanted.contains(&name)
                && !sources.contains_key(name)
                && (1..=1_048_576).contains(&count)
                && crate::identity::is_lower_hex(hash, 64),
            "retained source declaration differs",
        )?;
        sources.insert(
            name.to_owned(),
            read_match(&bundle.join(name), 1_048_576, Some(count), Some(hash))?,
        );
    }
    Ok(Some(sources))
}

#[expect(
    clippy::too_many_lines,
    reason = "The independent finite packet replay retains all original identity, source, descriptor, metric and no-promotion gates in review order."
)]
pub(super) fn replay(request: &Value) -> crate::Result<Value> {
    let evidence = path(request, "evidenceDirectory")?;
    let adapter_path = path(request, "adapter")?;
    let direction_path = path(request, "direction")?;
    let revision = text(request, "expectedSourceRevision")?;
    let run = text(request, "expectedRunId")?;
    let attempt = text(request, "expectedRunAttempt")?;
    let repository = path(request, "repository")?;
    let positive =
        |s: &str| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit());
    require(
        crate::identity::is_lower_hex(revision, 40) && positive(run) && positive(attempt),
        "independent producer selection invalid",
    )?;
    let report = read_json(&evidence.join("report.json"), 1_048_576)?;
    let retained = sources(&evidence, &report, revision)?;
    let source = |relative: &str| -> crate::Result<Vec<u8>> {
        retained.as_ref().map_or_else(
            || read(&repository.join(relative), 1_048_576),
            |sources| {
                sources
                    .get(relative)
                    .cloned()
                    .ok_or_else(|| "required replay source missing".into())
            },
        )
    };
    let pins_raw = source("tools/native-reference/public-authoring-pins.json")?;
    let pins = decode(&pins_raw)?;
    require(
        pins == read_json(
            &repository.join("tools/native-reference/public-authoring-pins.json"),
            65_536,
        )?,
        "reviewer pins differ from independently selected recipe",
    )?;
    for (name, key) in [
        ("swift-lora.lock.json", "adapter"),
        ("swift-lora.direction.json", "direction"),
    ] {
        let relative = format!("config/{name}");
        let lock = decode(&source(&relative)?)?;
        let approved = read_json(&repository.join(relative), 1_048_576)?;
        for field in ["filename", "bytes", "sha256", "url"] {
            require(
                pins[key][field] == lock[key][field] && lock[key][field] == approved[key][field],
                "reviewer pins differ from approved repository lock",
            )?;
        }
    }
    require(
        report == read_json(&evidence.join("attempt.json"), 1_048_576)?,
        "terminal attempt differs from report",
    )?;
    require(
        report["schemaVersion"] == 2
            && report["kind"] == "independent-public-pack-A-reference"
            && report["arithmeticId"] == ARITHMETIC
            && report["stage"] == "complete"
            && report["referencePinsSha256"] == crate::identity::sha256(&pins_raw)
            && report["inputPins"] == pins,
        "retained recipe/input identity differs",
    )?;
    require(
        report["repository"] == "symbiogenesis/SwiftBonsai2Runner"
            && report["workflowPath"] == ".github/workflows/ptq1-public-authoring-reference.yml"
            && report["sourceRevision"] == revision
            && report["runId"] == run
            && report["runAttempt"] == attempt
            && report["runnerEnvironment"] == "github-hosted",
        "retained producer differs from independent selection",
    )?;
    for flag in [
        "authorOriginalProvenanceQualified",
        "nativeWriterBasisQualified",
        "logitsOrStateQualified",
        "defaultMayChange",
        "fullCheckpointWritten",
    ] {
        require(
            report[flag] == false,
            "retained result overstates qualification",
        )?;
    }
    require(
        report["ownerDeviceTrials"] == 0 && report["modelInferenceCommands"] == 0,
        "retained execution scope differs",
    )?;
    let toolchain = read_json(&evidence.join("toolchain.json"), 1_048_576)?;
    for (field, name) in [
        ("referenceScriptSha256", "public_pack_reference.ps1"),
        ("formatScriptSha256", "public_pack_formats.ps1"),
        ("numericSourceSha256", "reference_numerics.rs"),
    ] {
        require(
            toolchain[field]
                == crate::identity::sha256(&source(&format!("tools/native-reference/{name}"))?),
            "retained producer source differs from reviewer source",
        )?;
    }
    if toolchain["producerImplementation"] == "native-rust-v1" {
        require(
            retained
                .as_ref()
                .is_some_and(|s| NATIVE_SOURCES.iter().all(|name| s.contains_key(*name)))
                && crate::identity::is_lower_hex(text(&toolchain, "policyExecutableSha256")?, 64),
            "native producer source/executable binding missing",
        )?;
    } else {
        let version = text(&toolchain, "powershell")?
            .split('.')
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        require(
            (2..=4).contains(&version.len()) && version.iter().all(|part|*part<=i32::MAX.unsigned_abs())
                && version.as_slice() >= [7, 6].as_slice(),
            "retained producer PowerShell differs",
        )?;
    }
    require(
        toolchain["threads"] == 1
            && toolchain["reduction"] == pins["reduction"]
            && toolchain["normalization"] == pins["normalization"]
            && crate::identity::is_lower_hex(text(&toolchain, "numericExecutableSha256")?, 64)
            && text(&toolchain, "rust")?.starts_with("rustc "),
        "retained producer toolchain differs",
    )?;
    let descriptors = descriptors(&evidence, &report, &pins)?;
    let factors = adapter(&read_match(
        &adapter_path,
        integer(&pins["adapter"], "bytes")?,
        Some(integer(&pins["adapter"], "bytes")?),
        Some(text(&pins["adapter"], "sha256")?),
    )?)?;
    let raw_direction = read_match(
        &direction_path,
        20_480,
        Some(20_480),
        Some(text(&pins["direction"], "sha256")?),
    )?;
    let direction = vector(&raw_direction, 5120)?;
    #[allow(
        clippy::suboptimal_flops,
        reason = "Independent scalar-v2 normalization requires sequential separate multiply/add rounding."
    )]
    let sum = direction
        .iter()
        .fold(0.0, |sum, &value| sum + f64::from(value) * f64::from(value));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "The recorded normalization explicitly rounds a finite f64 square root to binary32."
    )]
    let expected_norm = sum.sqrt() as f32;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "The report norm is compared at its specified binary32 rounding."
    )]
    let norm = report["directionF32NormBeforeNormalization"]
        .as_f64()
        .ok_or("invalid direction norm")? as f32;
    require(
        norm.is_finite()
            && norm > 0.0
            && (f64::from(norm) - 1.0).abs() <= 0.00001
            && norm.to_bits() == expected_norm.to_bits(),
        "retained direction norm inconsistent",
    )?;
    let normalized = direction.iter().map(|v| *v / norm).collect::<Vec<_>>();
    let negative = normalized.iter().map(|v| -*v).collect::<Vec<_>>();
    let normalized_raw = normalized
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    require(
        crate::identity::sha256(&normalized_raw) == text(&report, "normalizedDirectionSha256")?,
        "retained normalized direction digest differs",
    )?;
    let b_report = read_json(&evidence.join("direction-comparison.json"), 1_048_576)?;
    let b_items = b_report["sites"]
        .as_array()
        .filter(|a| a.len() == 129)
        .ok_or("retained B comparison inventory differs")?;
    let expected_b = factors
        .keys()
        .filter(|name| name.ends_with(".lora_b"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut seen_b = BTreeSet::new();
    for item in b_items {
        let name = text(item, "tensor")?;
        require(
            expected_b.contains(name) && seen_b.insert(name),
            "retained B comparison inventory differs",
        )?;
        let raw = &factors[name];
        let metric = metrics(&vector(raw, 5120)?, &negative, 0.000_001, 0.000_001)?;
        require(
            item["sha256"] == crate::identity::sha256(raw) && metric["mismatchedElements"] == 0,
            "retained B comparison replay differs",
        )?;
        check_metrics(item, &metric)?;
    }
    require(
        report["all129BComparisonsPassed"] == true,
        "retained B verdict differs",
    )?;
    let sites = report["sites"]
        .as_array()
        .filter(|a| a.len() == 48)
        .ok_or("retained A comparison inventory differs")?;
    let expected_files = blocks()
        .map(|block| format!("ssm-{block:02}.f32le"))
        .collect::<BTreeSet<_>>();
    let actual_files = fs::read_dir(&evidence)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map_err(|error| error.to_string()))
        .collect::<crate::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with("ssm-")
                && Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension == "f32le")
        })
        .collect::<BTreeSet<_>>();
    require(
        actual_files == expected_files,
        "retained A file inventory differs",
    )?;
    let mut results = Vec::new();
    for (index, block) in blocks().enumerate() {
        let item = &sites[index];
        let name = format!("ssm-{block:02}.f32le");
        let prefix = format!("language_model.model.layers.{block}.linear_attn.out_proj");
        require(
            item["block"] == block && item["publicTensorPrefix"] == prefix,
            "retained A site order differs",
        )?;
        keys(
            &item["inputTensorDescriptors"],
            &["weight", "scales", "biases", "signs"],
        )?;
        keys(
            &item["inputTensorSha256"],
            &["weight", "scales", "biases", "signs"],
        )?;
        for suffix in ["weight", "scales", "biases", "signs"] {
            let descriptor = &descriptors[format!("{prefix}.{suffix}")];
            site_descriptor(descriptor, suffix)?;
            require(
                *descriptor == item["inputTensorDescriptors"][suffix]
                    && crate::identity::is_lower_hex(text(&item["inputTensorSha256"], suffix)?, 64),
                "retained source tensor descriptor differs",
            )?;
        }
        require(
            crate::identity::is_lower_hex(text(item, "candidateF16MatrixSha256")?, 64)
                && item["authoringAFile"] == name
                && item["authoringABytes"] == 24_576,
            "retained candidate matrix/A extent differs",
        )?;
        let reference = read_match(
            &evidence.join(name),
            24_576,
            Some(24_576),
            Some(text(item, "authoringASha256")?),
        )?;
        let shipped = &factors[&format!("blk.{block}.ssm_out.weight.lora_a")];
        let mut metric = metrics(
            &vector(shipped, 6144)?,
            &vector(&reference, 6144)?,
            0.00001,
            0.00001,
        )?;
        require(
            item["shippedASha256"] == crate::identity::sha256(shipped),
            "retained shipped A digest differs",
        )?;
        check_metrics(item, &metric)?;
        metric["block"] = block.into();
        metric["authoringASha256"] = crate::identity::sha256(&reference).into();
        results.push(metric);
    }
    let passed = results.iter().all(|r| r["mismatchedElements"] == 0);
    let status = if passed {
        "complete-public-reference-match"
    } else {
        "rejected-public-A-comparison"
    };
    require(
        report["all48PublicAComparisonPassed"].as_bool() == Some(passed)
            && report["status"] == status,
        "retained A verdict differs",
    )?;
    Ok(
        json!({"schemaVersion":2,"kind":"offline-retained-public-reference-replay","status":"retained-result-verified","sourceRevision":revision,"runId":run,"runAttempt":attempt,"reportSha256":digest(&evidence.join("report.json"))?,"referencePinsSha256":crate::identity::sha256(&pins_raw),"replaySourceManifestSha256":report.get("replaySourceManifestSha256").unwrap_or(&Value::Null),"retainedReplaySourceVerified":retained.is_some(),"reviewScriptSha256":digest(&repository.join("tools/native-reference/audit_public_reference.ps1"))?,"reviewImplementation":"native-rust-v1","reviewExecutableSha256":digest(&std::env::current_exe().map_err(|error|error.to_string())?)?,"adapterSha256":pins["adapter"]["sha256"],"directionSha256":pins["direction"]["sha256"],"all48PublicAComparisonPassed":passed,"all129BComparisonsPassed":true,"sites":results,"producerAuthenticated":false,"matrixReconstructionReexecuted":false,"authorOriginalProvenanceQualified":false,"nativeWriterBasisQualified":false,"defaultMayChange":false,"modelInferenceCommands":0,"scope":"Independent retained-file and scalar A/B metric replay only. Authenticate the producer and artifact separately; no matrix reconstruction, provenance, writer-basis, logits/state, quality or promotion qualification."}),
    )
}

fn check_metrics(item: &Value, metric: &Value) -> crate::Result<()> {
    for field in [
        "comparedElements",
        "mismatchedElements",
        "maximumAbsoluteDifference",
    ] {
        require(
            item[field].as_f64() == metric[field].as_f64(),
            "retained comparison metrics differ",
        )?;
    }
    Ok(())
}
