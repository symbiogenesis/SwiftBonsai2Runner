//! Exact readiness for the explicitly selected native representation and adapter.

use serde::{Deserialize, Deserializer};
use std::{borrow::Cow, io, ops::Deref};

/// The selected private projection-worker budget, separate from execution evidence.
#[derive(Clone, Copy)]
pub(super) struct WorkerReadiness {
    expected: u8,
    context_tokens: usize,
    legacy_missing_allowed: bool,
    latest_prompt_reuse: bool,
    kv_cache: Option<crate::inference::KvCache>,
    placement: crate::inference::Placement,
}

impl WorkerReadiness {
    pub(super) fn for_version(version: &str, expected: u8) -> Self {
        Self {
            expected,
            context_tokens: crate::CONTEXT_TOKENS,
            // Only these reviewed historical releases omit this identity. A
            // later pin must report its graph's effective worker budget.
            legacy_missing_allowed: matches!(version, "0.1.8" | "0.1.9" | "0.1.10"),
            latest_prompt_reuse: false,
            kv_cache: None,
            placement: crate::inference::Placement::Auto,
        }
    }

    pub(super) const fn with_prompt_reuse(mut self, latest: bool) -> Self {
        self.latest_prompt_reuse = latest;
        self
    }

    pub(super) const fn with_kv_cache(mut self, policy: Option<crate::inference::KvCache>) -> Self {
        self.kv_cache = policy;
        self
    }

    pub(super) const fn with_context_tokens(mut self, context_tokens: usize) -> Self {
        self.context_tokens = context_tokens;
        self
    }

