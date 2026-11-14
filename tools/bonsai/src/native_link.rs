//! Bind retained explicit native links and package bytes without executing code.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use bonsai_artifacts::{VerifiedFile, open_locked_file, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Result, identity::is_lower_hex};

const DOCUMENT_LIMIT: u64 = 16 * 1024 * 1024;
const FILE_LIMIT: u64 = 256 * 1024 * 1024;
const IMAGE_LIMIT: u64 = 32 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    manifest: Identity,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputKind {
    CompiledObject,
    ToolchainObject,
    Library,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    kind: InputKind,
    identity: Identity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Link {
    working_directory: PathBuf,
    arguments: Vec<String>,
    exit_code: i32,
    inputs: Vec<Input>,
    output: Identity,
    package_name: String,
    normal_imports: Vec<String>,
    delay_imports: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageFile {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Capture {
    schema_version: u32,
    kind: String,
    source_manifest: Identity,
    source_receipt: Identity,
    linker: Identity,
    links: Vec<Link>,
    package_directory: PathBuf,
    package_files: Vec<PackageFile>,
    system_imports: Vec<String>,
}

#[derive(Default)]
struct Intake {
    total: u64,
    guards: Vec<VerifiedFile>,
}

fn path_key(path: &Path) -> Result<String> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let key = fs::canonicalize(path)
        .map_err(|error| error.to_string())?
        .to_str()
        .ok_or("Native link paths must be UTF-8.")?
        .replace('\\', "/");
    #[cfg(windows)]
    let key = key.to_ascii_lowercase();
    Ok(key)
}

impl Intake {
    fn verify(&mut self, identity: &Identity, limit: u64) -> Result<String> {
        if identity.bytes == 0 || identity.bytes > limit || !is_lower_hex(&identity.sha256, 64) {
            return Err("Native link file identity exceeds its bounds.".into());
        }
        self.total = self
            .total
            .checked_add(identity.bytes)
            .filter(|total| *total <= TOTAL_LIMIT)
            .ok_or("Native link intake exceeds its aggregate byte bound.")?;
        self.guards.push(
            VerifiedFile::verify(&identity.path, identity.bytes, &identity.sha256)
                .map_err(|error| error.to_string())?,
        );
        path_key(&identity.path)
    }

    fn read(&mut self, identity: &Identity, limit: u64) -> Result<Vec<u8>> {
        self.verify(identity, limit)?;
        let mut bytes = Vec::new();
        open_locked_file(&identity.path)
            .map_err(|error| error.to_string())?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if u64::try_from(bytes.len()).map_err(|error| error.to_string())? != identity.bytes {
            return Err("Native link input changed during bounded intake.".into());
        }
        Ok(bytes)
    }
}

// This binds a separately reviewed source-verifier result. It deliberately does
// not reinterpret that receipt as a fresh source verification in this command.
fn source_objects(manifest: &Capture, intake: &mut Intake) -> Result<BTreeMap<String, Identity>> {
    let source: Value =
        serde_json::from_slice(&intake.read(&manifest.source_manifest, DOCUMENT_LIMIT)?)
            .map_err(|error| error.to_string())?;
    let receipt: Value =
        serde_json::from_slice(&intake.read(&manifest.source_receipt, DOCUMENT_LIMIT)?)
            .map_err(|error| error.to_string())?;
    let concrete_identities = source["sourceRevision"]
        .as_str()
        .is_some_and(|value| is_lower_hex(value, 40))
        && source["compiler"]["sha256"]
            .as_str()
            .is_some_and(|value| is_lower_hex(value, 64))
        && source["compilationDatabase"]["sha256"]
            .as_str()
            .is_some_and(|value| is_lower_hex(value, 64))
        && source["configuration"].as_str().is_some_and(|value| {
            !value.trim().is_empty()
                && value.len() <= 256
                && !value.bytes().any(|byte| byte.is_ascii_control())
        });
    if source["schemaVersion"] != 3
        || source["kind"] != "selected-clang-native-source-closure"
        || source["target"] != "aarch64-pc-windows-msvc"
        || receipt["schemaVersion"] != 1
        || receipt["kind"] != "selected-native-source-closure-verification"
        || receipt["passed"] != true
        || receipt["manifestSchemaVersion"] != 3
        || receipt["recursiveGenerationProvenanceChecked"] != true
        || receipt["selectedBuildInventoryChecked"] != true
        || !concrete_identities
        || receipt["manifestSha256"] != manifest.source_manifest.sha256
        || receipt["sourceRevision"] != source["sourceRevision"]
        || receipt["target"] != source["target"]
        || receipt["configuration"] != source["configuration"]
        || receipt["compilerSha256"] != source["compiler"]["sha256"]
        || receipt["compilationDatabaseSha256"] != source["compilationDatabase"]["sha256"]
    {
        return Err(
            "Native link requires the matching independently reviewed schema-3 source receipt."
                .into(),
        );
    }
    let units = source["units"]
        .as_array()
        .filter(|units| (1..=4096).contains(&units.len()))
        .ok_or("Native link source inventory has invalid translation units.")?;
    if receipt["translationUnits"] != units.len() {
        return Err("Native link source receipt has a changed translation-unit count.".into());
    }
    let mut result = BTreeMap::new();
    for unit in units {
        let object: Identity =
            serde_json::from_value(unit["object"].clone()).map_err(|error| error.to_string())?;
        let key = intake.verify(&object, FILE_LIMIT)?;
        if result.insert(key, object).is_some() {
            return Err("Native link source inventory repeats an object.".into());
        }
    }
    Ok(result)
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
}

fn extension(name: &str, expected: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn image_name(name: &str) -> bool {
    extension(name, "dll") || extension(name, "exe")
}

fn module_set(names: &[String]) -> Result<BTreeSet<String>> {
    if names.len() > 4096 {
        return Err("Native link import inventory exceeds its bound.".into());
    }
    let mut set = BTreeSet::new();
    for name in names {
        if !safe_name(name) || !extension(name, "dll") || !set.insert(name.clone()) {
            return Err(
                "Native link import names must be distinct lowercase DLL basenames.".into(),
            );
        }
    }
    Ok(set)
}

fn verify_arguments(link: &Link, linker: &Identity) -> Result<BTreeSet<String>> {
    if !(5..=8192).contains(&link.arguments.len())
        || link.arguments[0] != linker.path.to_str().ok_or("Non-UTF8 linker path.")?
        || link.exit_code != 0
        || !link.working_directory.is_dir()
    {
        return Err("Native link lacks a successful explicit original invocation.".into());
    }
    path_key(&link.working_directory)?;
    let mut options = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    for argument in &link.arguments[1..] {
        if argument.len() > 16_384 || argument.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("Native link argument exceeds its bounds.".into());
        }
        if Path::new(argument).is_absolute() && Path::new(argument).is_file() {
            let extension = Path::new(argument)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !["obj", "lib", "a"].contains(&extension.as_str())
                || !inputs.insert(path_key(Path::new(argument))?)
            {
                return Err("Native link requires distinct explicit object/library inputs.".into());
            }
            continue;
        }
        let option = argument.to_ascii_lowercase();
        let key = option.split(':').next().unwrap_or_default();
        let allowed = matches!(
            option.as_str(),
            "/nologo"
                | "/nodefaultlib"
                | "/machine:arm64"
                | "/subsystem:console"
                | "/subsystem:windows"
                | "/dll"
                | "/noimplib"
                | "/opt:ref"
                | "/opt:icf"
                | "/debug:none"
                | "/nxcompat"
                | "/dynamicbase"
                | "/highentropyva"
                | "/fixed:no"
                | "/incremental:no"
                | "/manifest:no"
                | "/brepro"
        ) || option.strip_prefix("/entry:").is_some_and(|entry| {
            !entry.is_empty()
                && entry
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.$".contains(&byte))
        }) || argument
            .strip_prefix("/out:")
            .is_some_and(|output| Path::new(output) == link.output.path)
            || option
                .strip_prefix("/delayload:")
                .is_some_and(|name| safe_name(name) && extension(name, "dll"));
        // Multiple distinct /delayload options are meaningful; every other
        // action/output option occurs at most once.
        let unique = if key == "/delayload" { &option } else { key };
        if !allowed || !options.insert(unique.to_owned()) {
            return Err("Native link has an unsupported or repeated option; response/search/default-library inputs are excluded.".into());
        }
    }
    if inputs.is_empty()
        || !["/nodefaultlib", "/machine", "/out", "/manifest"]
            .iter()
            .all(|key| options.contains(*key))
        || (options.contains("/dll") && !options.contains("/noimplib"))
    {
        return Err("Native link requires explicit input, ARM64, output and disabled implicit-library/manifest paths.".into());
    }
    Ok(inputs)
}

fn package(manifest: &Capture, intake: &mut Intake) -> Result<BTreeMap<String, Identity>> {
    path_key(&manifest.package_directory)?;
    if !manifest.package_directory.is_dir() || !(1..=4096).contains(&manifest.package_files.len()) {
        return Err("Native link requires a bounded flat package directory.".into());
    }
    let mut files = BTreeMap::new();
    for file in &manifest.package_files {
        if !safe_name(&file.name) {
            return Err("Native link package names must be lowercase plain basenames.".into());
        }
        let identity = Identity {
            path: manifest.package_directory.join(&file.name),
            bytes: file.bytes,
            sha256: file.sha256.clone(),
        };
        intake.verify(&identity, FILE_LIMIT)?;
        if files.insert(file.name.clone(), identity).is_some() {
            return Err("Native link package repeats an entry.".into());
        }
    }
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(&manifest.package_directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or("Non-UTF8 package entry.")?
            .to_owned();
        if actual.len() >= 4096
            || !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
            || !actual.insert(name)
        {
            return Err("Native link package contains an unsupported entry.".into());
        }
    }
    if actual != files.keys().cloned().collect() {
        return Err("Native link package has missing or extra entries.".into());
    }
    Ok(files)
}

fn verify_inputs(
    link: &Link,
    intake: &mut Intake,
    objects: &BTreeMap<String, Identity>,
    linker: &Identity,
    outputs: &BTreeSet<String>,
    covered: &mut BTreeSet<String>,
) -> Result<()> {
    let arguments = verify_arguments(link, linker)?;
    let linker = path_key(&linker.path)?;
    let mut inputs = BTreeSet::new();
    if !(1..=4096).contains(&link.inputs.len()) {
        return Err("Native link input count exceeds its bound.".into());
    }
    for input in &link.inputs {
        let key = intake.verify(&input.identity, FILE_LIMIT)?;
        if outputs.contains(&key) || key == linker || !inputs.insert(key.clone()) {
            return Err("Native link repeats or aliases an input/output.".into());
        }
        let extension = input
            .identity
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !match input.kind {
            InputKind::CompiledObject | InputKind::ToolchainObject => {
                extension.eq_ignore_ascii_case("obj")
            }
            InputKind::Library => {
                extension.eq_ignore_ascii_case("lib") || extension.eq_ignore_ascii_case("a")
            }
        } {
            return Err("Native link input kind differs from its object/library extension.".into());
        }
        match input.kind {
            InputKind::CompiledObject => {
                if objects.get(&key) != Some(&input.identity) {
                    return Err(
                        "Native link compiled object differs from the source selection.".into(),
                    );
                }
                covered.insert(key);
            }
            InputKind::Library | InputKind::ToolchainObject if objects.contains_key(&key) => {
                return Err("Native link cannot relabel a selected compiled object.".into());
            }
            InputKind::Library | InputKind::ToolchainObject => {}
        }
    }
    if inputs != arguments {
        return Err("Native link inputs differ from its explicit invocation.".into());
    }
    Ok(())
}

fn verify(manifest: &Capture, intake: &mut Intake) -> Result<Value> {
    if manifest.schema_version != 1
        || manifest.kind != "selected-lld-arm64-link-package"
        || !(1..=128).contains(&manifest.links.len())
    {
        return Err("Unsupported native link manifest identity or bounds.".into());
    }
    let objects = source_objects(manifest, intake)?;
    let linker = intake.verify(&manifest.linker, FILE_LIMIT)?;
    let package = package(manifest, intake)?;
    let systems = module_set(&manifest.system_imports)?;
    if systems.iter().any(|name| package.contains_key(name)) {
        return Err("Native link system imports collide with packaged files.".into());
    }
    let mut covered = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for link in &manifest.links {
        let output = intake.verify(&link.output, IMAGE_LIMIT)?;
        if !outputs.insert(output.clone()) || output == linker || objects.contains_key(&output) {
            return Err(
                "Native link outputs alias another output, source object or linker.".into(),
            );
        }
    }
    let mut packaged_images = BTreeSet::new();
    let mut imported_systems = BTreeSet::new();
    for link in &manifest.links {
        verify_inputs(
            link,
            intake,
            &objects,
            &manifest.linker,
            &outputs,
            &mut covered,
        )?;
        let file = package
            .get(&link.package_name)
            .ok_or("Native link output is absent from its package.")?;
        if file.bytes != link.output.bytes
            || file.sha256 != link.output.sha256
            || !image_name(&link.package_name)
            || !packaged_images.insert(link.package_name.clone())
        {
            return Err("Native link output differs from or repeats its packaged image.".into());
        }
        let image = crate::pe_closure::parse_image(intake.read(file, IMAGE_LIMIT)?)?;
        for (delayed, expected) in [(false, &link.normal_imports), (true, &link.delay_imports)] {
            let actual = crate::pe_closure::imports(&image, delayed)?;
            if module_set(&actual)? != module_set(expected)? {
                return Err("Native link PE imports differ from the reviewed inventory.".into());
            }
            for name in actual {
                if systems.contains(&name) {
                    imported_systems.insert(name);
                } else if !package.contains_key(&name) {
                    return Err(
                        "Native link PE import is neither packaged nor a declared system boundary."
                            .into(),
                    );
                }
            }
        }
    }
    let expected_images: BTreeSet<_> = package
        .keys()
        .filter(|name| image_name(name))
        .cloned()
        .collect();
    if covered != objects.keys().cloned().collect()
        || packaged_images != expected_images
        || imported_systems != systems
    {
        return Err(
            "Native link omits source objects/package images or declares unused system imports."
                .into(),
        );
    }
    Ok(json!({
        "schemaVersion":1, "kind":"selected-native-link-package-verification", "passed":true,
        "sourceManifestSha256":manifest.source_manifest.sha256,
        "sourceReceiptSha256":manifest.source_receipt.sha256,
        "linkerSha256":manifest.linker.sha256, "translationUnits":objects.len(),
        "links":manifest.links.len(), "packageFiles":package.len(),
        "normalAndDelayImportsChecked":true, "exactFlatPackageChecked":true,
        "sourceClosureReexecuted":false, "independentSourceReceiptReviewRequired":true,
        "freshLinkerExecution":false, "systemDependencyClosureChecked":false,
        "staticLibrarySourceClosureChecked":false, "cppRuntimeAbsenceQualified":false,
        "nativeRetirementQualified":false
    }))
}

/// Verify a reviewed native link/package manifest and its original retained files.
///
/// The request's `manifest` is an exact path/bytes/SHA-256 identity. This command
/// binds a separately reviewed schema-3 source-verifier receipt; it does not run
/// that verifier again. Static-library source ancestry, OS imports/forwarders,
/// original invocation provenance and retirement acceptance remain independent.
///
/// # Errors
/// Rejects changed/redirected files, unsupported link options, omitted inputs,
/// failed links, PE import differences and missing/extra/changed package bytes.
pub fn run(request: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    let mut intake = Intake::default();
    let manifest: Capture =
        serde_json::from_slice(&intake.read(&request.manifest, DOCUMENT_LIMIT)?)
            .map_err(|error| error.to_string())?;
    let mut receipt = verify(&manifest, &mut intake)?;
    receipt["manifestSha256"] = request.manifest.sha256.into();
    receipt["checkedBytesIncludingRepeatedInputs"] = intake.total.into();
    Ok(receipt)
}

#[cfg(all(test, windows))]
mod tests;
