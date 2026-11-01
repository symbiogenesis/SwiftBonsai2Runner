//! JSON command entry point for packaged Bonsai maintenance tools.
#![forbid(unsafe_code)]

use std::io::Write as _;
use std::process::ExitCode;

fn initialize_startup(request: &mut serde_json::Value) -> bonsai_tools::Result<()> {
    let object = request.as_object_mut().ok_or("Invalid startup request.")?;
    let parent = match (
        object.remove("parentProcessId"),
        object.remove("parentCreationTimeFileTime"),
    ) {
        (None, None) => None,
        (Some(id), Some(creation)) => {
            let id = id
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id != 0)
                .ok_or("Invalid startup caller identity.")?;
            let creation = creation
                .as_u64()
                .filter(|creation| *creation != 0)
                .ok_or("Invalid startup caller identity.")?;
            Some((id, creation))
        }
        _ => return Err("Incomplete startup caller identity.".into()),
    };
    bonsai_tools::startup_cancel::initialize(parent)
}

fn execute() -> bonsai_tools::Result<serde_json::Value> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == "--version" {
        return Ok(bonsai_tools::version());
    }
    if arguments.len() == 1 && arguments[0] == "--help" {
        return Ok(serde_json::json!({
            "Usage": "bonsai-tools COMMAND --request ABSOLUTE_JSON_PATH | --request - | --json JSON_OBJECT; bonsai-tools chat [--repository-root PATH] [--data-directory PATH] [--native-engine] [--use-repacked-ptq1] [--no-speech-models]",
            "Commands": bonsai_tools::COMMANDS,
            "Chat": "All native launch options are documented in tools/bonsai/README.md."
        }));
    }
    let command = arguments.first().and_then(|value| value.to_str())
        .ok_or("Unknown or missing maintenance command; use --help.")?;
    let mut request = if command == "chat" {
        bonsai_tools::cli::chat(&arguments[1..])?
    } else {
        bonsai_tools::cli::request(command, &arguments[1..])?
    };
    let command = if command == "chat" { "chat-launch" } else { command };
    let startup = matches!(
        Some(command),
        Some("source-build" | "chat-launch" | "chat-prepare")
    ) || (command == "speech-build"
        && matches!(
            request["operation"].as_str(),
            Some("models" | "environment" | "build")
        ))
        || (command == "speech-evaluation" && request["operation"] == "evaluate")
        || (command == "chat-package"
            && matches!(request["operation"].as_str(), Some("launch" | "start")));
    if startup {
        initialize_startup(&mut request)?;
    }
    match Some(command) {
        Some("credentials") => bonsai_tools::credentials::run(request),
        Some("completion") => bonsai_tools::completion::run(request),
        Some("host-cpu") => bonsai_tools::host_cpu::run(&request),
        Some("host-gpu") => bonsai_tools::host_gpu::run(&request),
        Some("host-gpu-query") => bonsai_tools::host_gpu::query(&request),
        Some("host-opencl") => bonsai_tools::host_opencl::run(&request),
        Some("host-opencl-query") => bonsai_tools::host_opencl::query(&request),
        Some("cpu-evaluation") => bonsai_tools::cpu_evaluation::run(request),
        Some("cpu-check-plan") => bonsai_tools::completion::cpu_check_plan(request),
        Some("cpu-check-report") => bonsai_tools::completion::cpu_check_report(request),
        Some("cpu-check") => bonsai_tools::cpu_check::run(request),
        Some("server-plan") => bonsai_tools::server::plan(request),
        Some("server-policy") => bonsai_tools::server::run(request),
        Some("runtime-stage") => bonsai_tools::install::run(request),
        Some("runtime-verify") => bonsai_tools::runtime::run(request),
        Some("memory") if request.as_object().is_some_and(serde_json::Map::is_empty) => {
            bonsai_tools::completion::available_memory()
        }
        Some("qualification") => bonsai_tools::qualification::run(request),
        Some("runtime-promotion") => bonsai_tools::runtime_promotion::run(request),
        Some("speech-models") => bonsai_tools::speech::run(request),
        Some("inference-lock") => bonsai_tools::inference_lock::run(request),
        Some("inference-capabilities") => bonsai_tools::inference_capabilities::run(request),
        Some("speech-build") => bonsai_tools::speech_build::run(request),
        Some("speech-evaluation") => bonsai_tools::speech_evaluation::run(request),
        Some("source-build") => bonsai_tools::source_build::run(request),
        Some("chat-launch") => bonsai_tools::chat_launch::run(request),
        Some("chat-prepare") => bonsai_tools::chat_launch::prepare_run(request),
        Some("chat-package") => bonsai_tools::chat_package::run(request),
        Some("gguf") => bonsai_tools::gguf::run(request),
        Some("ptq1-capture") => bonsai_tools::ptq1_capture::run(request),
        Some("native-qualification") => bonsai_tools::native_qualification::run(request),
        Some("ptq1-packing") => bonsai_tools::ptq1_packing::run(request),
        Some("packing-qualification") => bonsai_tools::packing_qualification::run(request),
        Some("host-capabilities") => bonsai_tools::host_capabilities::run(&request),
        Some("native-acceptance-budget") => bonsai_tools::native_acceptance_budget::run(request),
        Some("hexagon-policy") => bonsai_tools::hexagon_policy::run(request),
        Some("publisher-startup") => bonsai_tools::publisher_startup::run(request),
        Some("startup-diagnostics") => bonsai_tools::startup_diagnostics::run(request),
        _ => Err("Unknown maintenance command.".into()),
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(value) => {
            let mut output = std::io::stdout().lock();
            if serde_json::to_writer(&mut output, &value).is_err()
                || output.write_all(b"\n").is_err()
            {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
