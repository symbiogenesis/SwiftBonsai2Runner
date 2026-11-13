//! Hosted, source-bound preparation of the original-artifact projection control.

use super::{Context, TARGET, TOOLCHAIN, compile_arguments, plain, write_json};
use crate::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

// Bind the library, its independent controls and the standalone consumer to the
// same closed input set. The packed-only build deliberately retains the extra
// consumer/verifier identities so a new module cannot escape either receipt.
pub(super) const INPUTS: &[&str] = &[
    "projection_probe.rs",
    "packed_projection.rs",
    "packed_tiles.rs",
    "q2_integer.rs",
    "q2_projection.rs",
    "hmx_projection.rs",
    "hmx_projection_tests.rs",
    "ptq1_hmx.rs",
    "ptq1_hmx_tests.rs",
    "packed-sources.json",
    "verify_packed_fixture.ps1",
    "fixtures/swift-q8k-parity.bin",
    "fixtures/swift-q8k-parity.json",
    "fixtures/swift-ptq1-dot-parity.bin",
    "fixtures/swift-ptq1-dot-parity.json",
];

fn dependencies(context: &mut Context, rustc: &Path, output: &Path) -> Result<Vec<OsString>> {
    context.environment.extend(BTreeMap::from([
        ("RUSTC".into(), rustc.to_string_lossy().into_owned()),
        ("RUSTC_WRAPPER".into(), String::new()),
        ("RUSTC_WORKSPACE_WRAPPER".into(), String::new()),
        ("RUSTFLAGS".into(), String::new()),
        ("CARGO_ENCODED_RUSTFLAGS".into(), String::new()),
    ]));
    let cargo = rustc
        .parent()
        .ok_or("Missing compiler directory.")?
        .join("cargo.exe");
    let target = std::env::var_os("BONSAI_TOOLS_TARGET")
        .map(PathBuf::from)
        .ok_or("Hosted projection preparation requires its external Cargo target.")?;
    plain(&target)?;
    bonsai_artifacts::validate_external_path(&target).map_err(|error| error.to_string())?;
    let lock = context.repo.join("Cargo.lock");
    let lock_digest = context.digest(&lock, 16 * 1024 * 1024)?;
    let build = context.run(
        cargo.as_os_str(),
        &[
            "build".into(),
            "--locked".into(),
            "--release".into(),
            "--lib".into(),
            "--target".into(),
            TARGET.into(),
            "--manifest-path".into(),
            context.repo.join("Cargo.toml").into_os_string(),
            "--target-dir".into(),
            target.clone().into_os_string(),
            "--package".into(),
            "bonsai-tools".into(),
            "--message-format=json-render-diagnostics".into(),
        ],
        180,
        &output.join("dependencies-build.txt"),
    )?;
    let mut selected = BTreeMap::<String, PathBuf>::new();
    let mut artifacts = Vec::new();
    let mut directories = BTreeSet::new();
    for line in build.stdout.lines().filter(|line| !line.is_empty()) {
        let message: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let name = message["target"]["name"].as_str().unwrap_or_default();
        let files = message["filenames"]
            .as_array()
            .ok_or("Missing dependency artifacts.")?;
        for filename in files {
            let path = PathBuf::from(filename.as_str().ok_or("Invalid dependency artifact.")?);
            if path
                .extension()
                .is_none_or(|extension| extension != "rlib" && extension != "dll")
            {
                continue;
            }
            if !path.starts_with(&target) {
                return Err("Projection dependency escaped owned target storage.".into());
            }
            directories.insert(
                path.parent()
                    .ok_or("Dependency directory missing.")?
                    .to_owned(),
            );
            if !["serde", "serde_json", "sha2"].contains(&name)
                || path.extension().is_none_or(|extension| extension != "rlib")
                || !target_artifact(&target, &path)?
            {
                continue;
            }
            if selected.insert(name.to_owned(), path.clone()).is_some() {
                return Err("Projection dependency is ambiguous or escaped owned storage.".into());
            }
            let metadata = path.with_extension("rmeta");
            if !files.iter().any(|value| {
                value
                    .as_str()
                    .is_some_and(|value| Path::new(value) == metadata)
            }) {
                return Err("Cargo omitted the projection dependency's companion metadata.".into());
            }
            artifacts.push(
                json!({"Crate":name,"Path":path,"Sha256":context.digest(&path,256*1024*1024)?,"MetadataPath":metadata,"MetadataSha256":context.digest(&metadata,256*1024*1024)?}),
            );
        }
    }
    if selected.len() != 3 {
        return Err("Cargo omitted the exact projection dependencies.".into());
    }
    write_json(
        &output.join("dependencies.json"),
        &json!({"CargoLockSha256":lock_digest,"Artifacts":artifacts,"SearchDirectories":directories}),
    )?;
    Ok(linking_arguments(selected, directories))
}

fn linking_arguments(
    libraries: BTreeMap<String, PathBuf>,
    directories: BTreeSet<PathBuf>,
) -> Vec<OsString> {
    let mut arguments = Vec::new();
    for (name, path) in libraries {
        arguments.extend([
            "--extern".into(),
            format!("{name}={}", path.display()).into(),
            "--extern".into(),
            format!("{name}={}", path.with_extension("rmeta").display()).into(),
        ]);
    }
    for directory in directories {
        arguments.extend([
            "-L".into(),
            format!("dependency={}", directory.display()).into(),
        ]);
    }
    arguments
}

