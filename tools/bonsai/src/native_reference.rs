//! Bounded native-reference format admission and complete-state comparisons.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::Deserialize as _;
use serde_json::{Value, json};

mod audit;
mod formats;
mod native;
mod public;
mod retained;
mod row;

const MODEL: &str = "e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5";
const ADAPTER: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";

fn require(condition: bool, message: &str) -> crate::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

// serde_json::Value alone accepts duplicate keys. Every reference intake uses this
// visitor before deserialization so an identity cannot depend on key precedence.
struct StrictJson(Value);
impl<'de> serde::Deserialize<'de> for StrictJson {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictJson;
            fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                out.write_str("unambiguous finite JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| StrictJson(n.into()))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictJson(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(StrictJson(values.into()))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut entries: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, StrictJson(value))) =
                    entries.next_entry::<String, StrictJson>()?
                {
                    if values.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(StrictJson(Value::Object(values)))
            }
        }
        decoder.deserialize_any(Visitor)
    }
}

fn decode(bytes: &[u8]) -> crate::Result<Value> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let StrictJson(value) =
        StrictJson::deserialize(&mut decoder).map_err(|error| error.to_string())?;
    decoder.end().map_err(|error| error.to_string())?;
    Ok(value)
}

fn read(path: &Path, maximum: u64) -> crate::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    require(
        metadata.is_file() && !redirected(&metadata),
        "redirected or non-file reference input",
    )?;
    require(
        metadata.len() <= maximum,
        "reference input exceeds byte bound",
    )?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    require(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? <= maximum,
        "reference input grew beyond byte bound",
    )?;
    Ok(bytes)
}

fn read_json(path: &Path, maximum: u64) -> crate::Result<Value> {
    decode(&read(path, maximum)?)
}

fn read_match(
    path: &Path,
    maximum: u64,
    exact: Option<u64>,
    sha256: Option<&str>,
) -> crate::Result<Vec<u8>> {
    let bytes = read(path, maximum)?;
    require(
        exact.is_none_or(|n| u64::try_from(bytes.len()).ok() == Some(n)),
        "retained file byte extent differs",
    )?;
    require(
        sha256.is_none_or(|hash| crate::identity::sha256(&bytes) == hash),
        "retained file digest differs",
    )?;
    Ok(bytes)
}

fn write_json(path: &Path, value: &Value) -> crate::Result<()> {
    let temporary = path.with_extension(format!("{}.partial", crate::startup_process::unique()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn digest(path: &Path) -> crate::Result<String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    digest_reader(&mut file, &mut || Ok(()))
}

fn digest_reader(
    file: &mut File,
    checkpoint: &mut dyn FnMut() -> crate::Result<()>,
) -> crate::Result<String> {
    let mut hash = crate::identity::Hasher::new()?;
    let mut buffer = vec![0_u8; 1_048_576];
    loop {
        checkpoint()?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count])?;
    }
    hash.finalize_hex()
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn directory(path: &Path) -> crate::Result<()> {
    require(path.is_absolute(), "reference directory must be absolute")?;
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?;
        require(
            metadata.is_dir() && !redirected(&metadata),
            "reference directory ancestry is redirected",
        )?;
    }
    Ok(())
}

