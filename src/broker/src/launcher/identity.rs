//! Exact readiness for the explicitly selected native representation and adapter.

use serde::{Deserialize, Deserializer};
use std::io;

/// The selected private projection-worker budget, separate from execution evidence.
#[derive(Clone, Copy)]
pub(super) struct WorkerReadiness {
    expected: u8,
    legacy_missing_allowed: bool,
}

impl WorkerReadiness {
    pub(super) fn for_version(version: &str, expected: u8) -> Self {
        Self {
            expected,
            // Only these reviewed historical releases omit this identity. A
            // later pin must report its graph's effective worker budget.
            legacy_missing_allowed: matches!(version, "0.1.8" | "0.1.9" | "0.1.10"),
        }
    }

    fn accepts(self, observed: Option<u8>) -> bool {
        matches!(self.expected, 1 | 4)
            && observed.map_or(self.legacy_missing_allowed, |workers| {
                workers == self.expected
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Models {
    data: Vec<crate::Object<NativeModel>>,
}

#[derive(Default)]
enum AdapterIdentity {
    #[default]
    Absent,
    Present(Option<String>),
}

impl<'de> Deserialize<'de> for AdapterIdentity {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Option::<String>::deserialize(input).map(Self::Present)
    }
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeModel {
    id: String,
    object: String,
    model_sha256: String,
    template_sha256: String,
    runtime: String,
    context_tokens: usize,
    seed: u32,
    #[serde(default, deserialize_with = "present")]
    row_workers: Option<u8>,
    #[serde(default, deserialize_with = "present")]
    weight_representation: Option<String>,
    #[serde(default)]
    adapter_sha256: AdapterIdentity,
    #[serde(default, deserialize_with = "present")]
    supports_abliteration_scale: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    abliteration_scale_range: Option<[u8; 2]>,
}

pub(super) fn validate_native_identity(
    bytes: &[u8],
    repacked: bool,
    adapter: bool,
    workers: WorkerReadiness,
) -> io::Result<()> {
    let body = super::validate_response(bytes, 200)?;
    let crate::Object(models): crate::Object<Models> =
        serde_json::from_slice(body).map_err(io::Error::other)?;
    let [crate::Object(model)] = models.data.as_slice() else {
        return Err(io::Error::other("native model inventory mismatch"));
    };
    let (id, hash, representation) = if repacked {
        (
            "Swift-Bonsai-2-current-repacked-PTQ1_0",
            crate::inference::REPACKED_PTQ1_SHA256,
            "current-repacked-PTQ1_0",
        )
    } else {
        (
            "Swift-Bonsai-2-PQ2_0",
            crate::inference::MODEL_SHA256,
            "PQ2_0",
        )
    };
    let extensions_match = if repacked || adapter {
        model.weight_representation.as_deref() == Some(representation)
            && matches!(&model.adapter_sha256, AdapterIdentity::Present(value)
                if value.as_deref() == adapter.then_some(crate::inference::ADAPTER_SHA256))
            && model.supports_abliteration_scale == Some(adapter)
            && model.abliteration_scale_range == adapter.then_some([0, 2])
    } else {
        model.weight_representation.is_none()
            && matches!(&model.adapter_sha256, AdapterIdentity::Absent)
            && model.supports_abliteration_scale.is_none()
            && model.abliteration_scale_range.is_none()
    };
    if model.id != id
        || model.object != "model"
        || model.model_sha256 != hash
        || model.template_sha256 != crate::inference::TEMPLATE_SHA256
        || model.runtime != "prism-inference"
        || model.context_tokens != crate::CONTEXT_TOKENS
        || model.seed != 42
        || !workers.accepts(model.row_workers)
        || !extensions_match
    {
        return Err(io::Error::other("native model readiness identity mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{WorkerReadiness, validate_native_identity};

    fn response(workers: Option<&str>) -> Vec<u8> {
        let extension =
            workers.map_or_else(String::new, |workers| format!(",\"row_workers\":{workers}"));
        let body = format!(
            "{{\"data\":[{{\"id\":\"Swift-Bonsai-2-PQ2_0\",\"object\":\"model\",\"model_sha256\":\"{}\",\"template_sha256\":\"{}\",\"runtime\":\"prism-inference\",\"context_tokens\":8192,\"seed\":42{extension}}}]}}",
            crate::inference::MODEL_SHA256,
            crate::inference::TEMPLATE_SHA256,
        );
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn worker_readiness_missing_field_is_limited_to_exact_legacy_pins() {
        for (version, legacy) in [
            ("0.1.8", true),
            ("0.1.9", true),
            ("0.1.10", true),
            ("0.1.11", false),
            ("1.0.0", false),
            ("0.1.010", false),
            ("v0.1.10", false),
            ("", false),
        ] {
            for expected in [1, 4] {
                let policy = WorkerReadiness::for_version(version, expected);
                assert_eq!(
                    validate_native_identity(&response(None), false, false, policy).is_ok(),
                    legacy,
                    "{version}/{expected}"
                );
                assert!(
                    validate_native_identity(
                        &response(Some(&expected.to_string())),
                        false,
                        false,
                        policy,
                    )
                    .is_ok()
                );
                let other = if expected == 1 { "4" } else { "1" };
                assert!(
                    validate_native_identity(&response(Some(other)), false, false, policy).is_err()
                );
            }
        }
    }

    #[test]
    fn current_release_requires_workers_for_every_adapter_and_representation_profile()
    -> Result<(), serde_json::Error> {
        use serde_json::json;

        for repacked in [false, true] {
            for adapter in [false, true] {
                for expected in [1, 4] {
                    let mut model = json!({
                        "id": if repacked { "Swift-Bonsai-2-current-repacked-PTQ1_0" } else { "Swift-Bonsai-2-PQ2_0" },
                        "object": "model",
                        "model_sha256": if repacked { crate::inference::REPACKED_PTQ1_SHA256 } else { crate::inference::MODEL_SHA256 },
                        "template_sha256": crate::inference::TEMPLATE_SHA256,
                        "runtime": "prism-inference", "context_tokens": 8192, "seed": 42,
                    });
                    if repacked || adapter {
                        model["weight_representation"] = json!(if repacked {
                            "current-repacked-PTQ1_0"
                        } else {
                            "PQ2_0"
                        });
                        model["adapter_sha256"] =
                            json!(adapter.then_some(crate::inference::ADAPTER_SHA256));
                        model["supports_abliteration_scale"] = json!(adapter);
                        if adapter {
                            model["abliteration_scale_range"] = json!([0, 2]);
                        }
                    }
                    for observed in [None, Some(1), Some(4)] {
                        let mut candidate = model.clone();
                        if let Some(workers) = observed {
                            candidate["row_workers"] = json!(workers);
                        }
                        let body = serde_json::to_string(&json!({"data": [candidate]}))?;
                        let framed = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        assert_eq!(
                            validate_native_identity(
                                framed.as_bytes(),
                                repacked,
                                adapter,
                                WorkerReadiness::for_version("0.1.11", expected)
                            )
                            .is_ok(),
                            observed == Some(expected),
                            "repacked={repacked}, adapter={adapter}, expected={expected}, observed={observed:?}"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn worker_readiness_rejects_coercion_unbounded_counts_and_duplicate_fields() {
        for version in ["0.1.10", "0.1.11"] {
            let policy = WorkerReadiness::for_version(version, 4);
            for invalid in [
                "null",
                "\"4\"",
                "0",
                "2",
                "16",
                "17",
                "-1",
                "256",
                "18446744073709551616",
                "4.0",
                "true",
                "[]",
                "{}",
                "4,\"row_workers\":4",
                "4,\"row_workers\":null",
            ] {
                assert!(
                    validate_native_identity(&response(Some(invalid)), false, false, policy)
                        .is_err(),
                    "{version}/{invalid}"
                );
            }
        }
        for expected in [0, 2, 16, 255] {
            let policy = WorkerReadiness::for_version("0.1.10", expected);
            assert!(validate_native_identity(&response(None), false, false, policy).is_err());
            assert!(
                validate_native_identity(
                    &response(Some(&expected.to_string())),
                    false,
                    false,
                    policy,
                )
                .is_err()
            );
        }
    }
}
