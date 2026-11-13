//! Verify selected compiler dependency closures without executing inspected code.

use std::{collections::BTreeSet, ffi::OsString, fs, path::Path};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::native_source_inventory::{digest, hex, lower_hex, plain_ancestors, read_bounded};
use crate::Result;

const USAGE: &str =
    "Usage: bonsai-ci native-closure-verify --manifest PATH --expected-sha256 SHA256";
const MAX_DOCUMENT: u64 = 16 * 1024 * 1024;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Generation {
    generator: Identity,
    arguments: Vec<String>,
    inputs: Vec<Identity>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    identity: Identity,
    origin: Origin,
    generation: Option<Generation>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    Source,
    Toolchain,
    Generated,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Unit {
    source: String,
    working_directory: String,
    arguments: Vec<String>,
    preprocessing_arguments: Vec<String>,
    compiler_exit_code: i32,
    preprocessor_exit_code: i32,
    dependencies: Identity,
    preprocessed: Identity,
    object: Identity,
    inputs: Vec<Input>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    kind: String,
    source_revision: String,
    target: String,
    configuration: String,
    compiler: Identity,
    units: Vec<Unit>,
}

fn path_key(path: &str) -> Result<String> {
    let native = Path::new(path);
    if path.len() > 4096
        || !path.is_ascii()
        || path.bytes().any(|byte| byte.is_ascii_control())
        || !native.is_absolute()
        || native.components().any(|part| {
            matches!(
                part,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err("Native closure requires ordinary absolute ASCII paths.".into());
    }
    plain_ancestors(native)?;
    let canonical = fs::canonicalize(native).map_err(|error| error.to_string())?;
    let key = canonical
        .to_str()
        .ok_or("Native closure path is not UTF-8.")?
        .replace('\\', "/");
    #[cfg(windows)]
    let key = key.to_ascii_lowercase();
    Ok(key)
}

fn verify_identity(identity: &Identity, total: &mut u64) -> Result<String> {
    if !lower_hex(&identity.sha256, 64) || identity.bytes > MAX_FILE {
        return Err("Native closure has an invalid file identity.".into());
    }
    *total = total
        .checked_add(identity.bytes)
        .filter(|bytes| *bytes <= MAX_TOTAL)
        .ok_or("Native closure exceeded its aggregate byte bound.")?;
    let key = path_key(&identity.path)?;
    if digest(Path::new(&identity.path), MAX_FILE)? != (identity.bytes, identity.sha256.clone()) {
        return Err(format!(
            "Native closure file identity changed: {}",
            identity.path
        ));
    }
    Ok(key)
}

// A deliberately narrow single-rule Make depfile reader: no variables, comments,
// phony rules or missing/system-header exclusions. Preserve raw Windows separators
// and drive colons, escaped spaces/hash/dollar, and LF/CRLF continuations.
fn dependency_rule(text: &str) -> Result<(String, Vec<String>)> {
    if !text.is_ascii() || text.is_empty() {
        return Err("Native dependency report must be nonempty ASCII.".into());
    }
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut separator = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'\\' if bytes.get(index + 1) == Some(&b'\n') => {
                index += 1;
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            b'\\' if bytes.get(index + 1..index + 3) == Some(b"\r\n") => {
                index += 2;
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            b'\\'
                if bytes
                    .get(index + 1)
                    .is_some_and(|next| matches!(next, b' ' | b'\t' | b'#' | b':' | b'\\')) =>
            {
                index += 1;
                token.push(char::from(bytes[index]));
            }
            b'$' if bytes.get(index + 1) == Some(&b'$') => {
                index += 1;
                token.push('$');
            }
            b'$' | b'#' | 0 => return Err("Unsupported Make dependency expression.".into()),
            b':' if !(token.len() == 1
                && token.as_bytes()[0].is_ascii_alphabetic()
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| matches!(next, b'/' | b'\\'))) =>
            {
                if separator.is_some() {
                    return Err("Native dependency report must contain one rule.".into());
                }
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                separator = Some(tokens.len());
            }
            b'\r' | b'\n' => {
                if !bytes[index..].iter().all(u8::is_ascii_whitespace) {
                    return Err("Native dependency report must contain one rule.".into());
                }
                break;
            }
            b' ' | b'\t' => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            byte if byte.is_ascii_control() => {
                return Err("Native dependency report contains a control byte.".into());
            }
            _ => token.push(char::from(byte)),
        }
        index += 1;
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    if separator != Some(1) || tokens.len() < 2 {
        return Err("Native dependency report needs one target and its inputs.".into());
    }
    Ok((tokens.remove(0), tokens))
}

fn arguments_valid(arguments: &[String], compiler: &str) -> bool {
    arguments.first().is_some_and(|first| first == compiler)
        && (2..=4096).contains(&arguments.len())
        && arguments
            .iter()
            .all(|arg| arg.len() <= 16_384 && !arg.bytes().any(|byte| byte.is_ascii_control()))
}

fn configuration(unit: &Unit, preprocess: bool) -> Result<Vec<&str>> {
    let args = if preprocess {
        &unit.preprocessing_arguments
    } else {
        &unit.arguments
    };
    let output = if preprocess {
        &unit.preprocessed.path
    } else {
        &unit.object.path
    };
    let mut selected = BTreeSet::new();
    let mut common = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        if ["-c", "-E", "-MD", "-MF", "-MT", "-o"].contains(&arg) {
            if !selected.insert(arg) {
                return Err("Native closure command repeats an action/output option.".into());
            }
            if ["-MF", "-MT", "-o"].contains(&arg) {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or("Missing native compiler option value.")?;
                let expected = match arg {
                    "-MF" => &unit.dependencies.path,
                    "-MT" => &unit.object.path,
                    _ => output,
                };
                if value != expected || (preprocess && arg != "-o") {
                    return Err("Native closure command has a different output identity.".into());
                }
            }
        } else {
            if arg.starts_with('@')
                || ["-MF", "-MT", "-MQ", "-o", "-Wp,", "--config", "--output"]
                    .iter()
                    .any(|prefix| arg.starts_with(prefix))
                || matches!(
                    arg,
                    "--" | "-S"
                        | "-M"
                        | "-MMD"
                        | "-MM"
                        | "-MG"
                        | "-MP"
                        | "-MV"
                        | "-Xclang"
                        | "-Xpreprocessor"
                        | "-fsyntax-only"
                        | "--preprocess"
                        | "--assemble"
                        | "--compile"
                )
            {
                return Err("Unsupported native closure command option.".into());
            }
            common.push(arg);
        }
        index += 1;
    }
    if !selected.contains("-o")
        || common.iter().filter(|arg| **arg == unit.source).count() != 1
        || if preprocess {
            !selected.contains("-E") || selected.contains("-c") || selected.contains("-MD")
        } else {
            !selected.contains("-c")
                || !selected.contains("-MD")
                || !selected.contains("-MF")
                || selected.contains("-E")
        }
    {
        return Err("Native closure requires explicit compile and preprocessing actions.".into());
    }
    Ok(common)
}

fn verify_command(unit: &Unit, compiler: &str) -> Result<()> {
    if !(1..=8192).contains(&unit.inputs.len())
        || unit.compiler_exit_code != 0
        || unit.preprocessor_exit_code != 0
        || !arguments_valid(&unit.arguments, compiler)
        || !arguments_valid(&unit.preprocessing_arguments, compiler)
        || configuration(unit, false)? != configuration(unit, true)?
    {
        return Err("Native closure requires successful Clang compile/preprocessing with identical configuration.".into());
    }
    Ok(())
}

fn verify_unit(unit: &Unit, compiler: &str, total: &mut u64) -> Result<usize> {
    verify_command(unit, compiler)?;
    path_key(&unit.working_directory)?;
    if !Path::new(&unit.working_directory).is_dir() {
        return Err("Native closure working directory is not a directory.".into());
    }
    let source = path_key(&unit.source)?;
    let object = verify_identity(&unit.object, total)?;
    let report = verify_identity(&unit.dependencies, total)?;
    let preprocessed = verify_identity(&unit.preprocessed, total)?;
    if BTreeSet::from([&source, &object, &report, &preprocessed]).len() != 4
        || unit.preprocessed.bytes == 0
        || unit.object.bytes == 0
    {
        return Err("Native closure requires distinct nonempty compilation outputs.".into());
    }
    let text = String::from_utf8(read_bounded(
        Path::new(&unit.dependencies.path),
        MAX_DOCUMENT,
    )?)
    .map_err(|error| error.to_string())?;
    if u64::try_from(text.len()).map_err(|error| error.to_string())? != unit.dependencies.bytes
        || hex(&Sha256::digest(text.as_bytes())) != unit.dependencies.sha256
    {
        return Err("Native dependency report changed before parsing.".into());
    }
    let (target, dependencies) = dependency_rule(&text)?;
    if path_key(&target)? != object {
        return Err("Native dependency target differs from the compiled object.".into());
    }
    let mut expected = BTreeSet::new();
    for dependency in dependencies {
        if !expected.insert(path_key(&dependency)?) {
            return Err("Native dependency report contains duplicate/aliased inputs.".into());
        }
    }
    if !expected.contains(&source) {
        return Err("Native dependency report omits its translation unit.".into());
    }
    let mut actual = BTreeSet::new();
    let mut generated = 0;
    for input in &unit.inputs {
        if (input.origin == Origin::Generated) != input.generation.is_some() {
            return Err("Only generated inputs require generator provenance.".into());
        }
        let key = verify_identity(&input.identity, total)?;
        if [&object, &report, &preprocessed].contains(&&key) || !actual.insert(key.clone()) {
            return Err("Native closure contains colliding input/output identities.".into());
        }
        if let Some(generation) = &input.generation {
            generated += 1;
            if !(1..=8192).contains(&generation.inputs.len())
                || !arguments_valid(&generation.arguments, &generation.generator.path)
                || verify_identity(&generation.generator, total)? == key
            {
                return Err("Native generated source lacks a generator/input identity.".into());
            }
            let mut generation_inputs = BTreeSet::new();
            for input in &generation.inputs {
                let dependency = verify_identity(input, total)?;
                if dependency == key || !generation_inputs.insert(dependency) {
                    return Err(
                        "Native generation inputs contain an alias or self-reference.".into(),
                    );
                }
            }
        }
    }
    if expected != actual {
        return Err("Native closure differs from its complete compiler dependency report.".into());
    }
    Ok(generated)
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    if arguments.len() != 5 || arguments[1] != "--manifest" || arguments[3] != "--expected-sha256" {
        return Err(USAGE.into());
    }
    let path = arguments[2].to_str().ok_or(USAGE)?;
    let expected = arguments[4]
        .to_str()
        .filter(|value| lower_hex(value, 64))
        .ok_or(USAGE)?;
    path_key(path)?;
    let bytes = read_bounded(Path::new(path), MAX_DOCUMENT)?;
    if hex(&Sha256::digest(&bytes)) != expected {
        return Err("Native closure manifest differs from its reviewed SHA-256.".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if manifest.schema_version != 1
        || manifest.kind != "selected-clang-native-source-closure"
        || !lower_hex(&manifest.source_revision, 40)
        || manifest.target.is_empty()
        || manifest.target.len() > 128
        || !manifest
            .target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || manifest.configuration.trim().is_empty()
        || manifest.configuration.len() > 256
        || manifest
            .configuration
            .bytes()
            .any(|byte| byte.is_ascii_control())
        || !(1..=4096).contains(&manifest.units.len())
    {
        return Err("Unsupported native source closure identity or bounds.".into());
    }
    let mut total = 0;
    verify_identity(&manifest.compiler, &mut total)?;
    let mut generated = 0;
    let mut objects = BTreeSet::new();
    for unit in &manifest.units {
        if !objects.insert(path_key(&unit.object.path)?) {
            return Err("Native closure has duplicate compiled objects.".into());
        }
        generated += verify_unit(unit, &manifest.compiler.path, &mut total)?;
    }
    Ok(serde_json::json!({
        "schemaVersion": 1, "kind": "selected-native-source-closure-verification", "passed": true,
        "manifestSha256": expected, "sourceRevision": manifest.source_revision,
        "target": manifest.target, "configuration": manifest.configuration,
        "compilerSha256": manifest.compiler.sha256, "translationUnits": manifest.units.len(),
        "generatedInputs": generated, "checkedBytesIncludingRepeatedInputs": total,
        "freshCompilerExecution": false, "completeBuildClosureQualified": false,
        "binaryLinkageQualified": false, "inferenceParityQualified": false,
        "nativeRetirementQualified": false
    })
    .to_string())
}

#[cfg(test)]
#[path = "native_source_closure_tests.rs"]
mod tests;
