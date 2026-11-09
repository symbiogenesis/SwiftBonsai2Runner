//! Exact readiness for the explicitly selected native representation and adapter.

use serde::{Deserialize, Deserializer};
use std::{borrow::Cow, io, ops::Deref};

/// The selected private projection-worker budget, separate from execution evidence.
#[derive(Clone, Copy)]
pub(super) struct WorkerReadiness {
    expected: u8,
    legacy_missing_allowed: bool,
    latest_prompt_reuse: bool,
}

impl WorkerReadiness {
    pub(super) fn for_version(version: &str, expected: u8) -> Self {
        Self {
            expected,
            // Only these reviewed historical releases omit this identity. A
            // later pin must report its graph's effective worker budget.
            legacy_missing_allowed: matches!(version, "0.1.8" | "0.1.9" | "0.1.10"),
            latest_prompt_reuse: false,
        }
    }

    pub(super) const fn with_prompt_reuse(mut self, latest: bool) -> Self {
        self.latest_prompt_reuse = latest;
        self
    }

    fn accepts(self, observed: Option<u8>) -> bool {
        matches!(self.expected, 1 | 4)
            && observed.map_or(self.legacy_missing_allowed, |workers| {
                workers == self.expected
            })
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct Text<'a>(#[serde(borrow)] Cow<'a, str>);

impl Deref for Text<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Models<'a> {
    #[serde(borrow)]
    data: [crate::Object<NativeModel<'a>>; 1],
}

#[derive(Default)]
enum AdapterIdentity<'a> {
    #[default]
    Absent,
    Present(Option<Text<'a>>),
}

impl<'de: 'a, 'a> Deserialize<'de> for AdapterIdentity<'a> {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Option::<Text<'a>>::deserialize(input).map(Self::Present)
    }
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeModel<'a> {
    #[serde(borrow)]
    id: Text<'a>,
    #[serde(borrow)]
    object: Text<'a>,
    #[serde(borrow)]
    model_sha256: Text<'a>,
    #[serde(borrow)]
    template_sha256: Text<'a>,
    #[serde(borrow)]
    runtime: Text<'a>,
    context_tokens: usize,
    seed: u32,
    #[serde(default, deserialize_with = "present")]
    row_workers: Option<u8>,
    #[serde(default, borrow, deserialize_with = "present")]
    weight_representation: Option<Text<'a>>,
    #[serde(default, borrow)]
    adapter_sha256: AdapterIdentity<'a>,
    #[serde(default, deserialize_with = "present")]
    supports_abliteration_scale: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    abliteration_scale_range: Option<[u8; 2]>,
    #[serde(default, borrow, deserialize_with = "present")]
    prompt_reuse: Option<Text<'a>>,
}

pub(super) fn validate_native_identity(
    bytes: &[u8],
    repacked: bool,
    adapter: bool,
    workers: WorkerReadiness,
) -> io::Result<()> {
    let body = super::validate_response(bytes, 200)?;
    let crate::Object(models): crate::Object<Models<'_>> =
        serde_json::from_slice(body).map_err(io::Error::other)?;
    let [crate::Object(model)] = &models.data;
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
    if &*model.id != id
        || &*model.object != "model"
        || &*model.model_sha256 != hash
        || &*model.template_sha256 != crate::inference::TEMPLATE_SHA256
        || &*model.runtime != "prism-inference"
        || model.context_tokens != crate::CONTEXT_TOKENS
        || model.seed != 42
        || !workers.accepts(model.row_workers)
        || model.prompt_reuse.as_deref() != workers.latest_prompt_reuse.then_some("latest")
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
            ("0.1.12", false),
            ("0.1.13", false),
            ("0.1.14", false),
            ("0.1.14-dev", false),
            ("v0.1.14", false),
            ("0.1.14.0", false),
            ("0.1.014", false),
            ("0.1.15", false),
            ("0.1.15-dev", false),
            ("v0.1.15", false),
            ("0.1.15.0", false),
            ("0.1.015", false),
            ("0.1.16", false),
            ("0.1.16-dev", false),
            ("v0.1.16", false),
            ("0.1.16.0", false),
            ("0.1.016", false),
            ("0.1.17", false),
            ("0.1.17-dev", false),
            ("v0.1.17", false),
            ("0.1.17.0", false),
            ("0.1.017", false),
            ("0.1.18", false),
            ("0.1.18-dev", false),
            ("v0.1.18", false),
            ("0.1.18.0", false),
            ("0.1.018", false),
            ("0.1.19", false),
            ("0.1.19-dev", false),
            ("v0.1.19", false),
            ("0.1.19.0", false),
            ("0.1.019", false),
            ("0.1.20", false),
            ("0.1.20-dev", false),
            ("v0.1.20", false),
            ("0.1.20.0", false),
            ("0.1.020", false),
            ("0.1.21", false),
            ("0.1.21-dev", false),
            ("v0.1.21", false),
            ("0.1.21.0", false),
            ("0.1.021", false),
            ("0.1.22", false),
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
    fn prompt_reuse_readiness_must_match_the_explicit_request_exactly() {
        let reset = WorkerReadiness::for_version("0.1.14", 4);
        let latest = reset.with_prompt_reuse(true);
        assert!(validate_native_identity(&response(Some("4")), false, false, reset).is_ok());
        assert!(validate_native_identity(&response(Some("4")), false, false, latest).is_err());
        for extension in [
            "\"latest\"",
            "\"disabled\"",
            "\"Latest\"",
            "null",
            "true",
            "0",
            "{}",
            "[]",
            "\"latest\",\"prompt_reuse\":\"latest\"",
        ] {
            let bytes = response(Some(&format!("4,\"prompt_reuse\":{extension}")));
            assert!(validate_native_identity(&bytes, false, false, reset).is_err());
            assert_eq!(
                validate_native_identity(&bytes, false, false, latest).is_ok(),
                extension == "\"latest\"",
                "{extension}"
            );
        }
    }

    #[test]
    fn current_release_requires_workers_for_every_adapter_and_representation_profile()
    -> Result<(), serde_json::Error> {
        use serde_json::json;

        for version in [
            "0.1.11", "0.1.12", "0.1.13", "0.1.14", "0.1.15", "0.1.16", "0.1.17", "0.1.18",
            "0.1.19", "0.1.20", "0.1.21",
        ] {
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
                                    WorkerReadiness::for_version(version, expected)
                                )
                                .is_ok(),
                                observed == Some(expected),
                                "version={version}, repacked={repacked}, adapter={adapter}, expected={expected}, observed={observed:?}"
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn worker_readiness_rejects_coercion_unbounded_counts_and_duplicate_fields() {
        for version in [
            "0.1.10", "0.1.11", "0.1.12", "0.1.13", "0.1.14", "0.1.15", "0.1.16", "0.1.17",
            "0.1.18", "0.1.19", "0.1.20", "0.1.21",
        ] {
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
