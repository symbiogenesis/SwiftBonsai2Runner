//! Independent dependency syntax and closure omission/tamper controls.

use super::{
    Generation, GenerationInput, Identity, Input, Origin, Unit, dependency_rule, verify_unit,
};
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
                        inputs: vec![GenerationInput::Legacy(generator_input)],
                        working_directory: None,
                        exit_code: None,
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
    assert_eq!(verify_unit(&unit, "clang", 1, &mut 0)?, 1);
    let header = unit.inputs.pop().ok_or("Missing header fixture")?;
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.inputs.push(header);
    unit.inputs.push(Input {
        identity: scratch.file("unused.h", b"unused")?,
        origin: Origin::Source,
        generation: None,
    });
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.inputs.pop();
    fs::write(&unit.inputs[1].identity.path, b"#define VALUE 8\n")?;
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    Ok(())
}

#[test]
fn native_source_closure_rejects_partial_flags_and_lost_generation() -> TestResult {
    let scratch = Scratch::new()?;
    let mut unit = scratch.unit()?;
    unit.arguments.push("-MMD".into());
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.push("@hidden.rsp".into());
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.push("-o".into());
    unit.arguments.push(unit.object.path.clone());
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.arguments.pop();
    unit.arguments.pop();
    unit.preprocessing_arguments.push("-DCHANGED=1".into());
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.preprocessing_arguments.pop();
    unit.arguments[1] = "-E".into();
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    unit.arguments[1] = "-c".into();
    unit.inputs[1].generation = None;
    assert!(verify_unit(&unit, "clang", 1, &mut 0).is_err());
    Ok(())
}

fn recursive_unit(scratch: &Scratch) -> Result<Unit, Box<dyn std::error::Error>> {
    let mut unit = scratch.unit()?;
    let intermediate = scratch.file("intermediate.txt", b"7\n")?;
    let original_input = scratch.file("value.txt", b"7\n")?;
    let generator = scratch.file("generator", b"independent-generator-identity")?;
    let generation = unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?;
    generation.arguments[1].clone_from(&intermediate.path);
    generation.working_directory = Some(unit.working_directory.clone());
    generation.exit_code = Some(0);
    generation.inputs = vec![GenerationInput::Classified(Input {
        identity: intermediate,
        origin: Origin::Generated,
        generation: Some(Generation {
            arguments: vec![generator.path.clone(), original_input.path.clone()],
            generator,
            inputs: vec![GenerationInput::Classified(Input {
                identity: original_input,
                origin: Origin::Source,
                generation: None,
            })],
            working_directory: Some(unit.working_directory.clone()),
            exit_code: Some(0),
        }),
    })];
    Ok(unit)
}

#[test]
fn native_source_closure_recursive_generation_binds_transitive_bytes() -> TestResult {
    let scratch = Scratch::new()?;
    let unit = recursive_unit(&scratch)?;
    assert_eq!(verify_unit(&unit, "clang", 2, &mut 0)?, 2);
    // This source is not a compiler dependency or direct generator input. Its
    // bytes still determine the intermediate that produced the included header.
    fs::write(scratch.0.join("value.txt"), b"8\n")?;
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    Ok(())
}

#[test]
fn native_source_closure_recursive_generation_rejects_missing_or_failed_steps() -> TestResult {
    let scratch = Scratch::new()?;
    let unit = scratch.unit()?;
    // Legacy immediate-input captures retain their old scope; they cannot stand
    // in for the stricter recursive schema merely by changing the version.
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    let mut unit = recursive_unit(&scratch)?;
    let generation = unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?;
    let GenerationInput::Classified(intermediate) = &mut generation.inputs[0] else {
        return Err("Missing intermediate fixture".into());
    };
    intermediate
        .generation
        .as_mut()
        .ok_or("Missing nested generation")?
        .exit_code = Some(1);
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    let mut unit = recursive_unit(&scratch)?;
    let generation = unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?;
    let GenerationInput::Classified(intermediate) = &mut generation.inputs[0] else {
        return Err("Missing intermediate fixture".into());
    };
    intermediate.generation = None;
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    let mut unit = recursive_unit(&scratch)?;
    unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?
        .working_directory = None;
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    Ok(())
}

