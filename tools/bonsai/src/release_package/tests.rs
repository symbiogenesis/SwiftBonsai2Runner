//! Source-bound release intake and PE controls without executing image fixtures.
use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "Bonsai-ReleasePackage-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lane_intake_requires_every_exact_source_target_extent_and_digest() -> Result<()> {
    let fixture = Fixture::new()?;
    let revision = "a".repeat(40);
    for (lane, name) in FILES {
        let directory = fixture.0.join(format!("release-build-{lane}"));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(name);
        fs::write(&path, b"inert release lane").map_err(|error| error.to_string())?;
        save(
            &directory.join("build-manifest.json"),
            &json!({"schemaVersion":1,"sourceRevision":revision,"target":TARGET,"lane":lane,"file":name,"bytes":18,"sha256":crate::startup_cache::digest_file(&path)?}),
        )?;
    }
    let (_, builds) = checked_builds(&fixture.0, &revision)?;
    require(builds.len() == 5, "Lane inventory changed.")?;
    let path = fixture.0.join("release-build-core/build-manifest.json");
    let original = crate::release_tool::read_json(&path)?;
    for (field, value) in [
        ("lane", json!("Core")),
        ("target", json!("x86_64-pc-windows-msvc")),
        ("bytes", json!(18.0)),
        ("sourceRevision", json!("b".repeat(40))),
        ("sha256", json!("0".repeat(64))),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        fs::write(
            &path,
            serde_json::to_vec(&changed).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        require(
            checked_builds(&fixture.0, &revision).is_err(),
            "Foreign lane receipt admitted.",
        )?;
    }
    fs::write(
        &path,
        serde_json::to_vec(&original).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        fixture.0.join("release-build-core/bonsai-broker.exe"),
        b"changed release lane",
    )
    .map_err(|error| error.to_string())?;
    require(
        checked_builds(&fixture.0, &revision).is_err(),
        "Changed executable admitted.",
    )
}

fn image() -> Vec<u8> {
    let mut image = vec![0; 174];
    image[..2].copy_from_slice(b"MZ");
    image[60..64].copy_from_slice(&80_u32.to_le_bytes());
    image[80..86].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
    image[100..102].copy_from_slice(&70_u16.to_le_bytes());
    image[104..106].copy_from_slice(&0x20b_u16.to_le_bytes());
    image[172..174].copy_from_slice(&2_u16.to_le_bytes());
    image
}

#[test]
fn smoke_checks_arm64_pe32plus_gui_header_before_launch_or_evidence_creation() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.0.join("inert.exe");
    let valid = image();
    fs::write(&path, &valid).map_err(|error| error.to_string())?;
    crate::chat_package::bundle::require_windowless(&mut crate::build_tools::input_file(&path)?)?;
    for (offset, replacement) in [(0, 0u8), (84, 0x66), (104, 0), (172, 3)] {
        let mut changed = valid.clone();
        changed[offset] = replacement;
        fs::write(&path, changed).map_err(|error| error.to_string())?;
        require(
            crate::chat_package::bundle::require_windowless(&mut crate::build_tools::input_file(
                &path,
            )?)
            .is_err(),
            "Invalid GUI image admitted.",
        )?;
    }
    fs::write(&path, &valid[..173]).map_err(|error| error.to_string())?;
    require(
        crate::chat_package::bundle::require_windowless(&mut crate::build_tools::input_file(
            &path,
        )?)
        .is_err(),
        "Truncated PE image admitted.",
    )?;
    require(
        !fixture.0.join("evidence").exists(),
        "Header controls launched a fixture.",
    )
}

#[test]
fn cleanup_failure_preserves_primary_and_invalid_requests_are_rejected() -> Result<()> {
    let error = completed::<()>(Err("primary failure".into()), Err("cleanup failure".into()))
        .err()
        .ok_or("Missing failure.")?;
    require(
        error.contains("primary failure") && error.contains("cleanup failure"),
        "Failure context lost.",
    )?;
    require(
        run(json!({"operation":"executable-smoke","unexpected":true})).is_err(),
        "Unknown package field admitted.",
    )
}
