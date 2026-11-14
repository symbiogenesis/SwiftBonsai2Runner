//! Hosted public compiler controls. SDK, wire and Windows ABI gates stay independent.
use super::{hold, plain};
use crate::{Result, bounded_input, hexagon_policy, identity as ids, startup_process};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod process;
mod qaic;
mod windows;
mod wire;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    operation: String,
    repository_directory: PathBuf,
    output_directory: PathBuf,
    rustc: PathBuf,
    #[serde(default)]
    source_directory: Option<PathBuf>,
    #[serde(default)]
    pin_path: Option<PathBuf>,
}

fn require(valid: bool, message: &str) -> Result<()> {
    if valid {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("Missing string field: {name}"))
}
fn list<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    value[name]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Missing array field: {name}"))
}
fn identity(path: &Path) -> Result<Value> {
    let (file, digest) = hold(path, 512 * 1024 * 1024)?;
    Ok(
        json!({"name":path.file_name().and_then(|s|s.to_str()).ok_or("Invalid input filename.")?,"bytes":file.metadata().map_err(|error|error.to_string())?.len(),"sha256":digest}),
    )
}
fn pinned(path: &Path, expected: &Value) -> Result<Value> {
    let actual = identity(path)?;
    require(
        actual["bytes"] == expected["bytes"] && actual["sha256"] == expected["sha256"],
        "Input differs from exact reviewed identity.",
    )?;
    Ok(actual)
}
fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    plain(path)?;
    bounded_input::read(File::open(path).map_err(|e| e.to_string())?, maximum)
        .map_err(|e| e.into_message("Public control input exceeds byte bound."))
}
fn json_file(path: &Path, maximum: u64) -> Result<Value> {
    hexagon_policy::data_json(
        std::str::from_utf8(&read(path, maximum)?).map_err(|e| e.to_string())?,
    )
}
fn save(path: &Path, value: &Value) -> Result<()> {
    let encoded = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    require(
        encoded.len() <= 2 * 1024 * 1024,
        "Public control receipt exceeds byte bound.",
    )?;
    fs::write(path, encoded).map_err(|e| e.to_string())
}
fn copy(original: &Path, destination: &Path) -> Result<Value> {
    identity(original)?;
    fs::copy(original, destination).map_err(|e| e.to_string())?;
    identity(destination)
}
fn directory(path: &Path) -> Result<()> {
    fs::create_dir(path).map_err(|e| e.to_string())
}
fn fields_equal(left: &Value, right: &Value, fields: &[&str]) -> Result<()> {
    for field in fields {
        require(
            left.get(*field).is_some()
                && right.get(*field).is_some()
                && left[*field] == right[*field],
            "Independent public capture fields differ.",
        )?;
    }
    Ok(())
}
fn flags_false(value: &Value, names: &[&str]) -> Result<()> {
    for name in names {
        require(
            value[*name] == false,
            "Public control cannot grant independent SDK/device/profile authority.",
        )?;
    }
    Ok(())
}

