//! Exporter-independent serialized fixtures exercised against the pinned CLI.

use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod census;
mod export;
mod parser;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    gguf_tool_path: PathBuf,
    evidence_directory: PathBuf,
    #[serde(default = "all")]
    case: String,
}

fn all() -> String {
    "all".into()
}

fn require(condition: bool, message: &str) -> crate::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn count(value: &Value, name: &str) -> crate::Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("Missing fixture result count: {name}"))
}

#[derive(Default)]
struct Wire(Vec<u8>);

impl Wire {
    fn header(tensors: u64, metadata: u64) -> Self {
        let mut value = Self::default();
        value.u32(0x4655_4747);
        value.u32(3);
        value.u64(tensors);
        value.u64(metadata);
        value
    }
    const fn pos(&self) -> usize {
        self.0.len()
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn text(&mut self, value: &str) -> crate::Result<()> {
        self.u64(u64::try_from(value.len()).map_err(|error| error.to_string())?);
        self.0.extend_from_slice(value.as_bytes());
        Ok(())
    }
    fn key(&mut self, name: &str, kind: u32) -> crate::Result<()> {
        self.text(name)?;
        self.u32(kind);
        Ok(())
    }
    fn scalar_text(&mut self, name: &str, value: &str) -> crate::Result<()> {
        self.key(name, 8)?;
        self.text(value)
    }
    fn scalar(&mut self, name: &str, value: u32) -> crate::Result<()> {
        self.key(name, 4)?;
        self.u32(value);
        Ok(())
    }
    fn align(&mut self) {
        self.0.resize(self.pos().div_ceil(32) * 32, 0);
    }
}

fn patch(bytes: &mut [u8], offset: usize, replacement: &[u8]) {
    bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
}

struct Harness<'a> {
    tool: &'a Path,
    directory: &'a Path,
}

impl Harness<'_> {
    fn write(&self, name: &str, bytes: &[u8]) -> crate::Result<PathBuf> {
        let path = self.directory.join(format!("{name}.gguf"));
        fs::write(&path, bytes).map_err(|error| error.to_string())?;
        Ok(path)
    }
    fn arguments(
        &self,
        name: &str,
        bytes: &[u8],
        dialect: &str,
        census: bool,
    ) -> crate::Result<Vec<std::ffi::OsString>> {
        let path = self.write(name, bytes)?;
        let mut arguments = vec![
            "inspect".into(),
            "--model".into(),
            path.into_os_string(),
            "--expected-bytes".into(),
            bytes.len().to_string().into(),
            "--expected-sha256".into(),
            crate::identity::sha256(bytes).into(),
            "--dialect".into(),
            dialect.into(),
        ];
        if census {
            arguments.push("--census-pq2".into());
        }
        Ok(arguments)
    }
    fn inspect(
        &self,
        name: &str,
        bytes: &[u8],
        dialect: &str,
        census: bool,
    ) -> crate::Result<Value> {
        let arguments = self.arguments(name, bytes, dialect, census)?;
        super::decode(super::invoke(self.tool, &arguments, self.directory, 15)?)
    }
    fn reject(
        &self,
        name: &str,
        bytes: &[u8],
        dialect: &str,
        census: bool,
        diagnostic: Option<&str>,
    ) -> crate::Result<()> {
        let arguments = self.arguments(name, bytes, dialect, census)?;
        let output = super::invoke(self.tool, &arguments, self.directory, 15)?;
        require(
            output.code == 1 && output.stdout.is_empty() && !output.stderr.is_empty(),
            &format!("Expected CLI rejection rather than crash, timeout or success for {name}."),
        )?;
        if let Some(diagnostic) = diagnostic {
            require(
                output.stderr.contains("GGUF:") && output.stderr.contains(diagnostic),
                &format!("Wrong parser rejection for {name}: {}", output.stderr),
            )?;
        }
        Ok(())
    }
    fn bridge(&self, name: &str, bytes: &[u8], dialect: &str, census: bool) -> crate::Result<()> {
        let path = self.write(name, bytes)?;
        let output = self.directory.join(format!("{name}.json"));
        let mut request = json!({"operation":"inspect", "modelPath":path, "outputPath":output,
            "expectedBytes":bytes.len(), "expectedSha256":crate::identity::sha256(bytes),
            "dialect":dialect, "censusPq2":census, "ggufToolPath":self.tool});
        let result = super::run(request.clone())?;
        let saved: Value =
            serde_json::from_slice(&fs::read(&output).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        require(
            saved == result && saved["ArtifactVerification"] == "verified-size-and-sha256",
            "Verified GGUF report did not round-trip.",
        )?;
        require(
            super::run(request.clone()).is_err(),
            "Existing evidence was overwritten.",
        )?;
        request
            .as_object_mut()
            .ok_or("Missing request.")?
            .remove("outputPath");
        request["expectedSha256"] = "0".repeat(64).into();
        require(
            super::run(request.clone()).is_err(),
            "Digest mismatch was accepted.",
        )?;
        request
            .as_object_mut()
            .ok_or("Missing request.")?
            .remove("expectedSha256");
        require(
            super::run(request).is_err(),
            "Incomplete artifact identity was accepted.",
        )
    }
}

