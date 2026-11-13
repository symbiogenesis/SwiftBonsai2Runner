//! Shared maintenance commands for the console and graphical entry points.

use std::io::Write as _;
use std::process::ExitCode;

fn initialize_startup(request: &mut serde_json::Value) -> bonsai_tools::Result<()> {
    let object = request.as_object_mut().ok_or("Invalid startup request.")?;
    let parent = match (
        object.remove("parentProcessId"),
        object.remove("parentCreationTimeFileTime"),
    ) {
        (None, None) => {
            #[cfg(windows)]
            {
                Some(
                    bonsai_windows_process::parent_process_identity()
                        .map_err(|error| error.to_string())?,
                )
            }
            #[cfg(not(windows))]
            {
                None
            }
        }
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

fn startup_command(command: &str, request: &serde_json::Value) -> bool {
    matches!(
        command,
        "source-build"
            | "chat-launch"
            | "chat-prepare"
            | "hexagon-tools"
            | "build-tools"
            | "provision"
            | "artifact"
            | "doctor"
    ) || (command == "speech-build"
        && matches!(
            request["operation"].as_str(),
            Some("models" | "environment" | "build")
        ))
        || (command == "speech-evaluation" && request["operation"] == "evaluate")
        || (command == "chat-package"
            && matches!(request["operation"].as_str(), Some("launch" | "start")))
}

fn execute() -> bonsai_tools::Result<serde_json::Value> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "updated-chat")
    {
        if !bonsai_tools::chat_package::bundle::current_is_bundled()? {
            return Err("Updated chat requires a verified portable bundle.".into());
        }
        // The bounded replacement helper exits after spawning this new root.
        // Its lifetime must not cancel the reopened application's preparation.
        bonsai_tools::startup_cancel::initialize(None)?;
        return bonsai_tools::chat_package::bundle::launch(&arguments[1..]);
    }
    let bundled_command = arguments
        .first()
        .is_some_and(|argument| argument == "bundled-chat");
    if bundled_command
        || (arguments.is_empty() && bonsai_tools::chat_package::bundle::current_is_bundled()?)
    {
        initialize_startup(&mut serde_json::json!({}))?;
        return bonsai_tools::chat_package::bundle::launch(if bundled_command {
            &arguments[1..]
        } else {
            &[]
        });
    }
    if arguments.len() == 1 && arguments[0] == "--version" {
        return Ok(bonsai_tools::version());
    }
    if arguments.len() == 1 && arguments[0] == "--help" {
        return Ok(serde_json::json!({
            "Usage": "bonsai-tools COMMAND --request ABSOLUTE_JSON_PATH | --request - | --json JSON_OBJECT; bonsai-tools chat [--repository-root PATH] [--data-directory PATH] [--native-engine|--publisher-engine] [--use-repacked-ptq1|--use-original-pq2] [--no-speech-models]; bonsai-tools installed-chat [--installation-directory PATH] [--data-directory PATH] [--no-speech-models]; SwiftBonsai2Runner.exe [bundled-chat [--data-directory PATH] [--no-speech-models]]",
            "Commands": bonsai_tools::COMMANDS,
            "Chat": "Development default: Rust engine, original PQ2, four row workers and standard adapter at scale 1.3. Full release qualification is pending; --publisher-engine selects manual rollback. All options are documented in tools/bonsai/README.md."
        }));
    }
    let command = arguments
        .first()
        .and_then(|value| value.to_str())
        .ok_or("Unknown or missing maintenance command; use --help.")?;
    let mut request = if command == "installed-chat" {
        bonsai_tools::cli::installed_chat(&arguments[1..])?
    } else if command == "chat" {
        bonsai_tools::cli::chat(&arguments[1..])?
    } else {
        bonsai_tools::cli::request(command, &arguments[1..])?
    };
    let command = if command == "installed-chat" {
        "chat-package"
    } else if command == "chat" {
        "chat-launch"
    } else {
        command
    };
    let startup = startup_command(command, &request);
    if startup {
        initialize_startup(&mut request)?;
    }
    #[cfg(windows)]
    if !startup
        && needs_caller(command, &request)
        && request.get("parentProcessId").is_none()
        && request.get("parentCreationTimeFileTime").is_none()
    {
        let (id, creation) =
            bonsai_windows_process::parent_process_identity().map_err(|error| error.to_string())?;
        request["parentProcessId"] = id.into();
        request["parentCreationTimeFileTime"] = creation.into();
    }
    dispatch(command, request)
}