fn inventory(root: &Path, native: bool) -> crate::Result<BTreeSet<String>> {
    directory(root)?;
    let mut paths = BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), 0_u32)];
    let mut count = 0;
    while let Some((path, depth)) = pending.pop() {
        for item in fs::read_dir(path).map_err(|error| error.to_string())? {
            count += 1;
            require(count <= 512, "reference directory inventory exceeds bound")?;
            let path = item.map_err(|error| error.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            require(!redirected(&metadata), "redirected reference entry")?;
            if metadata.is_dir() {
                require(depth < 2, "reference directory depth exceeds bound")?;
                pending.push((path, depth + 1));
            } else {
                require(metadata.is_file(), "unsupported reference entry")?;
                if path.extension().is_some_and(|extension| {
                    extension == "f32le" || (native && extension == "native-seq")
                }) {
                    paths.insert(
                        path.strip_prefix(root)
                            .map_err(|error| error.to_string())?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    Ok(paths)
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing reference string: {key}"))
}
fn integer(value: &Value, key: &str) -> crate::Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("Missing reference integer: {key}"))
}
fn path(value: &Value, key: &str) -> crate::Result<PathBuf> {
    Ok(PathBuf::from(text(value, key)?))
}

const fn float(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
fn half(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (bits >> 10) & 31;
    let mantissa = u32::from(bits & 1023);
    match exponent {
        0 => {
            let value = f32::from(bits & 1023) * (1.0 / 16_777_216.0);
            if sign == 0 { value } else { -value }
        }
        31 => f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13)),
        _ => f32::from_bits(sign | (u32::from(exponent + 112) << 23) | (mantissa << 13)),
    }
}

fn compare_bytes(
    expected: &[u8],
    actual: &[u8],
    component: &Value,
    absolute: f64,
    relative: f64,
) -> crate::Result<Value> {
    let count =
        usize::try_from(integer(component, "elements")?).map_err(|error| error.to_string())?;
    let offset =
        usize::try_from(integer(component, "offset")?).map_err(|error| error.to_string())?;
    let stride =
        usize::try_from(integer(component, "stride")?).map_err(|error| error.to_string())?;
    let width = match text(component, "code")? {
        "f16" => 2,
        "f32" => 4,
        _ => return Err("unsupported reference scalar format".into()),
    };
    let end = count
        .checked_sub(1)
        .and_then(|n| n.checked_mul(stride))
        .and_then(|n| n.checked_add(width))
        .and_then(|n| n.checked_add(offset))
        .ok_or("reference extent overflow")?;
    require(
        count <= 32 * 1024 * 1024
            && stride >= width
            && stride <= 8
            && expected.len() >= end
            && actual.len() == count * 4,
        "reference component extent differs",
    )?;
    let mut failed = 0;
    let mut maximum = 0.0_f64;
    let mut exact = true;
    for index in 0..count {
        let cursor = offset + index * stride;
        let reference = if width == 2 {
            half(u16::from_le_bytes([expected[cursor], expected[cursor + 1]]))
        } else {
            float(&expected[cursor..])
        };
        let observed = float(&actual[index * 4..]);
        require(
            reference.is_finite() && observed.is_finite(),
            "nonfinite compared value",
        )?;
        let difference = (f64::from(observed) - f64::from(reference)).abs();
        maximum = maximum.max(difference);
        exact &= reference.to_bits() == observed.to_bits();
        #[allow(
            clippy::suboptimal_flops,
            reason = "Historical native receipts specify separate tolerance multiply/add rounding."
        )]
        let tolerance = absolute + relative * f64::from(reference).abs();
        failed += u64::from(difference > tolerance);
    }
    Ok(
        json!({"elements": count, "failedElements": failed, "maximumAbsoluteError": maximum, "byteExact": exact}),
    )
}