#[test]
fn native_source_closure_recursive_generation_rejects_ancestor_aliases() -> TestResult {
    let scratch = Scratch::new()?;
    let mut unit = recursive_unit(&scratch)?;
    let ancestor = scratch.file("generated.h", b"#define VALUE 7\n")?;
    let generation = unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?;
    let GenerationInput::Classified(intermediate) = &mut generation.inputs[0] else {
        return Err("Missing intermediate fixture".into());
    };
    intermediate
        .generation
        .as_mut()
        .ok_or("Missing nested generation")?
        .inputs = vec![GenerationInput::Classified(Input {
        identity: ancestor,
        origin: Origin::Source,
        generation: None,
    })];
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
    let mut unit = recursive_unit(&scratch)?;
    let object = scratch.file("source.obj", b"independent object identity")?;
    let generation = unit.inputs[1]
        .generation
        .as_mut()
        .ok_or("Missing generation")?;
    let GenerationInput::Classified(intermediate) = &mut generation.inputs[0] else {
        return Err("Missing intermediate fixture".into());
    };
    intermediate
        .generation
        .as_mut()
        .ok_or("Missing nested generation")?
        .inputs = vec![GenerationInput::Classified(Input {
        identity: object,
        origin: Origin::Source,
        generation: None,
    })];
    assert!(verify_unit(&unit, "clang", 2, &mut 0).is_err());
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

fn identity(value: &Identity) -> serde_json::Value {
    serde_json::json!({"path": value.path, "bytes": value.bytes, "sha256": value.sha256})
}

fn input_value(input: &Input) -> serde_json::Value {
    let generation = input.generation.as_ref().map(|generation| {
        let mut document = serde_json::json!({
            "generator": identity(&generation.generator), "arguments": generation.arguments,
            "inputs": generation.inputs.iter().map(|input| match input {
                GenerationInput::Legacy(value) => identity(value),
                GenerationInput::Classified(value) => input_value(value),
            }).collect::<Vec<_>>()
        });
        if let Some(directory) = &generation.working_directory {
            document["workingDirectory"] = directory.clone().into();
        }
        if let Some(code) = generation.exit_code {
            document["exitCode"] = code.into();
        }
        document
    });
    serde_json::json!({"identity": identity(&input.identity),
        "origin": match input.origin {
            Origin::Source => "source", Origin::Toolchain => "toolchain", Origin::Generated => "generated",
        }, "generation": generation})
}

fn verify_envelope(schema: u32) -> TestResult {
    let scratch = Scratch::new()?;
    let mut unit = if schema >= 2 {
        recursive_unit(&scratch)?
    } else {
        scratch.unit()?
    };
    let compiler = scratch.file("clang", b"independent compiler identity")?;
    unit.arguments[0].clone_from(&compiler.path);
    unit.preprocessing_arguments[0].clone_from(&compiler.path);
    let inputs = unit.inputs.iter().map(input_value).collect::<Vec<_>>();
    let mut document = serde_json::json!({"schemaVersion": schema,
        "kind": "selected-clang-native-source-closure", "sourceRevision": "a".repeat(40),
        "target": "aarch64-pc-windows-msvc", "configuration": "Release CPU",
        "compiler": identity(&compiler), "units": [{"source": unit.source,
            "workingDirectory": unit.working_directory, "arguments": unit.arguments,
            "preprocessingArguments": unit.preprocessing_arguments,
            "compilerExitCode": 0, "preprocessorExitCode": 0,
            "dependencies": identity(&unit.dependencies), "preprocessed": identity(&unit.preprocessed),
            "object": identity(&unit.object), "inputs": inputs}]
    });
    if schema >= 3 {
        let database = scratch.file(
            "compile_commands.json",
            &serde_json::to_vec(&[compilation(&unit)])?,
        )?;
        document["compilationDatabase"] = identity(&database);
    }
    let manifest = scratch.file("manifest.json", &serde_json::to_vec(&document)?)?;
    let receipt: serde_json::Value = serde_json::from_str(&super::run(&[
        "native-closure-verify".into(),
        "--manifest".into(),
        manifest.path.into(),
        "--expected-sha256".into(),
        manifest.sha256.into(),
    ])?)?;
    assert_eq!(receipt["translationUnits"], 1);
    assert_eq!(receipt["generatedInputs"], if schema >= 2 { 2 } else { 1 });
    assert_eq!(receipt["manifestSchemaVersion"], schema);
    assert_eq!(receipt["recursiveGenerationProvenanceChecked"], schema >= 2);
    assert_eq!(receipt["selectedBuildInventoryChecked"], schema >= 3);
    assert_eq!(
        receipt["compilationDatabaseSha256"].is_string(),
        schema >= 3
    );
    assert_eq!(receipt["freshCompilerExecution"], false);
    assert_eq!(receipt["completeBuildClosureQualified"], false);
    assert_eq!(receipt["nativeRetirementQualified"], false);
    Ok(())
}

#[test]
fn native_source_closure_verified_envelope_keeps_retirement_unqualified() -> TestResult {
    verify_envelope(1)
}

#[test]
fn native_source_closure_recursive_envelope_keeps_retirement_unqualified() -> TestResult {
    verify_envelope(2)
}

#[test]
fn native_source_closure_inventory_envelope_keeps_retirement_unqualified() -> TestResult {
    verify_envelope(3)
}

fn compilation(unit: &Unit) -> serde_json::Value {
    serde_json::json!({"directory": unit.working_directory, "file": unit.source,
        "arguments": unit.arguments, "output": unit.object.path})
}

fn inventory_manifest(scratch: &Scratch) -> Result<super::Manifest, Box<dyn std::error::Error>> {
    let compiler = scratch.file("clang", b"independent compiler identity")?;
    let mut unit = recursive_unit(scratch)?;
    unit.arguments[0].clone_from(&compiler.path);
    unit.preprocessing_arguments[0].clone_from(&compiler.path);
    Ok(super::Manifest {
        schema_version: 3,
        kind: "selected-clang-native-source-closure".into(),
        source_revision: "a".repeat(40),
        target: "aarch64-pc-windows-msvc".into(),
        configuration: "Release CPU".into(),
        compiler,
        compilation_database: None,
        units: vec![unit],
    })
}

fn database(
    scratch: &Scratch,
    manifest: &mut super::Manifest,
    value: &serde_json::Value,
) -> TestResult {
    manifest.compilation_database =
        Some(scratch.file("compile_commands.json", &serde_json::to_vec(value)?)?);
    Ok(())
}

#[test]
fn native_source_closure_inventory_rejects_omitted_extra_changed_and_duplicate_units() -> TestResult
{
    let scratch = Scratch::new()?;
    let mut manifest = inventory_manifest(&scratch)?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    let entry = compilation(&manifest.units[0]);
    database(&scratch, &mut manifest, &serde_json::json!([entry]))?;
    super::verify_compilation_database(&manifest, &mut 0)?;
    database(&scratch, &mut manifest, &serde_json::json!([]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    database(&scratch, &mut manifest, &serde_json::json!([entry, entry]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    let mut changed = entry.clone();
    changed["arguments"]
        .as_array_mut()
        .ok_or("Missing arguments")?
        .push("-DOTHER_CONFIGURATION=1".into());
    database(&scratch, &mut manifest, &serde_json::json!([changed]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    database(&scratch, &mut manifest, &serde_json::json!([entry]))?;
    // The count alone is insufficient: repeated invocations cannot fill a
    // missing distinct configuration even when the translation unit is shared.
    let mut second = recursive_unit(&scratch)?;
    second.arguments[0].clone_from(&manifest.compiler.path);
    second.preprocessing_arguments[0].clone_from(&manifest.compiler.path);
    second.object = scratch.file("source-alt.obj", b"independent alternative object identity")?;
    second.preprocessed = scratch.file("source-alt.i", b"int value = 7;\n")?;
    second.dependencies = scratch.file(
        "source-alt.d",
        format!(
            "{}: {} {}\n",
            escape(&second.object.path),
            escape(&second.source),
            escape(&second.inputs[1].identity.path)
        )
        .as_bytes(),
    )?;
    second.arguments[5].clone_from(&second.dependencies.path);
    second.arguments[7].clone_from(&second.object.path);
    second.preprocessing_arguments[4].clone_from(&second.preprocessed.path);
    second.arguments.push("-DOTHER_CONFIGURATION=1".into());
    second
        .preprocessing_arguments
        .push("-DOTHER_CONFIGURATION=1".into());
    verify_unit(&second, &manifest.compiler.path, 3, &mut 0)?;
    manifest.units.push(second);
    database(&scratch, &mut manifest, &serde_json::json!([entry, entry]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    let second_entry = compilation(&manifest.units[1]);
    database(
        &scratch,
        &mut manifest,
        &serde_json::json!([second_entry, entry]),
    )?;
    super::verify_compilation_database(&manifest, &mut 0)?;
    Ok(())
}

#[test]
fn native_source_closure_inventory_rejects_shell_output_tamper_and_legacy_claims() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = inventory_manifest(&scratch)?;
    let entry = compilation(&manifest.units[0]);
    let mut changed = entry.clone();
    changed["output"] = manifest.units[0].preprocessed.path.clone().into();
    database(&scratch, &mut manifest, &serde_json::json!([changed]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    let mut changed = entry.clone();
    changed
        .as_object_mut()
        .ok_or("Missing entry object")?
        .remove("arguments");
    changed["command"] = "clang source.c -o source.obj".into();
    database(&scratch, &mut manifest, &serde_json::json!([changed]))?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    database(&scratch, &mut manifest, &serde_json::json!([entry]))?;
    manifest.schema_version = 2;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    manifest.schema_version = 3;
    fs::write(scratch.0.join("compile_commands.json"), b"[]")?;
    assert!(super::verify_compilation_database(&manifest, &mut 0).is_err());
    Ok(())
}

use std::ffi::OsString;