struct Control {
    root: PathBuf,
    output: PathBuf,
    review: PathBuf,
    deadline: Instant,
    environment: BTreeMap<String, String>,
    removed: Vec<String>,
    commands: Vec<Value>,
}
impl Control {
    fn provenance(&self, report: &mut Value) -> Result<()> {
        let names = [
            "tools/bonsai/Cargo.toml",
            "Cargo.lock",
            "tools/bonsai/src/hexagon_tools/public_control/mod.rs",
            "tools/bonsai/src/hexagon_tools/public_control/process.rs",
            "tools/bonsai/src/hexagon_tools/public_control/qaic.rs",
            "tools/bonsai/src/hexagon_tools/public_control/windows.rs",
            "tools/bonsai/src/hexagon_tools/public_control/wire.rs",
            "tools/bonsai/src/https_transfer.rs",
        ];
        let sources = names
            .iter()
            .map(|name| {
                let mut value = identity(&self.root.join(name))?;
                value["path"] = json!(name);
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()?;
        report["nativeControlSources"] = json!(sources);
        report["nativeSupervisor"] =
            identity(&std::env::current_exe().map_err(|error| error.to_string())?)?;
        Ok(())
    }
    #[expect(
        clippy::needless_bool,
        reason = "The two independent hosted compiler targets resolve opposite predicates on Windows ARM64 and Linux x86_64."
    )]
    fn new(request: &Request, seconds: u64, windows: bool) -> Result<Self> {
        require(
            std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
                && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
            "Public compiler controls require hosted execution; owner fallback is unsupported.",
        )?;
        require(
            if windows {
                cfg!(all(windows, target_arch = "aarch64"))
            } else {
                cfg!(all(target_os = "linux", target_arch = "x86_64"))
            },
            "Hosted public compiler target differs from its independent lane.",
        )?;
        for (name, length) in [
            ("GITHUB_SHA", 40),
            ("GITHUB_RUN_ID", 32),
            ("GITHUB_RUN_ATTEMPT", 8),
        ] {
            let value = std::env::var(name).unwrap_or_default();
            require(
                if name == "GITHUB_SHA" {
                    ids::is_lower_hex(&value, length)
                } else {
                    !value.is_empty()
                        && value.len() <= length
                        && value.bytes().all(|b| b.is_ascii_digit())
                },
                "Exact hosted source/run identity required.",
            )?;
        }
        plain(&request.repository_directory)?;
        plain(&request.output_directory)?;
        bonsai_artifacts::validate_external_path(&request.output_directory)
            .map_err(|e| e.to_string())?;
        if windows {
            let scratch = std::env::var_os("RUNNER_TEMP")
                .map(PathBuf::from)
                .ok_or("Hosted scratch unavailable.")?;
            require(
                request.output_directory.starts_with(scratch),
                "Select a new output directory inside hosted scratch.",
            )?;
        }
        directory(&request.output_directory)?;
        let review = request.output_directory.join("review");
        directory(&review)?;
        let mut environment = std::env::vars().collect::<BTreeMap<_, _>>();
        let removed = [
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "GHC_PACKAGE_PATH",
            "GHC_ENVIRONMENT",
            "GHC_OPTIONS",
            "CABAL_CONFIG",
            "CABAL_DIR",
            "CABAL_BUILDDIR",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "GCC_EXEC_PREFIX",
            "COMPILER_PATH",
            "LIBRARY_PATH",
            "CPATH",
            "C_INCLUDE_PATH",
            "CPLUS_INCLUDE_PATH",
            "CFLAGS",
            "CPPFLAGS",
            "LDFLAGS",
        ]
        .map(str::to_owned)
        .to_vec();
        for name in &removed {
            environment.remove(name);
        }
        Ok(Self {
            root: request.repository_directory.clone(),
            output: request.output_directory.clone(),
            review,
            deadline: Instant::now() + Duration::from_secs(seconds),
            environment,
            removed,
            commands: Vec::new(),
        })
    }
    fn source(&self, name: &str) -> PathBuf {
        self.root.join("tools/hexagon").join(name)
    }
    fn command(
        &mut self,
        name: &str,
        arguments: Vec<OsString>,
        directory: &Path,
        seconds: u64,
        expected_failure: bool,
    ) -> Result<String> {
        require(
            !arguments.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid fixed hosted command.",
        )?;
        let reserve = self
            .deadline
            .checked_sub(Duration::from_secs(15))
            .ok_or("Invalid aggregate deadline.")?;
        let deadline = reserve.min(Instant::now() + Duration::from_secs(seconds));
        require(
            deadline > Instant::now(),
            "Aggregate hosted source allowance exhausted.",
        )?;
        let executable = startup_process::resolve(&arguments[0])?;
        let mut record = json!({"name":name,"arguments":arguments.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>(),"completed":false});
        let command_arguments = arguments.into_iter().skip(1).collect::<Vec<_>>();
        let result = process::run(
            &process::Spec {
                executable: &executable,
                arguments: &command_arguments,
                directory,
                environment: &self.environment,
                removed: &self.removed,
                stdout: &self.review.join(format!("{name}.log")),
                stderr: &self.review.join(format!("{name}.stderr.log")),
                deadline,
            },
            &mut record,
        );
        let valid = record["exitCode"]
            .as_i64()
            .is_some_and(|code| (code == 0) != expected_failure);
        self.commands.push(record);
        let output = result?;
        require(valid, "Hosted command outcome differed from fixed control.")?;
        Ok(output)
    }
    fn run(&mut self, name: &str, arguments: Vec<OsString>, seconds: u64) -> Result<String> {
        self.command(name, arguments, &self.output.clone(), seconds, false)
    }
    fn receive(&self, pin: &Value, directory: &Path, maximum: u64) -> Result<Value> {
        let name = string(pin, "name")?;
        let url = string(pin, "url")?;
        let size = pin["bytes"]
            .as_u64()
            .filter(|size| *size > 0 && *size <= maximum)
            .ok_or("Invalid bounded public byte pin.")?;
        require(
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                && url.starts_with("https://raw.githubusercontent.com/"),
            "Invalid reviewed public-source download.",
        )?;
        let path = directory.join(name);
        let timeout = self
            .deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(30));
        require(
            !timeout.is_zero(),
            "Public download aggregate allowance exhausted.",
        )?;
        let response = crate::https_transfer::metadata_no_redirects(url, timeout, size)?;
        require(
            response.status == 200
                && u64::try_from(response.body.len()).map_err(|e| e.to_string())? == size
                && pin["sha256"] == ids::sha256(&response.body),
            "Public input differs from exact byte/digest pin.",
        )?;
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut output, &response.body).map_err(|e| e.to_string())?;
        let mut result = identity(&path)?;
        result["url"] = json!(url);
        Ok(result)
    }
}

pub(super) fn run(request: &Request) -> Result<Value> {
    match request.operation.as_str() {
        "windows-abi" => windows::run(request),
        "public-qaic" => qaic::run(request),
        _ => Err("Unknown public compiler control.".into()),
    }
}