    pub(super) const fn with_placement(mut self, placement: crate::inference::Placement) -> Self {
        self.placement = placement;
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

fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(input)
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
    #[serde(default, borrow, deserialize_with = "present")]
    kv_cache: Option<Text<'a>>,
    #[serde(default, borrow, deserialize_with = "present")]
    startup_graph_identity: Option<Text<'a>>,
    #[serde(default, borrow, deserialize_with = "present")]
    placement: Option<Text<'a>>,
    #[serde(default, borrow, deserialize_with = "present")]
    projection_backend: Option<Text<'a>>,
    #[serde(default, borrow, deserialize_with = "present")]
    startup_projection: Option<StartupProjection<'a>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupProjection<'a> {
    schema_version: u8,
    #[serde(borrow)]
    requested: Text<'a>,
    #[serde(borrow)]
    effective: Text<'a>,
    #[serde(borrow, deserialize_with = "nullable")]
    identity: Option<Text<'a>>,
    resident_tensors: u64,
    resident_weight_bytes: u64,
    host_metadata_bytes: u64,
}

fn startup_projection_matches(
    model: &NativeModel<'_>,
    workers: WorkerReadiness,
    repacked: bool,
) -> bool {
    let Some(startup) = &model.startup_projection else {
        // The published 0.1.27 protocol predates this optional observation.
        // Requiring it belongs to the exact-source release repin, not a local
        // source change that would refuse the currently pinned executable.
        return true;
    };
    let requested = if workers.placement == crate::inference::Placement::Cpu {
        "cpu"
    } else {
        "auto"
    };
    if startup.schema_version != 1 || &*startup.requested != requested {
        return false;
    }
    let no_residency = startup.resident_tensors == 0
        && startup.resident_weight_bytes == 0
        && startup.host_metadata_bytes == 0;
    match &*startup.effective {
        "cpu" => startup.identity.is_none() && no_residency,
        "hexagon-native-q2" => {
            requested == "auto"
                && !repacked
                && startup.identity.as_deref()
                    == Some("hexagon-hvx-native-q2-resident-v1-experimental")
                && startup.resident_tensors == 168
                && startup.resident_weight_bytes == 2_888_663_040
                && (1..=524_288).contains(&startup.host_metadata_bytes)
        }
        "external" => {
            requested == "auto"
                && no_residency
                && startup.identity.as_deref().is_some_and(|identity| {
                    !identity.trim().is_empty()
                        && identity.len() <= 256
                        && identity.bytes().all(|byte| byte.is_ascii_graphic())
                })
        }
        _ => false,
    }
}

fn cache_identity_matches(
    model: &NativeModel<'_>,
    workers: WorkerReadiness,
    adapter: bool,
) -> bool {
    let Some(policy @ (crate::inference::KvCache::Rsq4 | crate::inference::KvCache::Rsq4K3v)) =
        workers.kv_cache
    else {
        return model.kv_cache.is_none() && model.startup_graph_identity.is_none();
    };
    let cache = policy.name();
    let mut identity = format!(
        "{}:qwen35-text-v1:nonflash-f16:kv-{cache}-d256-rht3-v1:gdn-f32:prism-arm64-fused-v1",
        &*model.model_sha256
    );
    if adapter {
        use std::fmt::Write as _;
        if write!(
            identity,
            ":rank-one-lora-v1:{}:{:08x}",
            crate::inference::ADAPTER_SHA256,
            1.3_f32.to_bits()
        )
        .is_err()
        {
            return false;
        }
    }
    model.kv_cache.as_deref() == Some(cache)
        && model.startup_graph_identity.as_deref() == Some(identity.as_str())
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
    let placement = (workers.placement == crate::inference::Placement::Cpu).then_some("cpu");
    if &*model.id != id
        || &*model.object != "model"
        || &*model.model_sha256 != hash
        || &*model.template_sha256 != crate::inference::TEMPLATE_SHA256
        || &*model.runtime != "prism-inference"
        || model.context_tokens != workers.context_tokens
        || model.seed != 42
        || !workers.accepts(model.row_workers)
        || model.prompt_reuse.as_deref() != workers.latest_prompt_reuse.then_some("latest")
        || !cache_identity_matches(model, workers, adapter)
        || model.placement.as_deref() != placement
        || model.projection_backend.as_deref() != placement
        || !startup_projection_matches(model, workers, repacked)
        || !extensions_match
    {
        return Err(io::Error::other("native model readiness identity mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{WorkerReadiness, validate_native_identity};

    fn kv_cache_model(repacked: bool, adapter: bool) -> (serde_json::Value, String) {
        use serde_json::json;
        let hash = if repacked {
            crate::inference::REPACKED_PTQ1_SHA256
        } else {
            crate::inference::MODEL_SHA256
        };
        let mut graph = format!(
            "{hash}:qwen35-text-v1:nonflash-f16:kv-rsq4-d256-rht3-v1:gdn-f32:prism-arm64-fused-v1"
        );
        if adapter {
            graph = format!(
                "{graph}:rank-one-lora-v1:{}:3fa66666",
                crate::inference::ADAPTER_SHA256
            );
        }
        let mut model = json!({
            "id": if repacked { "Swift-Bonsai-2-current-repacked-PTQ1_0" } else { "Swift-Bonsai-2-PQ2_0" },
            "object": "model", "model_sha256": hash,
            "template_sha256": crate::inference::TEMPLATE_SHA256,
            "runtime": "prism-inference", "context_tokens": 8192, "seed": 42,
            "row_workers": 4, "kv_cache": "rsq4", "startup_graph_identity": graph,
        });
        if repacked || adapter {
            model["weight_representation"] = json!(if repacked {
                "current-repacked-PTQ1_0"
            } else {
                "PQ2_0"
            });
            model["adapter_sha256"] = json!(adapter.then_some(crate::inference::ADAPTER_SHA256));
            model["supports_abliteration_scale"] = json!(adapter);
            if adapter {
                model["abliteration_scale_range"] = json!([0, 2]);
            }
        }
        (model, graph)
    }

    fn model_response(model: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
        let body = serde_json::to_string(&serde_json::json!({"data": [model]}))?;
        Ok(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes())
    }

    fn projection_startup(effective: &str) -> serde_json::Value {
        serde_json::json!({"schema_version":1,"requested":"auto","effective":effective,
            "identity": if effective == "hexagon-native-q2" {
                Some("hexagon-hvx-native-q2-resident-v1-experimental")
            } else { None },
            "resident_tensors": if effective == "hexagon-native-q2" {168} else {0},
            "resident_weight_bytes": if effective == "hexagon-native-q2" {2_888_663_040_u64} else {0},
            "host_metadata_bytes": if effective == "hexagon-native-q2" {4096} else {0}})
    }

    #[test]
    fn projection_startup_binds_completed_residency_and_requested_policy()
    -> Result<(), serde_json::Error> {
        use crate::inference::{KvCache, Placement};
        let (mut model, _) = kv_cache_model(false, true);
        let auto = WorkerReadiness::for_version("0.1.27", 4).with_kv_cache(Some(KvCache::Rsq4));
        // The currently published pin omits the optional startup observation.
        assert!(validate_native_identity(&model_response(&model)?, false, true, auto).is_ok());
        for effective in ["cpu", "hexagon-native-q2"] {
            model["startup_projection"] = projection_startup(effective);
            assert!(validate_native_identity(&model_response(&model)?, false, true, auto).is_ok());
        }
        let cpu = auto.with_placement(Placement::Cpu);
        model["placement"] = serde_json::json!("cpu");
        model["projection_backend"] = serde_json::json!("cpu");
        assert!(validate_native_identity(&model_response(&model)?, false, true, cpu).is_err());
        model["startup_projection"] = projection_startup("cpu");
        model["startup_projection"]["requested"] = serde_json::json!("cpu");
        assert!(validate_native_identity(&model_response(&model)?, false, true, cpu).is_ok());
        let (mut ptq1, _) = kv_cache_model(true, true);
        ptq1["startup_projection"] = projection_startup("hexagon-native-q2");
        assert!(validate_native_identity(&model_response(&ptq1)?, true, true, auto).is_err());
        Ok(())
    }

    #[test]
    fn projection_startup_rejects_inconsistent_unknown_and_ambiguous_facts()
    -> Result<(), serde_json::Error> {
        use crate::inference::KvCache;
        use serde_json::json;
        let (model, _) = kv_cache_model(false, false);
        let auto = WorkerReadiness::for_version("0.1.27", 4).with_kv_cache(Some(KvCache::Rsq4));
        for (field, value) in [
            ("schema_version", json!(2)),
            ("schema_version", json!(true)),
            ("requested", json!("cpu")),
            ("effective", json!("hexagon")),
            ("identity", json!(null)),
            ("identity", json!("changed")),
            ("resident_tensors", json!(167)),
            ("resident_tensors", json!(169)),
            ("resident_weight_bytes", json!(2_888_663_041_u64)),
            ("host_metadata_bytes", json!(0)),
            ("host_metadata_bytes", json!(524_289)),
            ("unreviewed_field", json!(true)),
        ] {
            let mut candidate = model.clone();
            candidate["startup_projection"] = projection_startup("hexagon-native-q2");
            candidate["startup_projection"][field] = value;
            assert!(
                validate_native_identity(&model_response(&candidate)?, false, false, auto).is_err(),
                "{field}"
            );
        }
        for invalid in [json!(null), json!({}), json!("cpu"), json!([])] {
            let mut candidate = model.clone();
            candidate["startup_projection"] = invalid;
            assert!(
                validate_native_identity(&model_response(&candidate)?, false, false, auto).is_err()
            );
        }
        for field in [
            "schema_version",
            "requested",
            "effective",
            "identity",
            "resident_tensors",
            "resident_weight_bytes",
            "host_metadata_bytes",
        ] {
            let mut candidate = model.clone();
            candidate["startup_projection"] = projection_startup("cpu");
            if let Some(object) = candidate["startup_projection"].as_object_mut() {
                object.remove(field);
            }
            assert!(
                validate_native_identity(&model_response(&candidate)?, false, false, auto).is_err(),
                "missing {field}"
            );
        }
        let mut candidate = model;
        candidate["startup_projection"] = projection_startup("cpu");
        candidate["startup_projection"]["resident_weight_bytes"] = json!(1);
        assert!(
            validate_native_identity(&model_response(&candidate)?, false, false, auto).is_err()
        );
        // Check duplicate names before a Value map could erase the ambiguity.
        candidate["startup_projection"] = projection_startup("cpu");
        let body = json!({"data":[candidate]}).to_string().replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        );
        let bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes();
        assert!(validate_native_identity(&bytes, false, false, auto).is_err());
        Ok(())
    }

    #[test]
    fn projection_startup_external_identity_never_asserts_residency()
    -> Result<(), serde_json::Error> {
        use crate::inference::KvCache;
        use serde_json::json;
        let (mut model, _) = kv_cache_model(false, false);
        let auto = WorkerReadiness::for_version("0.1.27", 4).with_kv_cache(Some(KvCache::Rsq4));
        model["startup_projection"] = projection_startup("external");
        for identity in ["", " ", "bad\nidentity", &"x".repeat(257)] {
            model["startup_projection"]["identity"] = json!(identity);
            assert!(
                validate_native_identity(&model_response(&model)?, false, false, auto).is_err()
            );
        }
        model["startup_projection"]["identity"] = json!("reviewed-external-executor-v1");
        assert!(validate_native_identity(&model_response(&model)?, false, false, auto).is_ok());
        model["startup_projection"]["resident_tensors"] = json!(168);
        assert!(validate_native_identity(&model_response(&model)?, false, false, auto).is_err());
        Ok(())
    }

    #[test]
    fn cpu_placement_readiness_requires_both_requested_and_effective_cpu()
    -> Result<(), serde_json::Error> {
        use crate::inference::{KvCache, Placement};
        use serde_json::json;

        for repacked in [false, true] {
            for adapter in [false, true] {
                let (mut model, _) = kv_cache_model(repacked, adapter);
                let auto =
                    WorkerReadiness::for_version("0.1.25", 4).with_kv_cache(Some(KvCache::Rsq4));
                let cpu = auto.with_placement(Placement::Cpu);
                assert!(
                    validate_native_identity(&model_response(&model)?, repacked, adapter, auto)
                        .is_ok()
                );
                assert!(
                    validate_native_identity(&model_response(&model)?, repacked, adapter, cpu)
                        .is_err()
                );
                model["placement"] = json!("cpu");
                model["projection_backend"] = json!("cpu");
                let bytes = model_response(&model)?;
                assert!(validate_native_identity(&bytes, repacked, adapter, cpu).is_ok());
                assert!(validate_native_identity(&bytes, repacked, adapter, auto).is_err());
                for field in ["placement", "projection_backend"] {
                    let mut missing = model.clone();
                    if let Some(object) = missing.as_object_mut() {
                        object.remove(field);
                    }
                    assert!(
                        validate_native_identity(
                            &model_response(&missing)?,
                            repacked,
                            adapter,
                            cpu
                        )
                        .is_err()
                    );
                    for value in [
                        json!("auto"),
                        json!("CPU"),
                        json!("hexagon"),
                        json!(null),
                        json!(true),
                        json!(1),
                        json!([]),
                    ] {
                        let mut changed = model.clone();
                        changed[field] = value;
                        assert!(
                            validate_native_identity(
                                &model_response(&changed)?,
                                repacked,
                                adapter,
                                cpu
                            )
                            .is_err()
                        );
                        assert!(
                            validate_native_identity(
                                &model_response(&changed)?,
                                repacked,
                                adapter,
                                auto
                            )
                            .is_err()
                        );
                    }
                }
            }
        }
        let cpu = WorkerReadiness::for_version("0.1.25", 4).with_placement(Placement::Cpu);
        for extra in [
            "\"placement\":\"cpu\",\"projection_backend\":\"cpu\",\"placement\":\"cpu\"",
            "\"placement\":\"cpu\",\"projection_backend\":\"cpu\",\"projection_backend\":\"cpu\"",
        ] {
            assert!(
                validate_native_identity(&response(Some(&format!("4,{extra}"))), false, false, cpu)
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn kv_cache_readiness_binds_profile_graph_representation_and_startup_adapter()
    -> Result<(), serde_json::Error> {
        use crate::inference::KvCache;
        use serde_json::json;
        for policy in [KvCache::Rsq4, KvCache::Rsq4K3v] {
            for repacked in [false, true] {
                for adapter in [false, true] {
                    let (mut model, graph) = kv_cache_model(repacked, adapter);
                    let graph = graph.replace("kv-rsq4-", &format!("kv-{}-", policy.name()));
                    model["kv_cache"] = json!(policy.name());
                    model["startup_graph_identity"] = json!(graph);
                    let f16 = WorkerReadiness::for_version("0.1.25", 4);
                    let selected = f16.with_kv_cache(Some(policy));
                    let bytes = model_response(&model)?;
                    assert!(validate_native_identity(&bytes, repacked, adapter, selected).is_ok());
                    assert!(validate_native_identity(&bytes, repacked, adapter, f16).is_err());
                    let other = if policy == KvCache::Rsq4 {
                        KvCache::Rsq4K3v
                    } else {
                        KvCache::Rsq4
                    };
                    for (field, replacement) in [
                        ("kv_cache", json!("f16")),
                        ("kv_cache", json!(other.name())),
                        ("kv_cache", json!(null)),
                        ("startup_graph_identity", json!("wrong-model-or-codec")),
                        (
                            "startup_graph_identity",
                            json!(format!("{graph}:unexpected")),
                        ),
                        (
                            "startup_graph_identity",
                            json!(graph.replace(policy.name(), other.name())),
                        ),
                        ("startup_graph_identity", json!(null)),
                    ] {
                        let mut changed = model.clone();
                        changed[field] = replacement;
                        assert!(
                            validate_native_identity(
                                &model_response(&changed)?,
                                repacked,
                                adapter,
                                selected
                            )
                            .is_err()
                        );
                    }
                    for field in ["kv_cache", "startup_graph_identity"] {
                        let mut changed = model.clone();
                        if let Some(object) = changed.as_object_mut() {
                            object.remove(field);
                        }
                        assert!(
                            validate_native_identity(
                                &model_response(&changed)?,
                                repacked,
                                adapter,
                                selected
                            )
                            .is_err()
                        );
                    }
                    if let Some(object) = model.as_object_mut() {
                        object.remove("kv_cache");
                        object.remove("startup_graph_identity");
                    }
                    let bytes = model_response(&model)?;
                    assert!(validate_native_identity(&bytes, repacked, adapter, f16).is_ok());
                    assert!(validate_native_identity(&bytes, repacked, adapter, selected).is_err());
                }
            }
        }
        Ok(())
    }

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
            "0.1.19", "0.1.20", "0.1.21", "0.1.22",
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
            "0.1.18", "0.1.19", "0.1.20", "0.1.21", "0.1.22", "0.1.22",
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

#[cfg(test)]
mod context_tests {
    use super::{WorkerReadiness, validate_native_identity};

    #[test]
    fn readiness_rejects_any_context_other_than_the_selected_graph() {
        let body = serde_json::json!({"data":[{
            "id":"Swift-Bonsai-2-PQ2_0", "object":"model",
            "model_sha256":crate::inference::MODEL_SHA256,
            "template_sha256":crate::inference::TEMPLATE_SHA256,
            "runtime":"prism-inference", "context_tokens":16384,
            "seed":42, "row_workers":1
        }]})
        .to_string();
        let wire = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let selected = WorkerReadiness::for_version("0.1.25", 1).with_context_tokens(16384);
        assert!(validate_native_identity(wire.as_bytes(), false, false, selected).is_ok());
        let legacy = WorkerReadiness::for_version("0.1.25", 1);
        assert!(validate_native_identity(wire.as_bytes(), false, false, legacy).is_err());
    }
}
