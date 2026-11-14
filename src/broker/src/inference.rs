//! Compiled release identity for the independently built inference server.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Explicit parent scope keeps these implementation helpers private."
)]

use std::path::{Path, PathBuf};

use bonsai_artifacts::VerifiedFile;
use serde::Deserialize;

use crate::{Object, launcher::LaunchError};

const LOCK: &str = include_str!("../../../config/prism-inference.lock.json");
pub(super) const MODEL_SHA256: &str =
    "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
pub(super) const REPACKED_PTQ1_SHA256: &str =
    "e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5";
pub(super) const ADAPTER_SHA256: &str =
    "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
pub(super) const TEMPLATE_SHA256: &str =
    "c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041";

// Source review enables the policy; actual use still requires the separately
// verified released executable in LOCK. Newer versions need another source review.
const TURBOQUANT_REVISION: &str = "99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KvCache {
    F16,
    Rsq4,
    Rsq4K3v,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Placement {
    #[default]
    Auto,
    Cpu,
}

impl Placement {
    pub(super) fn parse(value: Option<&std::ffi::OsStr>) -> Result<Self, LaunchError> {
        match value.and_then(std::ffi::OsStr::to_str) {
            Some("auto") => Ok(Self::Auto),
            Some("cpu") => Ok(Self::Cpu),
            _ => Err(LaunchError::Arguments),
        }
    }
}

// CPU placement must skip accelerator initialization and report both its
// requested placement and effective CPU backend in a genuine reviewed release.
// Source omission alone does not justify a smaller fixed memory allowance.
const CPU_PLACEMENT_RELEASE: Option<(&str, &str)> =
    Some(("0.1.28", "99671be7ab2be01e406a11b0c3d6caede398d96f"));

fn select_placement(release: &ReleasePin, requested: Placement) -> Result<Placement, LaunchError> {
    if requested == Placement::Auto || reviewed_release_matches(release, CPU_PLACEMENT_RELEASE) {
        Ok(requested)
    } else {
        Err(LaunchError::NativeRuntime)
    }
}

pub(super) fn pinned_placement(
    native: bool,
    requested: Option<Placement>,
) -> Result<Placement, LaunchError> {
    if !native {
        return requested.map_or(Ok(Placement::Auto), |_| Err(LaunchError::Arguments));
    }
    select_placement(&release(LOCK)?, requested.unwrap_or_default())
}

impl KvCache {
    pub(super) fn parse(value: Option<&std::ffi::OsStr>) -> Result<Self, LaunchError> {
        match value.and_then(std::ffi::OsStr::to_str) {
            Some("f16") => Ok(Self::F16),
            Some("rsq4") => Ok(Self::Rsq4),
            Some("rsq4k3v") => Ok(Self::Rsq4K3v),
            _ => Err(LaunchError::Arguments),
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::F16 => "f16",
            Self::Rsq4 => "rsq4",
            Self::Rsq4K3v => "rsq4k3v",
        }
    }
}

fn reviewed_release_matches(release: &ReleasePin, identity: Option<(&str, &str)>) -> bool {
    identity.is_some_and(|(version, commit)| release.version == version && release.commit == commit)
}

fn turboquant_supported(release: &ReleasePin, memory_release: Option<(&str, &str)>) -> bool {
    (release.version == "0.1.25" && release.commit == TURBOQUANT_REVISION)
        || (release.version == "0.1.24"
            && release.commit == "8d2b3067e5f17369f579857a180c01a422a58211")
        // M1/M2 review includes the selected K4/V4 cache and its source-bound
        // readiness contract; delivery must retain the ordinary cache default.
        || reviewed_release_matches(release, memory_release)
}

fn select_kv_cache(
    release: &ReleasePin,
    requested: Option<KvCache>,
) -> Result<Option<KvCache>, LaunchError> {
    if requested == Some(KvCache::Rsq4K3v) {
        if k4v3_context_supported(release) {
            Ok(requested)
        } else {
            Err(LaunchError::NativeRuntime)
        }
    } else if turboquant_supported(release, MEMORY_CONTEXT_RELEASE) {
        Ok(Some(requested.unwrap_or(KvCache::Rsq4)))
    } else if requested == Some(KvCache::Rsq4) {
        Err(LaunchError::NativeRuntime)
    } else {
        // Historical servers do not accept --kv-cache; their reviewed profile
        // already selects FP16. This is release selection, never retry fallback.
        Ok(None)
    }
}

