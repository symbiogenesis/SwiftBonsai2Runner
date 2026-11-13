//! Independent dependency syntax and closure omission/tamper controls.

use super::{Generation, Identity, Input, Origin, Unit, dependency_rule, verify_unit};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = fs::canonicalize(std::env::temp_dir())?.join(format!(
            "Bonsai-NativeClosure-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }

    fn file(&self, name: &str, bytes: &[u8]) -> Result<Identity, Box<dyn std::error::Error>> {
        let path = self.0.join(name);
        fs::write(&path, bytes)?;
        let (length, sha256) = super::digest(&path, super::MAX_FILE)?;
        Ok(Identity {
            path: path.to_str().ok_or("Non-UTF8 scratch")?.into(),
            bytes: length,
            sha256,
        })
    }

    fn unit(&self) -> Result<Unit, Box<dyn std::error::Error>> {
        let source = self.file(
            "source.c",
            b"#include \"generated.h\"\nint value = VALUE;\n",
        )?;
        let generated = self.file("generated.h", b"#define VALUE 7\n")?;
        let generator = self.file("generator", b"independent-generator-identity")?;
        let generator_input = self.file("value.txt", b"7\n")?;
        let object = self.file("source.obj", b"independent object identity")?;
        let preprocessed = self.file("source.i", b"int value = 7;\n")?;
        // Literal fixture values are not compiler captures. Escape spaces as Make does.
        let report = format!(
            "{}: {} \\\n {}\n",
            escape(&object.path),
            escape(&source.path),
            escape(&generated.path)
        );
        let dependencies = self.file("source.d", report.as_bytes())?;
        Ok(Unit {
            arguments: vec![
                "clang".into(),
                "-c".into(),
                source.path.clone(),
                "-MD".into(),
                "-MF".into(),
                dependencies.path.clone(),
                "-o".into(),
                object.path.clone(),
            ],
            preprocessing_arguments: vec![
                "clang".into(),
                "-E".into(),
                source.path.clone(),
                "-o".into(),
                preprocessed.path.clone(),
            ],
            compiler_exit_code: 0,
            preprocessor_exit_code: 0,
            source: source.path.clone(),
            working_directory: self.0.to_str().ok_or("Non-UTF8 scratch")?.into(),
            dependencies,
            preprocessed,
            object,
            inputs: vec![
                Input {
                    identity: source,
                    origin: Origin::Source,
                    generation: None,
                },
                Input {
                    identity: generated,
                    origin: Origin::Generated,
                    generation: Some(Generation {
                        arguments: vec![generator.path.clone(), generator_input.path.clone()],
                        generator,
                        inputs: vec![generator_input],
                    }),
                },
            ],
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Ok(parent) = fs::canonicalize(std::env::temp_dir())
            && self.0.parent() == Some(parent.as_path())
            && fs::symlink_metadata(&self.0)
                .is_ok_and(|metadata| !metadata.file_type().is_symlink())
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn escape(path: &str) -> String {
    // canonicalize returns Windows verbatim paths; a Make capture uses ordinary
    // absolute drive paths, not a forward-slash spelling of the NT namespace.
    path.strip_prefix("\\\\?\\")
        .unwrap_or(path)
        .replace('\\', "/")
        .replace('$', "$$")
        .replace(' ', "\\ ")
        .replace('#', "\\#")
}

#[test]
fn native_source_closure_make_literal_windows_and_escaped_paths() -> TestResult {
    let (target, inputs) = dependency_rule(
        "C:/out/a.obj: C:\\src\\a.cpp \\\r\n C:/Program\\ Files/header.h C:/a\\#b$$c.h\n",
    )?;
    assert_eq!(target, "C:/out/a.obj");
    assert_eq!(
        inputs,
        ["C:\\src\\a.cpp", "C:/Program Files/header.h", "C:/a#b$c.h"]
    );
    assert_eq!(
        dependency_rule("/a.o: /a.c \\\n /b.h\n")?.1,
        ["/a.c", "/b.h"]
    );
    Ok(())
}

#[test]
fn native_source_closure_rejects_ambiguous_or_multiple_make_rules() {
    for text in [
        "",
        "/a.o /b.o: /a.c",
        "/a.o:",
        "/a.o: $(SRC)",
        "/a.o: /a.c #hidden",
        "/a.o: /a.c\n/b.h:\n",
        "/a.o: /a.c: /b.h",
        "/a.o: /a\0.c",
    ] {
        assert!(dependency_rule(text).is_err(), "{text:?}");
    }
}

#[test]
fn native_source_closure_exact_generated_inputs_and_tampering() -> TestResult {
    let scratch = Scratch::new()?;
    let mut unit = scratch.unit()?;
    assert_eq!(verify_unit(&unit, "clang", &mut 0)?, 1);
    let header = unit.inputs.pop().ok_or("Missing header fixture")?;
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.inputs.push(header);
    unit.inputs.push(Input {
        identity: scratch.file("unused.h", b"unused")?,
        origin: Origin::Source,
        generation: None,
    });
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.inputs.pop();
    fs::write(&unit.inputs[1].identity.path, b"#define VALUE 8\n")?;
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    Ok(())
}

#[test]
fn native_source_closure_rejects_partial_flags_and_lost_generation() -> TestResult {
    let scratch = Scratch::new()?;
    let mut unit = scratch.unit()?;
    unit.arguments.push("-MMD".into());
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.push("@hidden.rsp".into());
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.push("-o".into());
    unit.arguments.push(unit.object.path.clone());
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.pop();
    unit.preprocessing_arguments.push("-DCHANGED=1".into());
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.preprocessing_arguments.pop();
    unit.arguments[1] = "-E".into();
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    unit.arguments[1] = "-c".into();
    unit.inputs[1].generation = None;
    assert!(verify_unit(&unit, "clang", &mut 0).is_err());
    Ok(())
}

#[test]
fn native_source_closure_manifest_digest_and_unknown_fields() -> TestResult {
    // A changed/unknown envelope rejects before any compiler or inspected code runs.
    let scratch = Scratch::new()?;
    let manifest = scratch.file("manifest.json", br#"{"schemaVersion":1,"unknown":true}"#)?;
    let arguments = |sha: String| {
        vec![
            "native-closure-verify".into(),
            "--manifest".into(),
            manifest.path.clone().into(),
            "--expected-sha256".into(),
            OsString::from(sha),
        ]
    };
    assert!(super::run(&arguments("0".repeat(64))).is_err());
    assert!(super::run(&arguments(manifest.sha256)).is_err());
    Ok(())
}

#[test]
fn native_source_closure_verified_envelope_keeps_retirement_unqualified() -> TestResult {
    fn identity(value: &Identity) -> serde_json::Value {
        serde_json::json!({"path": value.path, "bytes": value.bytes, "sha256": value.sha256})
    }
    let scratch = Scratch::new()?;
    let mut unit = scratch.unit()?;
    let compiler = scratch.file("clang", b"independent compiler identity")?;
    unit.arguments[0].clone_from(&compiler.path);
    unit.preprocessing_arguments[0].clone_from(&compiler.path);
    let inputs = unit.inputs.iter().map(|input| {
        let generation = input.generation.as_ref().map(|generation| serde_json::json!({
            "generator": identity(&generation.generator), "arguments": generation.arguments,
            "inputs": generation.inputs.iter().map(identity).collect::<Vec<_>>()
        }));
        serde_json::json!({"identity": identity(&input.identity),
            "origin": if generation.is_some() {"generated"} else {"source"}, "generation": generation})
    }).collect::<Vec<_>>();
    let document = serde_json::json!({"schemaVersion": 1,
        "kind": "selected-clang-native-source-closure", "sourceRevision": "a".repeat(40),
        "target": "aarch64-pc-windows-msvc", "configuration": "Release CPU",
        "compiler": identity(&compiler), "units": [{"source": unit.source,
            "workingDirectory": unit.working_directory, "arguments": unit.arguments,
            "preprocessingArguments": unit.preprocessing_arguments,
            "compilerExitCode": 0, "preprocessorExitCode": 0,
            "dependencies": identity(&unit.dependencies), "preprocessed": identity(&unit.preprocessed),
            "object": identity(&unit.object), "inputs": inputs}]
    });
    let manifest = scratch.file("manifest.json", &serde_json::to_vec(&document)?)?;
    let receipt: serde_json::Value = serde_json::from_str(&super::run(&[
        "native-closure-verify".into(),
        "--manifest".into(),
        manifest.path.into(),
        "--expected-sha256".into(),
        manifest.sha256.into(),
    ])?)?;
    assert_eq!(receipt["translationUnits"], 1);
    assert_eq!(receipt["generatedInputs"], 1);
    assert_eq!(receipt["freshCompilerExecution"], false);
    assert_eq!(receipt["completeBuildClosureQualified"], false);
    assert_eq!(receipt["nativeRetirementQualified"], false);
    Ok(())
}

use std::ffi::OsString;
