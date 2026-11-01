//! Native package lifecycle and rejection controls, using inert ARM64 headers.

use super::*;
use serde_json::json;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> crate::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-package-lifecycle-{}",
            crate::startup_process::unique()
        ));
        external(&root)?;
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let mut image = vec![0; 512];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&128_u32.to_le_bytes());
        image[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        fs::write(root.join("input.exe"), image).map_err(|error| error.to_string())?;
        fs::write(root.join("notices.txt"), b"Reviewed fixture notices.")
            .map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn package(&self, version: &str) -> crate::Result<Value> {
        let binary = self.0.join("input.exe");
        super::super::run(json!({
            "operation": "create", "outputDirectory": self.0.join(version), "version": version,
            "sourceRevision": "a".repeat(40), "chatPath": binary, "brokerPath": binary,
            "asrPath": binary, "ttsPath": binary, "toolPath": binary,
            "thirdPartyNoticePath": self.0.join("notices.txt")
        }))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if external(&self.0).is_ok() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn path(package: &Value) -> crate::Result<PathBuf> {
    package["Directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "Missing package directory.".into())
}

fn activate_package(root: &Path, package: &Value) -> crate::Result<Value> {
    activate(
        root,
        Some((
            &path(package)?,
            package["ManifestSha256"]
                .as_str()
                .ok_or("Missing digest.")?,
        )),
    )
}

#[test]
fn create_install_update_noop_and_bidirectional_rollback() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    let root = fixture.0.join("installation");
    let first = fixture.package("0.1.0")?;
    assert_eq!(first["Manifest"]["schemaVersion"], 2);
    assert_eq!(
        first["Manifest"]["files"]
            .as_array()
            .ok_or("Missing inventory.")?
            .len(),
        8
    );
    assert!(!path(&first)?.join("scripts").exists());
    let installed_first = activate_package(&root, &first)?;
    assert_eq!(installed_first["Id"], first["Id"]);
    assert!(activate(&root, None).is_err());
    let second = fixture.package("0.2.0")?;
    activate_package(&root, &second)?;
    let selection = fs::read(root.join("active.json")).map_err(|error| error.to_string())?;
    let digest = second["ManifestSha256"]
        .as_str()
        .ok_or("Missing digest.")?
        .to_ascii_uppercase();
    activate(&root, Some((&path(&second)?, &digest)))?;
    assert_eq!(
        fs::read(root.join("active.json")).map_err(|error| error.to_string())?,
        selection
    );
    assert_eq!(activate(&root, None)?["Id"], first["Id"]);
    assert_eq!(activate(&root, None)?["Id"], second["Id"]);
    assert!(fixture.package("0.1.0").is_err());
    assert_eq!(
        fs::read(root.join("active.json")).map_err(|error| error.to_string())?,
        selection
    );
    Ok(())
}

#[test]
fn activation_rejects_tampering_and_contention_without_changing_selection() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    let root = fixture.0.join("installation");
    let first = fixture.package("0.1.0")?;
    let installed_first = activate_package(&root, &first)?;
    let second = fixture.package("0.2.0")?;
    activate_package(&root, &second)?;
    let selection = fs::read(root.join("active.json")).map_err(|error| error.to_string())?;
    let guard = crate::startup_cache::lock_file(&root.join("package.lock"))?;
    assert!(activate_package(&root, &first).is_err());
    drop(guard);
    fs::write(path(&first)?.join("NOTICE"), b"changed").map_err(|error| error.to_string())?;
    assert!(activate_package(&root, &first).is_err());
    fs::write(path(&installed_first)?.join("NOTICE"), b"changed")
        .map_err(|error| error.to_string())?;
    assert!(activate(&root, None).is_err());
    assert_eq!(
        fs::read(root.join("active.json")).map_err(|error| error.to_string())?,
        selection
    );
    for extra in ["bin/loader.dll", "bin/bonsai-tools.exe.local", "scripts"] {
        let extra_path = path(&second)?.join(extra);
        fs::write(&extra_path, b"unexpected").map_err(|error| error.to_string())?;
        assert!(activate_package(&root, &second).is_err());
        fs::remove_file(extra_path).map_err(|error| error.to_string())?;
    }
    assert!(activate(Path::new("relative"), None).is_err());
    assert!(activate(&root, Some((&path(&second)?, &"0".repeat(64)))).is_err());
    Ok(())
}

#[test]
fn source_inputs_and_metadata_fail_before_publication() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    fs::write(fixture.0.join("input.exe"), b"not an ARM64 executable")
        .map_err(|error| error.to_string())?;
    assert!(fixture.package("0.1.0").is_err());
    assert!(!fixture.0.join("0.1.0").exists());
    assert!(fixture.package("../escape").is_err());
    assert!(
        fs::read_dir(&fixture.0)
            .map_err(|error| error.to_string())?
            .all(|entry| entry.is_ok_and(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with("package-stage-")))
    );
    assert!(
        super::super::run(json!({
            "operation": "install", "installationDirectory": fixture.0.join("install"),
            "packageDirectory": fixture.0, "manifestSha256": "f".repeat(64), "executeHook": true,
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn native_installer_retains_legacy_package_rollback() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    let legacy = fixture.package("0.1.0")?;
    let directory = path(&legacy)?;
    fs::create_dir(directory.join("scripts")).map_err(|error| error.to_string())?;
    let mut manifest: Manifest =
        serde_json::from_value(legacy["Manifest"].clone()).map_err(|error| error.to_string())?;
    manifest.schema_version = 1;
    for name in super::super::LEGACY_FILES
        .iter()
        .filter(|name| name.starts_with("scripts/"))
    {
        fs::write(directory.join(name), b"legacy fixture").map_err(|error| error.to_string())?;
        manifest.files.push(PackageFile {
            name: (*name).into(),
            bytes: 14,
            sha256: crate::identity::sha256(b"legacy fixture"),
        });
    }
    let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    fs::write(directory.join("package.json"), &bytes).map_err(|error| error.to_string())?;
    let root = fixture.0.join("installation");
    let installed_legacy = activate(&root, Some((&directory, &crate::identity::sha256(&bytes))))?;
    let native = fixture.package("0.2.0")?;
    activate_package(&root, &native)?;
    assert_eq!(activate(&root, None)?["Id"], installed_legacy["Id"]);
    assert_eq!(activate(&root, None)?["Id"], native["Id"]);
    Ok(())
}
