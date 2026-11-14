//! Speech compiler pins, provisioning and startup builds.

use std::path::PathBuf;

use crate::identity::is_lower_hex;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "host")]
    Host,
    #[serde(rename = "validate-names", rename_all = "camelCase")]
    ValidateNames {
        archive_root: String,
        names: Vec<String>,
    },
    #[serde(rename = "environment", rename_all = "camelCase")]
    Environment {
        data_directory: PathBuf,
        working_directory: PathBuf,
    },
    #[serde(rename = "build", rename_all = "camelCase")]
    Build {
        repository_root: PathBuf,
        data_directory: PathBuf,
        working_directory: PathBuf,
        #[serde(default = "all_models")]
        model: String,
        #[serde(default)]
        force_build: bool,
        // A cache root; native objects remain partitioned by compiler and host.
        #[serde(default)]
        target_directory: Option<PathBuf>,
    },
    #[serde(rename = "models", rename_all = "camelCase")]
    Models {
        data_directory: PathBuf,
        working_directory: PathBuf,
        #[serde(default = "all_models")]
        model: String,
        #[serde(default = "default_quantization")]
        tts_quantization: String,
        #[serde(default)]
        verify_only: bool,
        #[serde(default)]
        defer_cached_verification: bool,
    },
}

fn all_models() -> String {
    "All".into()
}
fn default_quantization() -> String {
    "Q8_0".into()
}

/// Plans, provisions or builds native speech through explicit operation requests.
///
/// Planning and compiler-name validation remain nonexecuting. Model provisioning,
/// environment preparation and builds use the pinned native startup boundary.
///
/// # Errors
/// Rejects unsupported requests, unsafe compiler paths, malformed pins, invalid
/// host prerequisites, bounded download failures and native compiler failures.
pub fn run(request: Value) -> crate::Result<Value> {
    if request["operation"] == "host" && request.as_object().is_none_or(|fields| fields.len() != 1)
    {
        return Err("Native speech host identity does not accept caller-supplied facts.".into());
    }
    match serde_json::from_value(request).map_err(|_| "Invalid speech build request.")? {
        Request::Plan => build_plan(),
        Request::Host => crate::native_startup::host(),
        Request::Environment {
            data_directory,
            working_directory,
        } => crate::native_startup::environment(&data_directory, &working_directory),
        Request::Build {
            repository_root,
            data_directory,
            working_directory,
            model,
            force_build,
            target_directory,
        } => crate::native_startup::build_for_package(
            &repository_root,
            &data_directory,
            &working_directory,
            &model,
            force_build,
            target_directory.as_deref(),
        ),
        Request::Models {
            data_directory,
            working_directory,
            model,
            tts_quantization,
            verify_only,
            defer_cached_verification,
        } => crate::native_startup::models(
            &data_directory,
            &model,
            &tts_quantization,
            verify_only,
            defer_cached_verification,
            &working_directory,
        ),
        Request::ValidateNames {
            archive_root,
            names,
        } => {
            if names.is_empty() || names.len() > 100_000 {
                return Err("Compiler archive has too many entries.".into());
            }
            for name in names {
                validate_name(&name, &archive_root)?;
            }
            Ok(json!({}))
        }
    }
}

fn text<'a>(value: &'a Value, name: &str) -> crate::Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| "Invalid native speech build lock.".into())
}

fn artifact(value: &Value, url: &str) -> crate::Result<()> {
    let filename = text(value, "filename")?;
    if filename.is_empty()
        || !filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte))
        || value["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
        || !is_lower_hex(text(value, "sha256")?, 64)
        || text(value, "url")? != url
    {
        return Err("Invalid native speech artifact identity.".into());
    }
    Ok(())
}