fn target_artifact(target: &Path, path: &Path) -> Result<bool> {
    if !path.starts_with(target) {
        return Err("Projection dependency escaped owned target storage.".into());
    }
    Ok(path.starts_with(target.join(TARGET).join("release")))
}

pub(super) fn build(
    context: &mut Context,
    rustc: &Path,
    manifest: &Path,
    compiler_digest: &str,
    native_library: &Path,
) -> Result<()> {
    let output = context.output.join("projection-probe");
    let bundle = context.output.join("projection-probe-bundle");
    fs::create_dir(&output)
        .and_then(|()| fs::create_dir(&bundle))
        .map_err(|error| error.to_string())?;
    let root = context.repo.join("tools/hexagon");
    let sources = context.input_set(&root, INPUTS)?;
    let native_sources = context.input_set(&root.join("native_rpc"), super::NATIVE_INPUTS)?;
    let manifest_digest = context.digest(manifest, 65_536)?;
    let dispatch_lock_digest = context.digest(
        &context.repo.join("config/hexagon-dispatch.lock.json"),
        1024 * 1024,
    )?;
    let candidate_lock_digest = context.digest(
        &context
            .repo
            .join("config/hexagon-signed-q2-candidate.lock.json"),
        1024 * 1024,
    )?;
    let mut externs = dependencies(context, rustc, &output)?;
    let packed = output.join("libpacked_projection.rlib");
    let mut args = compile_arguments(manifest);
    args.extend([
        "--crate-type=lib".into(),
        "--crate-name=packed_projection".into(),
        root.join("packed_projection.rs").into_os_string(),
        "-o".into(),
        packed.clone().into_os_string(),
    ]);
    context.run(
        rustc.as_os_str(),
        &args,
        120,
        &output.join("packed-library-build.txt"),
    )?;
    externs.extend([
        "--extern".into(),
        format!("packed_projection={}", packed.display()).into(),
        "--extern".into(),
        format!("hexagon_native_rpc={}", native_library.display()).into(),
    ]);
    let executable = bundle.join("hexagon-projection-probe.exe");
    let tests = output.join("hexagon-projection-tests.exe");
    for (path, test, log) in [
        (&executable, false, "probe-build.txt"),
        (&tests, true, "tests-build.txt"),
    ] {
        let mut args = compile_arguments(manifest);
        if test {
            args.push("--test".into());
        }
        args.extend(externs.clone());
        args.extend([
            root.join("projection_probe.rs").into_os_string(),
            "-o".into(),
            path.clone().into_os_string(),
        ]);
        context.run(rustc.as_os_str(), &args, 120, &output.join(log))?;
    }
    context.run(
        executable.as_os_str(),
        &["--self-test".into()],
        15,
        &output.join("self-test.json"),
    )?;
    context.run(
        tests.as_os_str(),
        &[],
        30,
        &output.join("contract-tests.txt"),
    )?;
    let executable = context.executable(&executable, "hexagon-projection-probe.exe")?;
    let dependency_receipt_digest = context.digest(&output.join("dependencies.json"), 65_536)?;
    write_json(
        &bundle.join("projection-build.json"),
        &json!({
            "SchemaVersion":1,"Kind":"hexagon-original-projection-build",
            "RunnerRevision":context.revision,"RunId":context.run,"RunAttempt":context.attempt,
            "RustToolchain":TOOLCHAIN,"Target":TARGET,"HostedBuild":true,
            "CompilerIdentitySha256":compiler_digest,"SourceFiles":sources,
            "NativeSourceFiles":native_sources,"ManifestSha256":manifest_digest,
            "DispatchLockSha256":dispatch_lock_digest,"CandidateLockSha256":candidate_lock_digest,
            "BuildEnvironment":context.environment,"Executable":executable,
            "DependencyReceiptSha256":dependency_receipt_digest,
            "DeviceCodeLoaded":false,"ModelLoaded":false,"ProfileEligible":false,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_linker_receives_both_split_metadata_and_code() {
        let library = PathBuf::from("target/release/build/serde/unit/out/libserde-unit.rlib");
        let arguments = linking_arguments(
            BTreeMap::from([("serde".into(), library.clone())]),
            BTreeSet::new(),
        );
        assert_eq!(
            arguments,
            vec![
                OsString::from("--extern"),
                format!("serde={}", library.display()).into(),
                OsString::from("--extern"),
                format!("serde={}", library.with_extension("rmeta").display()).into()
            ]
        );
    }

    #[test]
    fn projection_dependencies_keep_target_rlibs_separate_from_host_build_helpers() -> Result<()> {
        let root = Path::new("owned-target");
        assert!(!target_artifact(
            root,
            &root.join("release/deps/libsha2-host.rlib")
        )?);
        assert!(target_artifact(
            root,
            &root.join(TARGET).join("release/deps/libsha2-native.rlib")
        )?);
        assert!(target_artifact(
            root,
            &root.join(TARGET).join("release/deps/libserde-native.rlib")
        )?);
        assert!(target_artifact(
            root,
            &root
                .join(TARGET)
                .join("release/build/serde/unit-hash/out/libserde-native.rlib")
        )?);
        assert!(!target_artifact(
            root,
            &root.join("other-target/release/deps/libsha2-other.rlib")
        )?);
        assert!(target_artifact(root, Path::new("outside/libsha2.rlib")).is_err());
        Ok(())
    }
}