/// Execute a model-free bounded reference operation.
///
/// # Errors
/// Rejects malformed binary formats, ambiguous JSON, changed control identities,
/// incomplete state inventories and unsafe or oversized reference paths.
#[expect(
    clippy::too_many_lines,
    reason = "The compatibility operation ledger keeps each admitted historical fixture adapter explicit."
)]
pub fn run(request: &Value) -> crate::Result<Value> {
    let output = request
        .get("output")
        .map(|value| {
            let name = value
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or("reference output path is required")?;
            require(!Path::new(name).exists(), "reference output must be fresh")?;
            Ok::<_, String>(name)
        })
        .transpose()?;
    let report = match text(request, "operation")? {
        "native-compare" => native::compare(request)?,
        "row-compare" => row::compare(request)?,
        "native-components" => native::components(
            &path(request, "path")?,
            integer(request, "position")?,
            request,
        )?,
        "adapter-condition" => {
            native::adapter_condition(&request["capture"], text(request, "condition")?)?
        }
        "native-controls" => native::controls(
            &path(request, "rust")?,
            &request["capture"],
            text(request, "backend")?,
            integer(request, "rowWorkers")?,
        )?,
        "row-controls" => row::controls(
            &path(request, "directory")?,
            integer(request, "workers")?,
            integer(&request["geometry"], "vocabulary")?,
            &row::model()?,
        )?,
        "tensor-header" => formats::tensor_header(
            text(request, "header")?.as_bytes(),
            integer(request, "fileBytes")?,
        )?,
        "safetensors-header" => formats::safetensors_header(&path(request, "path")?)?,
        "adapter" => formats::adapter(&read(&path(request, "path")?, 16 * 1024 * 1024)?)?,
        "public-reference" => public::produce(request)?,
        "public-replay" => audit::replay(request)?,
        "retained-audit" => retained::audit(request)?,
        "retained-replay" => {
            retained::replay(&path(request, "root")?, &path(request, "comparator")?)?
        }
        "retained-producer" => {
            retained::producer(&request["run"], &request["artifact"], &request["jobs"])?;
            json!({"validated":true})
        }
        "retained-redirect" => {
            retained::redirect(text(request, "url")?)?;
            json!({"validated":true})
        }
        "retained-extract" => retained::extract(
            &path(request, "archive")?,
            &path(request, "destination")?,
            &request["expected"],
        )?,
        "decode-json" => decode(&audit::unhex(text(request, "rawHex")?)?)?,
        "read-bytes" => {
            let raw = read_match(
                &path(request, "path")?,
                integer(request, "maximum")?,
                request.get("exact").and_then(Value::as_u64),
                request.get("sha256").and_then(Value::as_str),
            )?;
            json!({"rawHex":crate::identity::hex(&raw)})
        }
        "read-json" => read_json(&path(request, "path")?, integer(request, "maximum")?)?,
        "write-json" => {
            write_json(&path(request, "path")?, &request["value"])?;
            json!({"written":true})
        }
        "hash-bytes" => {
            json!({"sha256":crate::identity::sha256(&audit::unhex(text(request,"rawHex")?)?)})
        }
        "hash-file" => json!({"sha256":digest(&path(request,"path")?)?}),
        "equal" => json!({"equal":request["left"]==request["right"]}),
        "keys" => {
            let wanted = request["keys"].as_array().ok_or("invalid expected keys")?;
            require(
                request["value"].as_object().is_some_and(|object| {
                    object.len() == wanted.len()
                        && wanted
                            .iter()
                            .all(|key| key.as_str().is_some_and(|name| object.contains_key(name)))
                }),
                "reference object schema differs",
            )?;
            json!({"validated":true})
        }
        "integer" => {
            json!({"valid":request["value"].as_u64().is_some_and(|n|request["maximum"].as_u64().is_some_and(|maximum|n<=maximum))})
        }
        "public-hosted" => {
            public::hosted()?;
            json!({"validated":true})
        }
        "public-config" => public::config_sites(&audit::unhex(text(request, "rawHex")?)?)?,
        "public-directories" => {
            public::directories(
                &path(request, "data")?,
                &path(request, "evidence")?,
                &path(request, "temporary")?,
            )?;
            json!({"validated":true})
        }
        "public-save-source" => public::save_sources(
            &path(request, "repository")?,
            &path(request, "evidence")?,
            text(request, "revision")?,
            request["legacy"].as_bool().unwrap_or(false),
        )?,
        "transfer-validate" => {
            let raw = audit::unhex(text(request, "rawHex")?)?;
            require(
                u64::try_from(raw.len()).map_err(|error| error.to_string())?
                    == integer(&request["pin"], "bytes")?
                    && crate::identity::sha256(&raw) == text(&request["pin"], "sha256")?,
                "complete input raw-byte identity differs",
            )?;
            json!({"validated":true})
        }
        "audit-adapter" => {
            let factors = audit::adapter(&read(&path(request, "path")?, 16 * 1024 * 1024)?)?;
            Value::Object(
                factors
                    .into_iter()
                    .map(|(name, bytes)| (name, Value::from(crate::identity::hex(&bytes))))
                    .collect(),
            )
        }
        "audit-descriptors" => audit::descriptors(
            &path(request, "evidence")?,
            &request["report"],
            &request["pins"],
        )?,
        "audit-metrics" => {
            let actual = audit::vector(
                &audit::unhex(text(request, "actualHex")?)?,
                usize::try_from(integer(request, "count")?).map_err(|error| error.to_string())?,
            )?;
            let reference =
                audit::vector(&audit::unhex(text(request, "referenceHex")?)?, actual.len())?;
            audit::metrics(
                &actual,
                &reference,
                request["absolute"]
                    .as_f64()
                    .ok_or("invalid absolute tolerance")?,
                request["relative"]
                    .as_f64()
                    .ok_or("invalid relative tolerance")?,
            )?
        }
        _ => return Err("Unknown native reference operation.".into()),
    };
    if let Some(output) = output {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut file, &report).map_err(|error| error.to_string())?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