pub(crate) fn build_plan() -> crate::Result<Value> {
    let mut pin: Value =
        serde_json::from_str(include_str!("../../../config/native-speech.lock.json"))
            .map_err(|_| "Invalid native build lock JSON.")?;
    let compiler = &pin["compiler"];
    let version = text(compiler, "version")?;
    let parts: Vec<_> = version.split('.').collect();
    if pin["schemaVersion"] != 1
        || pin["target"] != "aarch64-pc-windows-msvc"
        || parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || compiler["directory"] != format!("llvm-{version}-arm64")
        || compiler["root"] != format!("clang+llvm-{version}-aarch64-pc-windows-msvc")
        || compiler["filename"] != format!("clang+llvm-{version}-aarch64-pc-windows-msvc.tar.xz")
    {
        return Err("Invalid native speech compiler lock.".into());
    }
    artifact(
        compiler,
        &format!(
            "https://github.com/llvm/llvm-project/releases/download/llvmorg-{version}/clang%2Bllvm-{version}-aarch64-pc-windows-msvc.tar.xz"
        ),
    )?;
    let executables = compiler["executables"]
        .as_object()
        .ok_or("Missing compiler executable identities.")?;
    if executables.len() != 2 {
        return Err("Invalid compiler executable inventory.".into());
    }
    for name in ["clang-cl.exe", "llvm-rc.exe"] {
        let file = &compiler["executables"][name];
        if file["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
            || !is_lower_hex(text(file, "sha256")?, 64)
        {
            return Err("Invalid compiler executable identity.".into());
        }
    }
    // Historical source identities remain in the lock as attribution only.
    // Production TTS builds use Rust and must not request those archives.
    pin.as_object_mut()
        .ok_or("Invalid native build lock.")?
        .remove("sources");
    Ok(pin)
}

fn safe_part(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !part.is_empty()
        && !part.eq_ignore_ascii_case(".git")
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        && !matches!(
            stem.as_str(),
            "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
        )
        && !((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
        && !part
            .chars()
            .any(|character| character.is_control() || "\\:<>\"|?*".contains(character))
}

fn validate_name<'a>(name: &'a str, root: &str) -> crate::Result<&'a str> {
    if !safe_part(root)
        || root.contains('/')
        || !name.starts_with(root)
        || name.as_bytes().get(root.len()) != Some(&b'/')
        || !name.trim_end_matches('/').split('/').all(safe_part)
        || name.contains("//")
    {
        return Err("Native archive contains an unsafe or ambiguous path.".into());
    }
    Ok(&name[root.len() + 1..])
}

#[cfg(test)]
mod tests {
    use super::{Request, build_plan, validate_name};
    use serde_json::json;
    use std::{error::Error, path::PathBuf};

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    #[test]
    fn build_cache_options_are_explicit() -> TestResult {
        let mut request = json!({"operation": "build", "repositoryRoot": "source",
            "workingDirectory": "work", "dataDirectory": "data"});
        let parsed: Request = serde_json::from_value(request.clone())?;
        assert!(matches!(
            parsed,
            Request::Build {
                target_directory: None,
                force_build: false,
                ..
            }
        ));
        let target = PathBuf::from("cache with spaces & fixtures");
        request["targetDirectory"] = json!(target);
        let parsed: Request = serde_json::from_value(request.clone())?;
        assert!(matches!(parsed, Request::Build {
            target_directory: Some(path), ..
        } if path == target));
        request["targetDirectory"] = json!(true);
        assert!(serde_json::from_value::<Request>(request).is_err());
        Ok(())
    }

    #[test]
    fn compiled_asr_compiler_pins_exclude_retired_tts_sources() -> crate::Result<()> {
        let pin = build_plan()?;
        assert!(pin.get("sources").is_none());
        assert_eq!(
            pin["compiler"]["executables"]
                .as_object()
                .map(serde_json::Map::len),
            Some(2)
        );
        Ok(())
    }

    #[test]
    fn retired_tts_source_staging_is_rejected_before_intake() {
        let request = json!({
            "operation": "stage-source",
            "archivePath": "missing-qwen.zip",
            "destination": "retired-qwen-source",
            "archiveRoot": "qwen",
            "bytes": 1,
            "sha256": "0".repeat(64)
        });
        assert!(serde_json::from_value::<Request>(request.clone()).is_err());
        assert_eq!(
            super::run(request),
            Err("Invalid speech build request.".into())
        );
    }

    #[test]
    fn archive_names_reject_windows_aliases_and_traversal() {
        for name in [
            "source/../outside",
            "source/sub/../../outside",
            "/absolute",
            "source/a:stream",
            "source/CON.cpp",
            "source/NUL .txt",
            "source/COM1 .cpp",
            "source/CONIN$",
            "source/conout$.cpp",
            "source/COM¹.cpp",
            "source/COM².cpp",
            "source/COM³.cpp",
            "source/LPT¹.cpp",
            "source/LPT².cpp",
            "source/LPT³.cpp",
            "source/file. ",
            "source//file",
            "source\\file",
        ] {
            assert!(validate_name(name, "source").is_err());
        }
        assert_eq!(
            validate_name("source/src/engine.cpp", "source").as_deref(),
            Ok("src/engine.cpp")
        );
    }
}