fn dispatch(command: &str, request: serde_json::Value) -> bonsai_tools::Result<serde_json::Value> {
    match command {
        "hexagon-tools" => bonsai_tools::hexagon_tools::run(request),
        "build-tools" => bonsai_tools::build_tools::run(request),
        "provision" => bonsai_tools::provision::run(request),
        "artifact" => bonsai_tools::provision::artifact_run(request),
        "doctor" => bonsai_tools::doctor::run(request),
        "credentials" => bonsai_tools::credentials::run(request),
        "completion" => bonsai_tools::completion::run(request),
        "host-cpu" => bonsai_tools::host_cpu::run(&request),
        "host-gpu" => bonsai_tools::host_gpu::run(&request),
        "host-gpu-query" => bonsai_tools::host_gpu::query(&request),
        "host-opencl" => bonsai_tools::host_opencl::run(&request),
        "host-opencl-query" => bonsai_tools::host_opencl::query(&request),
        "cpu-evaluation" => bonsai_tools::cpu_evaluation::run(request),
        "cpu-check-plan" => bonsai_tools::completion::cpu_check_plan(request),
        "cpu-check-report" => bonsai_tools::completion::cpu_check_report(request),
        "cpu-check" => bonsai_tools::cpu_check::run(request),
        "server-plan" => bonsai_tools::server::plan(request),
        "server-policy" => bonsai_tools::server::run(request),
        "runtime-stage" => bonsai_tools::install::run(request),
        "runtime-verify" => bonsai_tools::runtime::run(request),
        "memory" if request.as_object().is_some_and(serde_json::Map::is_empty) => {
            bonsai_tools::completion::available_memory()
        }
        "qualification" => bonsai_tools::qualification::run(request),
        "runtime-promotion" => bonsai_tools::runtime_promotion::run(request),
        "kv-promotion" => bonsai_tools::kv_promotion::run(request),
        "evidence-archive" => bonsai_tools::evidence_archive::run(request),
        "speech-models" => bonsai_tools::speech::run(request),
        "inference-lock" => bonsai_tools::inference_lock::run(request),
        "inference-capabilities" => bonsai_tools::inference_capabilities::run(request),
        "speech-build" => bonsai_tools::speech_build::run(request),
        "speech-evaluation" => bonsai_tools::speech_evaluation::run(request),
        "source-build" => bonsai_tools::source_build::run(request),
        "chat-launch" => bonsai_tools::chat_launch::run(request),
        "chat-prepare" => bonsai_tools::chat_launch::prepare_run(request),
        "chat-package" => bonsai_tools::chat_package::run(request),
        "chat-bundle" => bonsai_tools::chat_package::bundle::run(request),
        "app-update" => bonsai_tools::app_update::run(request),
        "gguf" => bonsai_tools::gguf::run(request),
        "ptq1-capture" => bonsai_tools::ptq1_capture::run(request),
        "native-qualification" => bonsai_tools::native_qualification::run(request),
        "ptq1-packing" => bonsai_tools::ptq1_packing::run(request),
        "packing-qualification" => bonsai_tools::packing_qualification::run(request),
        "host-capabilities" => bonsai_tools::host_capabilities::run(&request),
        "native-acceptance-budget" => bonsai_tools::native_acceptance_budget::run(request),
        "hexagon-policy" => bonsai_tools::hexagon_policy::run(request),
        "publisher-startup" => bonsai_tools::publisher_startup::run(request),
        "pe-closure" => bonsai_tools::pe_closure::run(request),
        "startup-diagnostics" => bonsai_tools::startup_diagnostics::run(request),
        _ => Err("Unknown maintenance command.".into()),
    }
}

#[cfg(windows)]
fn needs_caller(command: &str, request: &serde_json::Value) -> bool {
    matches!(
        command,
        "completion"
            | "runtime-verify"
            | "inference-capabilities"
            | "cpu-check"
            | "server-plan"
            | "native-qualification"
            | "publisher-startup"
    ) || (command == "cpu-evaluation" && request["operation"] == "capture")
        || (command == "speech-evaluation" && request["operation"] == "session")
        || (command == "qualification"
            && matches!(
                request["operation"].as_str(),
                Some("run" | "screen-run" | "capture" | "screen-capture")
            ))
}

/// Runs the shared protocol entry with the selected Windows presentation policy.
#[expect(
    clippy::redundant_pub_crate,
    reason = "Keep the shared binary entry private while satisfying unreachable_pub."
)]
pub(super) fn run(graphical_launcher: bool) -> ExitCode {
    let graphical = cfg!(windows)
        && graphical_launcher
        && std::env::args_os().nth(1).is_none_or(|argument| {
            matches!(
                argument.to_str(),
                Some("bundled-chat" | "updated-chat" | "installed-chat" | "chat")
            )
        });
    #[cfg(windows)]
    if graphical_launcher && !graphical {
        bonsai_windows_process::attach_parent_console();
    }
    match execute() {
        Ok(_) if graphical => ExitCode::SUCCESS,
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
            #[cfg(windows)]
            if graphical {
                bonsai_windows_process::show_startup_error(&error);
            } else {
                let _ = writeln!(std::io::stderr().lock(), "{error}");
            }
            #[cfg(not(windows))]
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