pub(super) fn pinned_kv_cache(
    native: bool,
    requested: Option<KvCache>,
) -> Result<Option<KvCache>, LaunchError> {
    if native {
        select_kv_cache(&release(LOCK)?, requested)
    } else if requested.is_some() {
        Err(LaunchError::Arguments)
    } else {
        Ok(None)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleasePin {
    schema_version: u8,
    package: String,
    version: String,
    repository: String,
    commit: String,
    target: String,
    release: String,
    asset: Object<Asset>,
    files: Vec<Object<File>>,
    #[serde(rename = "Source")]
    source: Object<Source>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Source {
    repository: String,
    commit: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    filename: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    bytes: u64,
    sha256: String,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn release(input: &str) -> Result<ReleasePin, LaunchError> {
    let Object(release): Object<ReleasePin> =
        serde_json::from_str(input).map_err(|_| LaunchError::Artifacts)?;
    let filename = format!("prism-inference-{}-{}.zip", release.version, release.target);
    if release.schema_version != 1
        || release.package != "prism-inference"
        || release.repository != "https://github.com/symbiogenesis/prism-inference"
        || release.target != "aarch64-pc-windows-msvc"
        || release.version.is_empty()
        || !release
            .version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || release.release.strip_prefix('v') != Some(release.version.as_str())
        || !hex(&release.commit, 40)
        || release.asset.filename != filename
        || release.asset.url
            != format!(
                "{}/releases/download/{}/{}",
                release.repository, release.release, filename
            )
        || release.asset.bytes == 0
        || !hex(&release.asset.sha256, 64)
        || release.source.repository != release.repository
        || release.source.commit != release.commit
        || release.source.url != format!("{}/archive/{}.zip", release.repository, release.commit)
        || release.source.bytes == 0
        || release.source.bytes > 256 * 1024 * 1024
        || !hex(&release.source.sha256, 64)
        || release.files.is_empty()
    {
        return Err(LaunchError::Artifacts);
    }
    let mut names = std::collections::HashSet::new();
    for Object(file) in &release.files {
        if file.bytes == 0
            || !hex(&file.sha256, 64)
            || !names.insert(file.name.as_str())
            || ![
                "prism-inference.exe",
                "bonsai-token-worker.exe",
                "bonsai-swift-worker.exe",
                "prism-inference-server.exe",
                "bonsai-gguf.exe",
                "LICENSE",
                "NOTICE",
                "PRISM-LICENSE.txt",
                "CAMELID-LICENSE.txt",
            ]
            .contains(&file.name.as_str())
        {
            return Err(LaunchError::Artifacts);
        }
    }
    if !names.contains("prism-inference-server.exe") {
        return Err(LaunchError::Artifacts);
    }
    Ok(release)
}

fn row_workers_supported(
    release: &ReleasePin,
    workers: u8,
    memory_release: Option<(&str, &str)>,
) -> Result<(), LaunchError> {
    if workers == 1
        || (workers == 4
            && (reviewed_release_matches(release, memory_release)
                || matches!(
                    release.version.as_str(),
                    "0.1.9"
                        | "0.1.10"
                        | "0.1.11"
                        | "0.1.12"
                        | "0.1.13"
                        | "0.1.14"
                        | "0.1.15"
                        | "0.1.16"
                        | "0.1.17"
                        | "0.1.18"
                        | "0.1.19"
                        | "0.1.20"
                        | "0.1.21"
                        | "0.1.22"
                        | "0.1.23"
                        | "0.1.24"
                        | "0.1.25"
                )))
    {
        Ok(())
    } else {
        Err(LaunchError::Artifacts)
    }
}

// Enable only after a reviewed source-bound release implements the paired
// context/memory flags AND the selected-cache/sequential-scratch accounting
// contract in bonsai_cpu_policy::context. A protocol-only source candidate must
// not activate smaller process allowances. A version number alone attests neither.
// This combined review also retains four row workers, K4/V4 and latest reuse;
// a later repin cannot silently lose those existing serving capabilities.
const MEMORY_CONTEXT_RELEASE: Option<(&str, &str)> =
    Some(("0.1.28", "99671be7ab2be01e406a11b0c3d6caede398d96f"));

// K4/V3 changes precision and needs its own reviewed capacity/quality release.
// Both gates must identify the same genuine released source before this profile
// can use its smaller admission charge. Existing K4/V4 releases stay unchanged.
const K4V3_CONTEXT_RELEASE: Option<(&str, &str)> = None;

// Full-model K4/V4 context needs the separately reviewed larger live-state and
// checkpoint guards, not just the existing paired memory protocol. This exact
// source supplies the geometry-derived Swift allowance; generic graphs retain
// 4 GiB. Precision and process charges remain unchanged; this gate cannot
// authorize an unrelated memory release.
const K4V4_FULL_CONTEXT_RELEASE: Option<(&str, &str)> =
    Some(("0.1.28", "99671be7ab2be01e406a11b0c3d6caede398d96f"));

fn full_context_release_matches(release: &ReleasePin, reviewed: Option<(&str, &str)>) -> bool {
    reviewed.is_some()
        && MEMORY_CONTEXT_RELEASE == reviewed
        && reviewed_release_matches(release, reviewed)
}

pub(super) fn full_context_supported() -> Result<bool, LaunchError> {
    let pin = release(LOCK)?;
    Ok(full_context_release_matches(
        &pin,
        K4V4_FULL_CONTEXT_RELEASE,
    ))
}

fn k4v3_context_supported(release: &ReleasePin) -> bool {
    K4V3_CONTEXT_RELEASE.is_some_and(|identity| {
        MEMORY_CONTEXT_RELEASE == Some(identity)
            && release.version == identity.0
            && release.commit == identity.1
    })
}

pub(super) fn memory_context_supported() -> Result<bool, LaunchError> {
    let pin = release(LOCK)?;
    Ok(reviewed_release_matches(&pin, MEMORY_CONTEXT_RELEASE))
}

pub(super) fn pinned_version() -> Result<String, LaunchError> {
    release(LOCK).map(|release| release.version)
}

fn prompt_reuse_supported(release: &ReleasePin, latest: bool) -> Result<(), LaunchError> {
    // Capability is bound to reviewed source and immutable released bytes. A
    // later version must be reviewed explicitly, even if its CLI looks similar.
    if !latest
        || turboquant_supported(release, MEMORY_CONTEXT_RELEASE)
        || (release.version == "0.1.14"
            && release.commit == "9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57")
        || (release.version == "0.1.15"
            && release.commit == "730be3acc7f8893af1397ec85f71da7fee80c3be")
        || (release.version == "0.1.16"
            && release.commit == "88d812b563c930c20843f8d88f1b27a849db50a5")
        || (release.version == "0.1.17"
            && release.commit == "958fd4a8bc3feb2ae1f923d15d279eb191a4fd6f")
        || (release.version == "0.1.18"
            && release.commit == "c4cfe40eedd3d7913a225cfa494ac596c948c9c9")
        || (release.version == "0.1.19"
            && release.commit == "951cc88486fdc5609a3aa57aad3b9e92216de2b1")
        || (release.version == "0.1.20"
            && release.commit == "3a22fefc497da3d571c07bba072f270fc9c4b135")
        || (release.version == "0.1.21"
            && release.commit == "90eb97481a8df532cc0375660a1533bb0c5fe4cb")
        || (release.version == "0.1.22"
            && release.commit == "6b4f4520b465e5feb665a71af64e7b0e21eb750a")
        || (release.version == "0.1.23"
            && release.commit == "055c7430cb4771dfddb24ddaa30494af6ad182da")
    {
        Ok(())
    } else {
        Err(LaunchError::Artifacts)
    }
}

pub(super) fn verify(
    data: &Path,
    row_workers: u8,
    latest_prompt_reuse: bool,
) -> Result<(PathBuf, VerifiedFile), LaunchError> {
    let release = release(LOCK)?;
    row_workers_supported(&release, row_workers, MEMORY_CONTEXT_RELEASE)?;
    prompt_reuse_supported(&release, latest_prompt_reuse)?;
    let file = release
        .files
        .iter()
        .find(|Object(file)| file.name == "prism-inference-server.exe")
        .ok_or(LaunchError::Artifacts)?;
    let executable = data
        .join("inference")
        .join(&release.commit)
        .join(&release.target)
        .join(&file.name);
    let guard = VerifiedFile::verify(&executable, file.bytes, &file.sha256)
        .map_err(|_| LaunchError::Artifacts)?;
    Ok((executable, guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_placement_waits_for_an_exact_reviewed_release() -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        assert_eq!(select_placement(&pin, Placement::Cpu)?, Placement::Cpu);
        for (version, commit) in [
            ("0.1.25", TURBOQUANT_REVISION),
            ("0.1.26", TURBOQUANT_REVISION),
            ("0.1.26", "0000000000000000000000000000000000000000"),
            ("0.1.27", "0000000000000000000000000000000000000000"),
            ("0.1.28", "0000000000000000000000000000000000000000"),
            ("0.1.27", "99671be7ab2be01e406a11b0c3d6caede398d96f"),
            ("0.1.28", "1429a33772089dfd3a50a96c698b9dbe0b36836a"),
        ] {
            pin.version = version.into();
            pin.commit = commit.into();
            assert_eq!(select_placement(&pin, Placement::Auto)?, Placement::Auto);
            assert_eq!(
                select_placement(&pin, Placement::Cpu),
                Err(LaunchError::NativeRuntime)
            );
        }
        assert_eq!(pinned_placement(true, None)?, Placement::Auto);
        assert_eq!(pinned_placement(false, None)?, Placement::Auto);
        for placement in [Placement::Auto, Placement::Cpu] {
            assert_eq!(
                pinned_placement(false, Some(placement)),
                Err(LaunchError::Arguments)
            );
        }
        Ok(())
    }

    #[test]
    fn kv_cache_default_and_recovery_require_the_exact_reviewed_source() -> Result<(), LaunchError>
    {
        let mut pin = release(LOCK)?;
        assert_eq!(
            MEMORY_CONTEXT_RELEASE,
            Some((pin.version.as_str(), pin.commit.as_str()))
        );
        assert!(memory_context_supported()?);
        assert!(row_workers_supported(&pin, 4, MEMORY_CONTEXT_RELEASE).is_ok());
        assert_eq!(select_kv_cache(&pin, None)?, Some(KvCache::Rsq4));
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::F16))?,
            Some(KvCache::F16)
        );
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4))?,
            Some(KvCache::Rsq4)
        );
        pin.version = "0.1.23".into();
        pin.commit = "055c7430cb4771dfddb24ddaa30494af6ad182da".into();
        assert_eq!(select_kv_cache(&pin, None)?, None);
        assert_eq!(select_kv_cache(&pin, Some(KvCache::F16))?, None);
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4)),
            Err(LaunchError::NativeRuntime)
        );
        pin.version = "0.1.25".into();
        pin.commit = "0".repeat(40);
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4)),
            Err(LaunchError::NativeRuntime)
        );
        pin.commit = TURBOQUANT_REVISION.into();
        pin.version = "0.1.26".into();
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4)),
            Err(LaunchError::NativeRuntime)
        );
        Ok(())
    }

    #[test]
    fn full_k4v4_context_requires_its_own_aligned_source_release() -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        assert!(full_context_supported()?);
        assert_eq!(K4V4_FULL_CONTEXT_RELEASE, MEMORY_CONTEXT_RELEASE);
        assert!(!full_context_release_matches(&pin, None));
        assert!(full_context_release_matches(&pin, MEMORY_CONTEXT_RELEASE));
        for version in ["0.1.27", "0.1.29"] {
            pin.version = version.into();
            assert!(!full_context_release_matches(&pin, MEMORY_CONTEXT_RELEASE));
        }
        pin.version = "0.1.28".into();
        pin.commit = "0".repeat(40);
        assert!(!full_context_release_matches(&pin, MEMORY_CONTEXT_RELEASE));
        let other_source = (pin.version.as_str(), pin.commit.as_str());
        assert!(!full_context_release_matches(&pin, Some(other_source)));
        pin.version = "0.1.26".into();
        pin.commit = "ac941f2668ae93dba9824002b4597081457aa09f".into();
        assert!(!full_context_release_matches(&pin, MEMORY_CONTEXT_RELEASE));
        Ok(())
    }

    #[test]
    fn k4v3_capacity_selection_waits_for_a_separately_reviewed_memory_release()
    -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        assert!(!k4v3_context_supported(&pin));
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4K3v)),
            Err(LaunchError::NativeRuntime)
        );
        assert_eq!(
            KvCache::parse(Some(std::ffi::OsStr::new("rsq4k3v")))?,
            KvCache::Rsq4K3v
        );
        for (version, commit) in [
            ("0.1.25", TURBOQUANT_REVISION),
            ("0.1.24", "8d2b3067e5f17369f579857a180c01a422a58211"),
            ("0.1.26", TURBOQUANT_REVISION),
            ("0.1.26", "0000000000000000000000000000000000000000"),
        ] {
            pin.version = version.into();
            pin.commit = commit.into();
            assert!(!k4v3_context_supported(&pin));
            assert_eq!(
                select_kv_cache(&pin, Some(KvCache::Rsq4K3v)),
                Err(LaunchError::NativeRuntime)
            );
        }
        assert_eq!(
            pinned_kv_cache(false, Some(KvCache::Rsq4K3v)),
            Err(LaunchError::Arguments)
        );
        Ok(())
    }

    #[test]
    fn latest_prompt_reuse_requires_exact_reviewed_release_source() -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        assert!(prompt_reuse_supported(&pin, true).is_ok());
        pin.commit = "0".repeat(40);
        assert_eq!(
            prompt_reuse_supported(&pin, true),
            Err(LaunchError::Artifacts)
        );
        assert!(prompt_reuse_supported(&pin, false).is_ok());
        pin.commit = "9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57".into();
        for version in [
            "0.1.13",
            "0.1.15",
            "0.1.16",
            "0.1.17",
            "0.1.18",
            "0.1.19",
            "0.1.20",
            "0.1.21",
            "0.1.22",
            "0.1.23",
            "0.1.22-dev",
            "v0.1.22",
            "0.1.022",
            "0.1.21-dev",
            "v0.1.21",
            "0.1.021",
            "0.1.20-dev",
            "v0.1.20",
            "0.1.020",
            "0.1.19-dev",
            "v0.1.19",
            "0.1.019",
            "0.1.18-dev",
            "v0.1.18",
            "0.1.018",
            "0.1.17-dev",
            "v0.1.17",
            "0.1.017",
            "0.1.16-dev",
            "v0.1.16",
            "0.1.016",
            "0.1.15-dev",
            "v0.1.15",
            "0.1.015",
            "0.1.14-dev",
            "v0.1.14",
            "0.1.014",
        ] {
            pin.version = version.into();
            assert_eq!(
                prompt_reuse_supported(&pin, true),
                Err(LaunchError::Artifacts)
            );
            assert!(prompt_reuse_supported(&pin, false).is_ok());
        }
        Ok(())
    }

    #[test]
    fn compiled_server_release_is_pinned_and_rejects_path_or_identity_drift()
    -> Result<(), serde_json::Error> {
        assert!(release(LOCK).is_ok());
        let value: serde_json::Value = serde_json::from_str(LOCK)?;
        for (field, replacement) in [
            ("schemaVersion", serde_json::json!(2)),
            ("package", serde_json::json!("other")),
            ("target", serde_json::json!("../escape")),
            ("commit", serde_json::json!("../escape")),
            ("release", serde_json::json!("latest")),
            ("files", serde_json::json!([])),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            assert!(
                release(&serde_json::to_string(&changed)?).is_err(),
                "{field}"
            );
        }
        let mut changed = value.clone();
        changed["files"][0]["name"] = serde_json::json!("../prism-inference-server.exe");
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        for (field, replacement) in [
            ("Repository", serde_json::json!("https://example.com/prism")),
            ("Commit", serde_json::json!("../escape")),
            ("Url", serde_json::json!("https://example.com/source.zip")),
            ("Bytes", serde_json::json!(0)),
            ("Bytes", serde_json::json!(268_435_457_u64)),
            ("Sha256", serde_json::json!("not-a-hash")),
        ] {
            let mut changed = value.clone();
            changed["Source"][field] = replacement;
            assert!(
                release(&serde_json::to_string(&changed)?).is_err(),
                "Source.{field}"
            );
        }
        let mut changed = value.clone();
        changed["Source"] = serde_json::json!([]);
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        let mut changed = value.clone();
        changed["Source"]["unexpected"] = serde_json::json!(true);
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        let mut changed = value;
        changed["asset"]["url"] = serde_json::json!("https://example.com/runtime.zip");
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        Ok(())
    }

    #[test]
    fn four_row_workers_require_the_reviewed_parallel_release() -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        pin.version = "0.1.8".into();
        assert!(row_workers_supported(&pin, 1, MEMORY_CONTEXT_RELEASE).is_ok());
        for version in [
            "0.1.9", "0.1.10", "0.1.11", "0.1.12", "0.1.13", "0.1.14", "0.1.15", "0.1.16",
            "0.1.17", "0.1.18", "0.1.19", "0.1.20", "0.1.21", "0.1.22", "0.1.23", "0.1.24",
            "0.1.25",
        ] {
            pin.version = version.into();
            assert!(row_workers_supported(&pin, 1, MEMORY_CONTEXT_RELEASE).is_ok());
            assert!(row_workers_supported(&pin, 4, MEMORY_CONTEXT_RELEASE).is_ok());
            for workers in [0, 2, 3, 5, 16, 255] {
                assert_eq!(
                    row_workers_supported(&pin, workers, MEMORY_CONTEXT_RELEASE),
                    Err(LaunchError::Artifacts)
                );
            }
        }
        pin.version = "0.1.28".into();
        assert!(row_workers_supported(&pin, 4, MEMORY_CONTEXT_RELEASE).is_ok());
        pin.commit = "0".repeat(40);
        for version in [
            "0.1.8",
            "0.1.26",
            "0.1.27",
            "0.1.28",
            "0.1.22-dev",
            "v0.1.22",
            "0.1.22.0",
            "0.1.022",
            "0.1.21-dev",
            "v0.1.21",
            "0.1.21.0",
            "0.1.021",
            "0.1.20-dev",
            "v0.1.20",
            "0.1.20.0",
            "0.1.020",
            "0.1.19-dev",
            "v0.1.19",
            "0.1.19.0",
            "0.1.019",
            "0.1.18-dev",
            "v0.1.18",
            "0.1.18.0",
            "0.1.018",
            "0.1.17-dev",
            "v0.1.17",
            "0.1.17.0",
            "0.1.017",
            "0.1.16-dev",
            "v0.1.16",
            "0.1.16.0",
            "0.1.016",
            "0.1.15-dev",
            "v0.1.15",
            "0.1.15.0",
            "0.1.015",
            "0.1.14-dev",
            "v0.1.14",
            "0.1.14.0",
            "0.1.014",
            "0.1.13-dev",
            "v0.1.13",
            "0.1.13.0",
            "0.1.013",
            "0.1.9-dev",
            "0.1.10-dev",
            "0.1.11-dev",
            "0.1.12-dev",
            "v0.1.12",
            "v0.1.11",
            "v0.1.10",
            "0.1.12.0",
            "0.1.10.0",
            "",
            "latest",
        ] {
            pin.version = version.into();
            assert_eq!(
                row_workers_supported(&pin, 4, MEMORY_CONTEXT_RELEASE),
                Err(LaunchError::Artifacts)
            );
        }
        Ok(())
    }

    #[test]
    fn reviewed_memory_release_keeps_parallel_quantized_and_reuse_prerequisites()
    -> Result<(), LaunchError> {
        let mut pin = release(LOCK)?;
        let reviewed = Some(("0.1.26", "1111111111111111111111111111111111111111"));
        pin.version = "0.1.26".into();
        pin.commit = "1111111111111111111111111111111111111111".into();
        assert!(reviewed_release_matches(&pin, reviewed));
        assert!(turboquant_supported(&pin, reviewed));
        assert!(row_workers_supported(&pin, 4, reviewed).is_ok());
        for workers in [0, 2, 3, 5, 16, 255] {
            assert_eq!(
                row_workers_supported(&pin, workers, reviewed),
                Err(LaunchError::Artifacts)
            );
        }
        for identity in [
            None,
            Some(("0.1.27", "1111111111111111111111111111111111111111")),
            Some(("0.1.26", TURBOQUANT_REVISION)),
        ] {
            assert!(!reviewed_release_matches(&pin, identity));
            assert!(!turboquant_supported(&pin, identity));
            assert_eq!(
                row_workers_supported(&pin, 4, identity),
                Err(LaunchError::Artifacts)
            );
            assert!(row_workers_supported(&pin, 1, identity).is_ok());
        }
        // Synthetic source review does not open the production gate or change
        // the published pin. Latest reuse shares the TurboQuant source review.
        assert!(!turboquant_supported(&pin, MEMORY_CONTEXT_RELEASE));
        assert_eq!(
            select_kv_cache(&pin, Some(KvCache::Rsq4)),
            Err(LaunchError::NativeRuntime)
        );
        assert_eq!(
            prompt_reuse_supported(&pin, true),
            Err(LaunchError::Artifacts)
        );
        assert!(prompt_reuse_supported(&pin, false).is_ok());
        Ok(())
    }
}