pub(super) fn run(request: Request) -> crate::Result<Value> {
    let cases = [
        "parser",
        "strings",
        "arrays",
        "descriptors",
        "artifact",
        "census",
        "repack",
        "rejection",
    ];
    require(
        request.case == "all" || cases.contains(&request.case.as_str()),
        "Unknown GGUF fixture case.",
    )?;
    require(
        cfg!(all(windows, target_arch = "aarch64"))
            && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && std::env::var("GITHUB_REF").as_deref() == Ok("refs/heads/main")
            && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "GGUF integration fixtures run only in hosted native Windows ARM64 CI on main.",
    )?;
    super::external(&request.gguf_tool_path)?;
    super::external(&request.evidence_directory)?;
    let runtime = super::pin(super::INFERENCE)?;
    let pin = runtime["files"]
        .as_array()
        .ok_or("Missing pinned inference files.")?
        .iter()
        .find(|file| file["name"] == "bonsai-gguf.exe")
        .ok_or("The pinned release has no GGUF fixture tool.")?;
    let _guard = super::verify(&request.gguf_tool_path, pin)?;
    crate::startup_cache::assert_arm64(&request.gguf_tool_path)?;
    fs::create_dir(&request.evidence_directory)
        .map_err(|error| format!("Use a new GGUF fixture evidence directory: {error}"))?;
    let harness = Harness {
        tool: &request.gguf_tool_path,
        directory: &request.evidence_directory,
    };
    let mut report = json!({"schemaVersion":1,"kind":"native-gguf-integration","toolSha256":pin["sha256"],"passed":false,"cases":[]});
    let result = (|| {
        for case in cases
            .into_iter()
            .filter(|case| request.case == "all" || *case == request.case)
        {
            report["activeCase"] = case.into();
            match case {
                "parser" => parser::parser(&harness),
                "strings" => parser::strings(&harness),
                "arrays" => parser::arrays(&harness),
                "descriptors" => parser::descriptors(&harness),
                "artifact" => parser::artifact(&harness),
                "census" => census::run(&harness),
                "repack" => export::repack(&harness),
                "rejection" => export::rejection(&harness),
                _ => Err("Unknown GGUF fixture case.".into()),
            }?;
            report["cases"]
                .as_array_mut()
                .ok_or("Missing cases.")?
                .push(case.into());
        }
        Ok::<(), String>(())
    })();
    report["passed"] = result.is_ok().into();
    if let Err(error) = &result {
        report["failure"] = error.clone().into();
    }
    fs::write(
        request.evidence_directory.join("checks.json"),
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    result?;
    Ok(report)
}
