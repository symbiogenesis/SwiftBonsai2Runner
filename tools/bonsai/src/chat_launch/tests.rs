use super::*;

#[test]
fn model_preparation_overlaps_independent_work_and_joins_before_handoff() -> crate::Result<()> {
    let (started, awaiting_start) = std::sync::mpsc::sync_channel(1);
    let (release, awaiting_release) = std::sync::mpsc::sync_channel(1);
    let verified = AtomicBool::new(false);
    let runtime_verified = &verified;
    let built = parallel_model_preparation(
        move || {
            started.send(()).map_err(|error| error.to_string())?;
            awaiting_release
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            runtime_verified.store(true, Ordering::Release);
            Ok(())
        },
        || {
            awaiting_start
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            release.send(()).map_err(|error| error.to_string())?;
            Ok(json!({"BrokerPath": "built.exe"}))
        },
    )?;
    assert!(verified.load(Ordering::Acquire));
    assert_eq!(built, json!({"BrokerPath": "built.exe"}));
    Ok(())
}

fn wait_for_preparation_peer_failure(stopped: &AtomicBool) -> crate::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Err(error) = crate::startup_cancel::checkpoint() {
            stopped.store(true, Ordering::Release);
            return Err(error);
        }
        if std::time::Instant::now() >= deadline {
            return Err("Preparation peer did not cancel.".into());
        }
        std::thread::yield_now();
    }
}

#[test]
fn model_preparation_failure_cancels_joins_peer_and_keeps_original_error() -> crate::Result<()> {
    for runtime_fails in [true, false] {
        let (started, awaiting_start) = std::sync::mpsc::sync_channel(1);
        let (ready, awaiting_ready) = std::sync::mpsc::sync_channel(1);
        let stopped = AtomicBool::new(false);
        let runtime_stopped = &stopped;
        let result = parallel_model_preparation(
            move || {
                started.send(()).map_err(|error| error.to_string())?;
                awaiting_ready
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|error| error.to_string())?;
                if runtime_fails {
                    Err("Invalid runtime checksum.".into())
                } else {
                    wait_for_preparation_peer_failure(runtime_stopped)
                }
            },
            || {
                awaiting_start
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|error| error.to_string())?;
                ready.send(()).map_err(|error| error.to_string())?;
                if runtime_fails {
                    wait_for_preparation_peer_failure(&stopped).map(|()| Value::Null)
                } else {
                    Err("Broker build failed.".into())
                }
            },
        );
        assert_eq!(
            result,
            Err(if runtime_fails {
                "Invalid runtime checksum.".into()
            } else {
                "Broker build failed.".into()
            })
        );
        assert!(stopped.load(Ordering::Acquire));
        crate::startup_cancel::checkpoint()?;
    }
    Ok(())
}

#[test]
fn model_preparation_unwind_cancels_and_joins_the_other_arm() {
    let (started, awaiting_start) = std::sync::mpsc::sync_channel(1);
    let stopped = AtomicBool::new(false);
    let result = parallel_model_preparation(
        move || {
            awaiting_start
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            std::panic::resume_unwind(Box::new("Synthetic runtime preparation unwind."));
        },
        || {
            started.send(()).map_err(|error| error.to_string())?;
            wait_for_preparation_peer_failure(&stopped).map(|()| Value::Null)
        },
    );
    assert_eq!(result, Err("Model preparation worker panicked.".into()));
    assert!(stopped.load(Ordering::Acquire));
}

// Exercise backend selection/argument policy independently of the early window
// handoff. This records the fully prepared launch without a real UI or model.
fn execute(plan: Plan, operations: &mut impl Operations, working: &Path) -> crate::Result<Value> {
    let chat = plan.request.chat_path.clone();
    let models_only = plan.request.speech_models_only;
    let environment = graphics_environment(plan.request.graphics_backend);
    let prepared = prepare_backend(plan, operations, working)?;
    if models_only {
        return Ok(prepared);
    }
    let arguments = prepared["Arguments"]
        .as_array()
        .ok_or("Missing prepared arguments.")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(OsString::from)
                .ok_or_else(|| "Invalid argument.".to_owned())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let chat = chat.unwrap_or_else(|| working.parent().unwrap_or(working).join("built-chat.exe"));
    let code = operations.launch(&chat, &arguments, working, &environment)?;
    if code != 0 {
        return Err(format!("Chat stopped with exit code {code}."));
    }
    Ok(prepared)
}

#[derive(Debug, Eq, PartialEq)]
enum Event {
    Inference(Value),
    Models(String, bool),
    Apps(Vec<String>, bool),
    Native(String, bool),
    Keys(PathBuf),
    Launch(PathBuf, Vec<OsString>, PathBuf, BTreeMap<String, String>),
}

struct Recorder {
    root: PathBuf,
    events: Vec<Event>,
    artifact_requests: Vec<Value>,
    artifact_error: Option<&'static str>,
    exit_code: u32,
    fail_native: bool,
    fail_inference: bool,
    queue_update: bool,
}

impl Recorder {
    const fn new(root: PathBuf) -> Self {
        Self {
            root,
            events: Vec::new(),
            artifact_requests: Vec::new(),
            artifact_error: None,
            exit_code: 0,
            fail_native: false,
            fail_inference: false,
            queue_update: false,
        }
    }

    fn launch(&self) -> crate::Result<(&Path, &[OsString], &Path)> {
        self.events
            .iter()
            .find_map(|event| match event {
                Event::Launch(path, arguments, working, _) => {
                    Some((path.as_path(), arguments.as_slice(), working.as_path()))
                }
                _ => None,
            })
            .ok_or_else(|| "No recorded launch.".into())
    }
}

impl Operations for Recorder {
    fn artifacts(&mut self, request: &Request, _working: &Path) -> crate::Result<()> {
        assert!(
            self.launch().is_err(),
            "Artifacts must be ready before broker handoff."
        );
        self.artifact_requests
            .push(serde_json::to_value(request).map_err(|error| error.to_string())?);
        if let Some(error) = self.artifact_error {
            return Err(error.into());
        }
        Ok(())
    }

    fn inference(&mut self, request: &Request, working: &Path) -> crate::Result<()> {
        self.events
            .push(Event::Inference(inference_request(request, working)));
        if self.fail_inference {
            return Err("Deliberate inference preparation failure.".into());
        }
        Ok(())
    }

    fn models(&mut self, request: &Request, model: &str, _working: &Path) -> crate::Result<Value> {
        self.events
            .push(Event::Models(model.into(), request.speech_models_only));
        Ok(json!({
            "CanaryModelPath": self.root.join("canary.gguf"),
            "TtsModelPath": self.root.join("talker.gguf"),
            "TtsCodecPath": self.root.join("codec.gguf"),
            "TtsVoiceModelPath": self.root.join("voice.gguf"),
            "TotalBytes": 1,
        }))
    }

    fn apps(
        &mut self,
        request: &Request,
        packages: &[&str],
        _working: &Path,
    ) -> crate::Result<Value> {
        self.events.push(Event::Apps(
            packages.iter().map(ToString::to_string).collect(),
            request.force_build,
        ));
        Ok(json!({
            "ChatPath": self.root.join("built-chat.exe"),
            "BrokerPath": self.root.join("built-broker.exe"),
        }))
    }

    fn native(&mut self, request: &Request, model: &str, _working: &Path) -> crate::Result<Value> {
        self.events
            .push(Event::Native(model.into(), request.force_build));
        if self.fail_native {
            return Err("Deliberate native build failure.".into());
        }
        Ok(json!({
            "AsrPath": self.root.join("built-asr.exe"),
            "TtsPath": self.root.join("built-tts.exe"),
        }))
    }

    fn keys(&mut self, parent: &Path) -> crate::Result<PathBuf> {
        self.events.push(Event::Keys(parent.into()));
        Ok(parent.join("frontend.key"))
    }

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        working: &Path,
        environment: &BTreeMap<String, String>,
    ) -> crate::Result<u32> {
        if self.queue_update {
            let startup = PathBuf::from(value_after(arguments, "--startup-request")?);
            fs::write(startup.with_extension("update.json"), b"{}")
                .map_err(|error| error.to_string())?;
        }
        self.events.push(Event::Launch(
            executable.into(),
            arguments.to_vec(),
            working.into(),
            environment.clone(),
        ));
        Ok(self.exit_code)
    }
}

#[test]
fn installer_handoff_requires_acknowledged_update_exit_and_ignores_ordinary_close()
-> crate::Result<()> {
    for (code, queue, accepted, handed_off) in [
        (0, true, true, false),
        (42, false, false, false),
        (42, true, true, true),
        (7, true, false, false),
    ] {
        let fixture = fixture("update-exit-handshake")?;
        let mut recorder = Recorder::new(fixture.root.clone());
        recorder.exit_code = code;
        recorder.queue_update = queue;
        let result = launch_pending(
            plan(fixture.request)?.request,
            &mut recorder,
            &fixture.working,
            &fixture.root.join("tools.exe"),
        );
        assert_eq!(result.is_ok(), accepted);
        if let Ok(result) = result {
            assert_eq!(result.get("UpdateHandoff").is_some(), handed_off);
        }
    }
    Ok(())
}

struct Fixture {
    root: PathBuf,
    working: PathBuf,
    request: Value,
}

fn fixture(name: &str) -> crate::Result<Fixture> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "SwiftBonsai-ChatLaunchTests-{}-{time}-{name}",
        std::process::id()
    ));
    let working = root.join("session");
    for directory in [
        &root,
        &root.join("data"),
        &root.join("repository"),
        &working,
    ] {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    for name in [
        "chat.exe",
        "built-chat.exe",
        "broker.exe",
        "built-broker.exe",
        "asr.exe",
        "tts.exe",
        "external.key",
    ] {
        fs::write(root.join(name), b"fixture").map_err(|error| error.to_string())?;
    }
    let request = json!({
        "DataDirectory": root.join("data"), "RepositoryRoot": root.join("repository"),
        "WorkingDirectory": working, "ChatPath": root.join("chat.exe"),
        "BrokerPath": root.join("broker.exe"), "NativeEngine": false,
    });
    Ok(Fixture {
        root,
        working,
        request,
    })
}

fn plan(value: Value) -> crate::Result<Plan> {
    prepare(
        serde_json::from_value(value).map_err(|error| error.to_string())?,
        true,
    )
}

#[test]
fn kv_cache_survives_deferred_preparation_and_rejects_unsupported_selection() -> crate::Result<()> {
    let mut fixture = fixture("kv-cache")?;
    fixture.request["NativeEngine"] = json!(true);
    let default = plan(fixture.request.clone())?;
    let serialized = serde_json::to_vec(&default.request).map_err(|error| error.to_string())?;
    let restored: Request =
        serde_json::from_slice(&serialized).map_err(|error| error.to_string())?;
    assert!(restored.kv_cache.is_none());
    for value in ["f16", "rsq4"] {
        fixture.request["KvCache"] = json!(value);
        let prepared = plan(fixture.request.clone())?;
        let serialized =
            serde_json::to_vec(&prepared.request).map_err(|error| error.to_string())?;
        let restored: Request =
            serde_json::from_slice(&serialized).map_err(|error| error.to_string())?;
        let mut recorder = Recorder::new(fixture.root.clone());
        let result = prepare_backend(prepare(restored, true)?, &mut recorder, &fixture.working)?;
        let arguments = result["Arguments"].as_array().ok_or("Missing arguments.")?;
        assert_eq!(
            arguments
                .array_windows::<2>()
                .filter(|args| args[0] == "--kv-cache" && args[1] == value)
                .count(),
            1
        );
    }
    for invalid in [
        json!("F16"),
        json!("rsq4k3v"),
        json!(null),
        json!(true),
        json!(0),
    ] {
        fixture.request["KvCache"] = invalid;
        assert!(plan(fixture.request.clone()).is_err());
    }
    fixture.request["KvCache"] = json!("f16");
    fixture.request["NativeEngine"] = json!(false);
    assert!(plan(fixture.request.clone()).is_err());
    Ok(())
}

#[test]
fn prompt_reuse_survives_deferred_preparation_and_rejects_unsupported_selection()
-> crate::Result<()> {
    let mut fixture = fixture("prompt-reuse")?;
    fixture.request["NativeEngine"] = json!(true);
    for (value, latest) in [("disabled", false), ("latest", true)] {
        fixture.request["PromptReuse"] = json!(value);
        let prepared = plan(fixture.request.clone())?;
        let serialized =
            serde_json::to_vec(&prepared.request).map_err(|error| error.to_string())?;
        let restored: Request =
            serde_json::from_slice(&serialized).map_err(|error| error.to_string())?;
        let mut recorder = Recorder::new(fixture.root.clone());
        let result = prepare_backend(prepare(restored, true)?, &mut recorder, &fixture.working)?;
        let arguments = result["Arguments"].as_array().ok_or("Missing arguments.")?;
        assert_eq!(
            arguments
                .array_windows::<2>()
                .filter(|args| args[0] == "--prompt-reuse" && args[1] == "latest")
                .count(),
            usize::from(latest)
        );
    }
    for invalid in [
        json!("Latest"),
        json!("approximate"),
        json!(null),
        json!(true),
        json!(0),
    ] {
        fixture.request["PromptReuse"] = invalid;
        assert!(plan(fixture.request.clone()).is_err());
    }
    fixture.request["PromptReuse"] = json!("latest");
    fixture.request["NativeEngine"] = json!(false);
    assert!(plan(fixture.request.clone()).is_err());
    Ok(())
}

fn value_after(arguments: &[OsString], option: &str) -> crate::Result<OsString> {
    arguments
        .array_windows::<2>()
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("Missing launch option {option}."))
}

#[test]
fn development_default_and_explicit_rollback_survive_deferred_preparation() -> crate::Result<()> {
    for native in [None, Some(true), Some(false)] {
        for workers in [None, Some(1)] {
            let mut fixture = fixture("development-default")?;
            let object = fixture.request.as_object_mut().ok_or("Expected request.")?;
            object.remove("NativeEngine");
            if let Some(native) = native {
                object.insert("NativeEngine".into(), json!(native));
            }
            if let Some(workers) = workers {
                object.insert("NativeRowWorkers".into(), json!(workers));
            }
            object.insert("NoSpeechModels".into(), json!(true));
            let selected_native = native.unwrap_or(true);
            let selected_workers = workers.unwrap_or(if selected_native { 4 } else { 1 });
            let worker_argument = selected_workers.to_string();
            let mut recorder = Recorder::new(fixture.root.clone());
            launch_pending(
                plan(fixture.request)?.request,
                &mut recorder,
                &fixture.working,
                &fixture.root.join("maintenance.exe"),
            )?;
            let (_, arguments, _) = recorder.launch()?;
            let request_path = PathBuf::from(value_after(arguments, "--startup-request")?);
            let serialized: Value =
                serde_json::from_slice(&fs::read(request_path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            assert_eq!(serialized["NativeEngine"], selected_native);
            assert_eq!(serialized["NativeRowWorkers"], selected_workers);
            assert_eq!(serialized["UseRepackedPtq1"], false);
            assert_eq!(serialized["NoAbliterateLora"], false);
            let prepared = prepare_backend(plan(serialized)?, &mut recorder, &fixture.working)?;
            let arguments = prepared["Arguments"]
                .as_array()
                .ok_or("Missing arguments.")?;
            assert_eq!(
                arguments
                    .iter()
                    .filter(|arg| arg.as_str() == Some("--native-engine"))
                    .count(),
                usize::from(selected_native)
            );
            assert_eq!(
                arguments
                    .iter()
                    .filter(|arg| arg.as_str() == Some("--publisher-engine"))
                    .count(),
                usize::from(!selected_native)
            );
            assert!(
                arguments
                    .array_windows::<2>()
                    .any(|args| args[0] == "--native-row-workers" && args[1] == worker_argument)
            );
            assert!(arguments.iter().any(|arg| arg == "--use-original-pq2"));
            assert_eq!(
                recorder
                    .events
                    .iter()
                    .filter(|event| matches!(event, Event::Inference(_)))
                    .count(),
                usize::from(selected_native)
            );
        }
    }
    Ok(())
}

#[test]
fn native_runtime_preparation_uses_matching_lock_before_backend_work() -> crate::Result<()> {
    for source_broker in [false, true] {
        let mut fixture = fixture("runtime-preparation")?;
        fixture.request["NativeEngine"] = json!(true);
        if source_broker {
            fixture
                .request
                .as_object_mut()
                .ok_or("Expected request.")?
                .remove("BrokerPath");
        }
        let mut recorder = Recorder::new(fixture.root.clone());
        prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        let Some(Event::Inference(request)) = recorder.events.first() else {
            return Err("Runtime preparation must precede speech, builds and credentials.".into());
        };
        assert_eq!(request["operation"], "inference");
        assert_eq!(request["dataDirectory"], json!(fixture.root.join("data")));
        assert_eq!(request["workingDirectory"], json!(fixture.working));
        if source_broker {
            let expected = fixture
                .root
                .join("repository")
                .join("config")
                .join("prism-inference.lock.json");
            assert_eq!(
                request
                    .get("lockPath")
                    .and_then(Value::as_str)
                    .map(Path::new),
                Some(expected.as_path())
            );
        } else {
            assert!(request.get("lockPath").is_none());
        }
        assert!(request.get("verifyOnly").is_none());
    }
    Ok(())
}

#[test]
fn failed_native_runtime_preparation_stops_before_downloads_builds_and_keys() -> crate::Result<()> {
    let mut fixture = fixture("runtime-failure")?;
    fixture.request["NativeEngine"] = json!(true);
    let mut recorder = Recorder::new(fixture.root);
    recorder.fail_inference = true;
    assert!(prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working).is_err());
    assert_eq!(recorder.events.len(), 1);
    assert!(matches!(recorder.events[0], Event::Inference(_)));
    Ok(())
}

#[test]
fn native_window_handoff_precedes_backend_downloads_builds_and_keys() -> crate::Result<()> {
    for prebuilt_chat in [false, true] {
        let mut fixture = fixture(if prebuilt_chat {
            "early-prebuilt"
        } else {
            "early-build"
        })?;
        if !prebuilt_chat {
            fixture
                .request
                .as_object_mut()
                .ok_or("Expected request.")?
                .remove("ChatPath");
        }
        fixture.request["ForceBuild"] = json!(true);
        fixture.request["NativeEngine"] = json!(true);
        let plan = plan(fixture.request)?;
        let mut recorder = Recorder::new(fixture.root.clone());
        let tool = fixture.root.join("maintenance.exe");
        launch_pending(plan.request, &mut recorder, &fixture.working, &tool)?;
        assert_eq!(recorder.events.len(), if prebuilt_chat { 1 } else { 2 });
        if !prebuilt_chat {
            assert_eq!(
                recorder.events[0],
                Event::Apps(vec!["bonsai-chat".into()], true)
            );
        }
        let (_, arguments, _) = recorder.launch()?;
        assert_eq!(value_after(arguments, "--startup-tool")?, tool);
        assert_eq!(
            value_after(arguments, "--startup-data-directory")?,
            fixture.root.join("data")
        );
        let request_path = PathBuf::from(value_after(arguments, "--startup-request")?);
        let request: Request =
            serde_json::from_slice(&fs::read(request_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        assert!(request.chat_path.is_some());
        assert!(request.force_build);
        assert_eq!(
            request.working_directory.as_deref(),
            Some(fixture.working.as_path())
        );
        let deferred = prepare(request, true)?;
        assert!(!deferred.app_packages.contains(&"bonsai-chat"));
        assert_eq!(deferred.native_selection, Some("All"));
    }
    Ok(())
}

#[test]
fn weight_selection_survives_deferred_json_and_emits_one_explicit_app_flag() -> crate::Result<()> {
    for native in [false, true] {
        for repacked in [false, true] {
            let mut fixture = fixture("explicit-weights")?;
            fixture.request["NativeEngine"] = json!(native);
            fixture.request["UseRepackedPtq1"] = json!(repacked);
            fixture.request["NoSpeechModels"] = json!(true);
            let mut recorder = Recorder::new(fixture.root.clone());
            launch_pending(
                plan(fixture.request)?.request,
                &mut recorder,
                &fixture.working,
                &fixture.root.join("maintenance.exe"),
            )?;
            let (_, arguments, _) = recorder.launch()?;
            let request_path = PathBuf::from(value_after(arguments, "--startup-request")?);
            let serialized: Value =
                serde_json::from_slice(&fs::read(request_path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            // A present false value is the original-PQ2 selection, rather than
            // an omitted option that a later launcher default could reinterpret.
            assert_eq!(serialized.get("UseRepackedPtq1"), Some(&json!(repacked)));
            let prepared = prepare_backend(plan(serialized)?, &mut recorder, &fixture.working)?;
            let args = prepared["Arguments"]
                .as_array()
                .ok_or("Missing arguments.")?;
            assert_eq!(
                args.iter()
                    .filter(|arg| arg.as_str() == Some("--use-repacked-ptq1"))
                    .count(),
                usize::from(repacked)
            );
            assert_eq!(
                args.iter()
                    .filter(|arg| arg.as_str() == Some("--use-original-pq2"))
                    .count(),
                usize::from(!repacked)
            );
            assert!(!args.iter().any(|arg| arg == "--no-abliterate-lora"));
        }
    }
    Ok(())
}

#[test]
fn deferred_preparation_never_rebuilds_running_chat_and_keeps_backend_policy() -> crate::Result<()>
{
    let mut fixture = fixture("deferred-broker")?;
    fixture
        .request
        .as_object_mut()
        .ok_or("Expected request.")?
        .remove("BrokerPath");
    fixture.request["ForceBuild"] = json!(true);
    let mut recorder = Recorder::new(fixture.root);
    let prepared = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert!(
        recorder
            .events
            .contains(&Event::Apps(vec!["bonsai-broker".into()], true))
    );
    assert!(
        recorder
            .events
            .iter()
            .all(|event| !matches!(event, Event::Launch(..)))
    );
    assert!(
        prepared["Arguments"]
            .as_array()
            .ok_or("Missing arguments.")?
            .iter()
            .any(|arg| arg == "--broker-path")
    );
    assert_eq!(
        recorder
            .events
            .iter()
            .filter(|event| matches!(event, Event::Keys(_)))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn staged_model_preparation_never_waits_for_owned_or_external_speech() -> crate::Result<()> {
    for external in [false, true] {
        let mut fixture = fixture("staged-model")?;
        fixture.request["PreparationStage"] = json!("model");
        fixture.request["NativeEngine"] = json!(true);
        fixture.request["BrokerPath"] = Value::Null;
        if external {
            fixture.request["CanaryKeyFile"] = json!(fixture.root.join("external.key"));
            fixture.request["TtsKeyFile"] = json!(fixture.root.join("external.key"));
        }
        let mut recorder = Recorder::new(fixture.root.clone());
        recorder.fail_native = true;
        let prepared = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        assert_eq!(recorder.events.len(), 2);
        assert!(matches!(recorder.events[0], Event::Inference(_)));
        assert_eq!(
            recorder.events[1],
            Event::Apps(vec!["bonsai-broker".into()], false)
        );
        let arguments = prepared["Arguments"]
            .as_array()
            .ok_or("Missing arguments.")?;
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--native-engine")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--use-original-pq2")
        );
        assert!(arguments.iter().any(|argument| argument == "--broker-path"));
        assert!(arguments.iter().all(|argument| {
            argument.as_str().is_none_or(|argument| {
                !argument.starts_with("--canary-") && !argument.starts_with("--tts-")
            })
        }));
    }
    Ok(())
}

#[test]
fn staged_speech_preparation_preserves_services_without_repeating_model_work() -> crate::Result<()>
{
    for external in [false, true] {
        let mut fixture = fixture("staged-speech")?;
        fixture.request["PreparationStage"] = json!("speech");
        fixture.request["NativeEngine"] = json!(true);
        fixture.request["ForceBuild"] = json!(true);
        if external {
            fixture.request["CanaryKeyFile"] = json!(fixture.root.join("external.key"));
            fixture.request["TtsKeyFile"] = json!(fixture.root.join("external.key"));
        }
        let mut recorder = Recorder::new(fixture.root.clone());
        recorder.fail_inference = true;
        let prepared = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        if external {
            assert_eq!(recorder.events.len(), 0);
        } else {
            assert_eq!(
                recorder.events,
                [
                    Event::Models("All".into(), false),
                    Event::Native("All".into(), true),
                    Event::Keys(fixture.working.join("canary-credentials")),
                    Event::Keys(fixture.working.join("tts-credentials")),
                ]
            );
        }
        let arguments = prepared["Arguments"]
            .as_array()
            .ok_or("Missing arguments.")?;
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--canary-key-file")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--tts-key-file")
        );
        assert_eq!(
            arguments
                .iter()
                .any(|argument| argument == "--canary-executable"),
            !external
        );
        assert_eq!(
            arguments
                .iter()
                .any(|argument| argument == "--tts-executable"),
            !external
        );
    }
    Ok(())
}

#[test]
fn staged_preparation_failures_stop_before_later_work() -> crate::Result<()> {
    let mut fixture = fixture("staged-failures")?;
    fixture.request["NativeEngine"] = json!(true);
    fixture.request["PreparationStage"] = json!("model");
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.fail_inference = true;
    assert!(
        prepare_backend(
            plan(fixture.request.clone())?,
            &mut recorder,
            &fixture.working
        )
        .is_err()
    );
    assert_eq!(recorder.events.len(), 1);
    assert!(matches!(recorder.events[0], Event::Inference(_)));

    fixture.request["PreparationStage"] = json!("speech");
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.fail_native = true;
    assert!(
        prepare_backend(
            plan(fixture.request.clone())?,
            &mut recorder,
            &fixture.working
        )
        .is_err()
    );
    assert_eq!(
        recorder.events,
        [
            Event::Models("All".into(), false),
            Event::Native("All".into(), false)
        ]
    );
    fixture.request["BrokerPath"] = Value::Null;
    recorder.events.clear();
    let error = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)
        .err()
        .ok_or("Expected missing prepared broker failure.")?;
    assert!(error.contains("prepared broker"));
    assert_eq!(recorder.events.len(), 0);
    let mut invalid = self::fixture("staged-invalid")?;
    for stage in ["model", "speech"] {
        invalid.request["PreparationStage"] = json!(stage);
        invalid.request["SpeechModelsOnly"] = json!(true);
        assert!(plan(invalid.request.clone()).is_err());
    }
    invalid.request["SpeechModelsOnly"] = json!(false);
    invalid.request["PreparationStage"] = json!("unknown");
    assert!(plan(invalid.request).is_err());
    Ok(())
}

#[test]
fn early_window_failure_retains_nonzero_exit_and_skips_backend_work() -> crate::Result<()> {
    let fixture = fixture("early-failure")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.exit_code = 7;
    let error = launch_pending(
        plan(fixture.request)?.request,
        &mut recorder,
        &fixture.working,
        &fixture.root.join("maintenance.exe"),
    )
    .err()
    .ok_or("Expected native launch failure.")?;
    assert!(error.contains("exit code 7"));
    assert_eq!(recorder.events.len(), 1);
    assert!(matches!(recorder.events[0], Event::Launch(..)));
    Ok(())
}

#[test]
fn ordinary_startup_only_prepares_owned_services_and_creates_independent_keys() -> crate::Result<()>
{
    let fixture = fixture("owned")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert_eq!(
        &recorder.events[..4],
        &[
            Event::Models("All".into(), false),
            Event::Native("All".into(), false),
            Event::Keys(fixture.working.join("canary-credentials")),
            Event::Keys(fixture.working.join("tts-credentials")),
        ]
    );
    let (executable, arguments, working) = recorder.launch()?;
    assert_eq!(executable, fixture.root.join("chat.exe"));
    assert_eq!(working, fixture.working);
    assert_ne!(
        value_after(arguments, "--canary-key-file")?,
        value_after(arguments, "--tts-key-file")?
    );
    assert!(!arguments.iter().any(|value| value == "--use-repacked-ptq1"));
    assert!(!arguments.iter().any(|value| value == "--native-engine"));
    assert!(arguments.iter().any(|value| value == "--publisher-engine"));
    assert_eq!(value_after(arguments, "--native-row-workers")?, "1");
    assert_eq!(value_after(arguments, "--tts-quantization")?, "Q8_0");
    assert_eq!(
        value_after(arguments, "--tts-voice-model")?,
        fixture.root.join("voice.gguf").as_os_str()
    );
    Ok(())
}

#[test]
fn explicit_speech_files_and_text_only_never_build_or_download_speech() -> crate::Result<()> {
    for text_only in [false, true] {
        let mut fixture = fixture(if text_only { "text" } else { "external" })?;
        if text_only {
            fixture.request["NoSpeechModels"] = json!(true);
        } else {
            fixture.request["CanaryKeyFile"] = json!(fixture.root.join("external.key"));
            fixture.request["TtsKeyFile"] = json!(fixture.root.join("external.key"));
        }
        let mut recorder = Recorder::new(fixture.root.clone());
        execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        assert_eq!(recorder.events.len(), 1);
        let (_, arguments, _) = recorder.launch()?;
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--canary-executable")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--tts-executable")
        );
        assert_eq!(
            arguments
                .iter()
                .any(|argument| argument == "--tts-key-file"),
            !text_only
        );
    }
    Ok(())
}

#[test]
fn one_owned_service_does_not_prepare_the_other_engine() -> crate::Result<()> {
    for (property, value, expected) in [
        ("NoTts", json!(true), "QwenAsr"),
        ("CanaryKeyFile", Value::Null, "QwenTts"),
    ] {
        let mut fixture = fixture(expected)?;
        fixture.request[property] = if property == "CanaryKeyFile" {
            json!(fixture.root.join("external.key"))
        } else {
            value
        };
        let mut recorder = Recorder::new(fixture.root.clone());
        execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        assert_eq!(recorder.events[0], Event::Models(expected.into(), false));
        assert_eq!(recorder.events[1], Event::Native(expected.into(), false));
        assert_eq!(
            recorder
                .events
                .iter()
                .filter(|event| matches!(event, Event::Keys(_)))
                .count(),
            1
        );
    }
    Ok(())
}

#[test]
fn speech_models_only_fully_verifies_without_app_builds_keys_or_launch() -> crate::Result<()> {
    let mut fixture = fixture("models-only")?;
    fixture.request["SpeechModelsOnly"] = json!(true);
    fixture.request["DataDirectory"] = json!(fixture.root.join("uncreated-data"));
    let request: Request =
        serde_json::from_value(fixture.request).map_err(|error| error.to_string())?;
    let mut recorder = Recorder::new(fixture.root.clone());
    let result = execute(prepare(request, false)?, &mut recorder, &fixture.working)?;
    assert_eq!(recorder.events, [Event::Models("All".into(), true)]);
    assert_eq!(result["TotalBytes"], 1);
    assert!(fixture.root.join("uncreated-data").is_dir());
    Ok(())
}

#[test]
fn prebuilt_paths_skip_builds_and_all_profile_choices_are_forwarded() -> crate::Result<()> {
    let mut fixture = fixture("forwarded")?;
    fixture.request["AsrPath"] = json!(fixture.root.join("asr.exe"));
    fixture.request["TtsPath"] = json!(fixture.root.join("tts.exe"));
    fixture.request["TtsQuantization"] = json!("q4_k_m");
    fixture.request["TtsThreads"] = json!(10);
    fixture.request["UseRepackedPtq1"] = json!(true);
    fixture.request["NoAbliterateLora"] = json!(true);
    fixture.request["FrontendPort"] = json!(9001);
    fixture.request["BackendPort"] = json!(9002);
    fixture.request["CanaryPort"] = json!(9003);
    fixture.request["TtsPort"] = json!(9004);
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert!(
        !recorder
            .events
            .iter()
            .any(|event| matches!(event, Event::Apps(..) | Event::Native(..)))
    );
    let (_, arguments, _) = recorder.launch()?;
    for (option, expected) in [
        ("--tts-quantization", "Q4_K_M"),
        ("--tts-threads", "10"),
        ("--frontend-port", "9001"),
        ("--backend-port", "9002"),
        ("--canary-port", "9003"),
        ("--tts-port", "9004"),
    ] {
        assert_eq!(value_after(arguments, option)?, expected);
    }
    assert!(arguments.iter().any(|value| value == "--use-repacked-ptq1"));
    assert!(
        arguments
            .iter()
            .any(|value| value == "--no-abliterate-lora")
    );
    Ok(())
}

#[test]
fn missing_app_selection_and_force_build_reach_build_owners() -> crate::Result<()> {
    let mut fixture = fixture("build")?;
    fixture
        .request
        .as_object_mut()
        .ok_or("Expected request.")?
        .remove("ChatPath");
    fixture.request["ForceBuild"] = json!(true);
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert_eq!(
        recorder.events[1],
        Event::Apps(vec!["bonsai-chat".into()], true)
    );
    assert_eq!(recorder.events[2], Event::Native("All".into(), true));
    let (chat, arguments, _) = recorder.launch()?;
    assert_eq!(chat, fixture.root.join("built-chat.exe"));
    assert_eq!(
        value_after(arguments, "--broker-path")?,
        fixture.root.join("broker.exe").into_os_string()
    );
    Ok(())
}

#[test]
fn explicit_rust_runtime_survives_early_window_and_backend_preparation() -> crate::Result<()> {
    let mut fixture = fixture("native-runtime")?;
    fixture.request["NativeEngine"] = json!(true);
    fixture.request["NativeRowWorkers"] = json!(4);
    fixture.request["NoAbliterateLora"] = json!(true);
    fixture.request["NoSpeechModels"] = json!(true);
    let mut recorder = Recorder::new(fixture.root.clone());
    launch_pending(
        plan(fixture.request)?.request,
        &mut recorder,
        &fixture.working,
        &fixture.root.join("maintenance.exe"),
    )?;
    let (_, arguments, _) = recorder.launch()?;
    let request_path = PathBuf::from(value_after(arguments, "--startup-request")?);
    let request =
        serde_json::from_slice(&fs::read(request_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let deferred = prepare(request, true)?;
    assert!(deferred.request.native_engine && deferred.request.no_abliterate_lora);
    assert_eq!(deferred.request.native_row_workers, Some(4));
    let prepared = prepare_backend(deferred, &mut recorder, &fixture.working)?;
    let arguments = prepared["Arguments"]
        .as_array()
        .ok_or("Missing arguments.")?;
    assert!(arguments.iter().any(|value| value == "--native-engine"));
    assert!(
        arguments
            .array_windows::<2>()
            .any(|args| args[0] == "--native-row-workers" && args[1] == "4")
    );
    assert!(
        arguments
            .iter()
            .any(|value| value == "--no-abliterate-lora")
    );
    assert!(!arguments.iter().any(|value| value == "--use-repacked-ptq1"));
    Ok(())
}

#[test]
fn native_alternate_profiles_keep_verified_launch_options() -> crate::Result<()> {
    for (repacked, adapter) in [(true, false), (false, true), (true, true)] {
        let mut fixture = fixture("native-alternate")?;
        fixture.request["NativeEngine"] = json!(true);
        fixture.request["NoAbliterateLora"] = json!(!adapter);
        fixture.request["UseRepackedPtq1"] = json!(repacked);
        fixture.request["NoSpeechModels"] = json!(true);
        let mut recorder = Recorder::new(fixture.root);
        let result = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        let args = result["Arguments"]
            .as_array()
            .ok_or("Missing launch arguments.")?;
        assert!(args.iter().any(|value| value == "--native-engine"));
        assert_eq!(
            args.iter().any(|value| value == "--use-repacked-ptq1"),
            repacked
        );
        assert_eq!(
            args.iter().any(|value| value == "--no-abliterate-lora"),
            !adapter
        );
    }
    Ok(())
}

#[test]
fn invalid_options_reject_before_provisioning() -> crate::Result<()> {
    let fixture = fixture("invalid")?;
    for patch in [
        json!({"NativeRowWorkers": 4}),
        json!({"NativeEngine": true, "NativeRowWorkers": 2}),
        json!({"NativeEngine": true, "NativeRowWorkers": 0}),
        json!({"NativeEngine": true, "NativeRowWorkers": "4"}),
        json!({"NativeEngine": true, "NativeRowWorkers": 4.0}),
        json!({"NativeEngine": true, "NativeRowWorkers": null}),
        json!({"FrontendPort": 8081}),
        json!({"FrontendPort": 80}),
        json!({"CanaryPort": 5173}),
        json!({"CanaryPort": 8767}),
        json!({"SpeechModelsOnly": true, "NoSpeechModels": true}),
        json!({"NoTts": true, "TtsQuantization": "Q4_K_M"}),
        json!({"NoSpeechModels": true, "TtsThreads": 1}),
        json!({"NoTts": true, "TtsKeyFile": fixture.root.join("external.key")}),
        json!({"NoSpeechModels": true, "AsrPath": fixture.root.join("asr.exe")}),
        json!({"TtsThreads": 257}),
        json!({"ChatPath": fixture.root.join("external.key")}),
        json!({"DataDirectory": "relative-data"}),
        json!({"CanaryKeyFile": fixture.root.join("missing-key")}),
    ] {
        let mut request = fixture.request.clone();
        for (key, value) in patch.as_object().ok_or("Expected patch.")? {
            request[key] = value.clone();
        }
        assert!(plan(request).is_err(), "Accepted invalid options: {patch}");
    }
    let mut unknown = fixture.request.clone();
    unknown["ArbitraryRuntimeArguments"] = json!("--unsafe");
    assert!(serde_json::from_value::<Request>(unknown).is_err());
    let request = serde_json::from_value(fixture.request).map_err(|error| error.to_string())?;
    assert!(prepare(request, false).is_err());
    Ok(())
}

#[test]
fn automatic_setup_accepts_fresh_data_and_preserves_every_explicit_model_choice()
-> crate::Result<()> {
    let mut fixture = fixture("automatic-fresh-setup")?;
    let data = fixture.root.join("new-data-folder");
    fixture.request["DataDirectory"] = json!(data);
    fixture.request["NoSpeechModels"] = json!(true);
    fixture.request["PreparationStage"] = json!("model");
    assert!(!data.exists());
    for native in [false, true] {
        for ptq1 in [false, true] {
            for adapter in [false, true] {
                fixture.request["NativeEngine"] = json!(native);
                fixture.request["UseRepackedPtq1"] = json!(ptq1);
                fixture.request["NoAbliterateLora"] = json!(!adapter);
                let mut recorder = Recorder::new(fixture.root.clone());
                execute(
                    plan(fixture.request.clone())?,
                    &mut recorder,
                    &fixture.working,
                )?;
                assert!(data.is_dir());
                assert_eq!(recorder.artifact_requests.len(), 1);
                let setup = &recorder.artifact_requests[0];
                assert_eq!(setup["DataDirectory"], json!(data));
                assert_eq!(setup["NativeEngine"], native);
                assert_eq!(setup["UseRepackedPtq1"], ptq1);
                assert_eq!(setup["NoAbliterateLora"], !adapter);
                assert!(setup.get("LockPath").is_none());
                let (_, arguments, _) = recorder.launch()?;
                assert_eq!(
                    arguments
                        .iter()
                        .any(|argument| argument == "--native-engine"),
                    native
                );
                assert_eq!(
                    arguments
                        .iter()
                        .any(|argument| argument == "--publisher-engine"),
                    !native
                );
                assert_eq!(
                    arguments
                        .iter()
                        .any(|argument| argument == "--use-repacked-ptq1"),
                    ptq1
                );
                assert_eq!(
                    arguments
                        .iter()
                        .any(|argument| argument == "--no-abliterate-lora"),
                    !adapter
                );
            }
        }
    }
    fs::remove_dir_all(fixture.root).map_err(|error| error.to_string())
}

#[test]
fn automatic_setup_failure_never_hands_off_to_broker_or_speech() -> crate::Result<()> {
    let mut fixture = fixture("automatic-setup-failure")?;
    for native in [false, true] {
        fixture.request["NativeEngine"] = json!(native);
        let mut recorder = Recorder::new(fixture.root.clone());
        recorder.artifact_error = Some("Model download interrupted; partial retained.");
        let result = execute(
            plan(fixture.request.clone())?,
            &mut recorder,
            &fixture.working,
        );
        assert_eq!(
            result,
            Err("Model download interrupted; partial retained.".into())
        );
        assert_eq!(recorder.artifact_requests.len(), 1);
        assert!(recorder.launch().is_err());
        assert!(
            recorder
                .events
                .iter()
                .all(|event| matches!(event, Event::Inference(_)))
        );
    }
    fs::remove_dir_all(fixture.root).map_err(|error| error.to_string())
}

#[test]
fn speech_preparation_and_early_window_never_repeat_swift_setup() -> crate::Result<()> {
    let mut fixture = fixture("automatic-setup-boundaries")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    launch_pending(
        plan(fixture.request.clone())?.request,
        &mut recorder,
        &fixture.working,
        &fixture.root.join("tools.exe"),
    )?;
    assert!(recorder.artifact_requests.is_empty());

    fixture.request["PreparationStage"] = json!("speech");
    let mut recorder = Recorder::new(fixture.root.clone());
    prepare_backend(
        plan(fixture.request.clone())?,
        &mut recorder,
        &fixture.working,
    )?;
    assert!(recorder.artifact_requests.is_empty());
    assert!(
        recorder
            .events
            .iter()
            .all(|event| !matches!(event, Event::Inference(_)))
    );

    fixture.request["PreparationStage"] = json!("all");
    fixture.request["SpeechModelsOnly"] = json!(true);
    let mut recorder = Recorder::new(fixture.root.clone());
    prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert!(recorder.artifact_requests.is_empty());
    assert!(recorder.launch().is_err());
    assert_eq!(recorder.events, [Event::Models("All".into(), true)]);
    fs::remove_dir_all(fixture.root).map_err(|error| error.to_string())
}

#[test]
fn build_failure_and_nonzero_app_exit_remain_errors() -> crate::Result<()> {
    let fixture = fixture("failure")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.fail_native = true;
    assert!(
        execute(
            plan(fixture.request.clone())?,
            &mut recorder,
            &fixture.working
        )
        .is_err()
    );
    assert!(recorder.launch().is_err());
    let mut recorder = Recorder::new(fixture.root);
    recorder.exit_code = 7;
    let error = execute(plan(fixture.request)?, &mut recorder, &fixture.working)
        .err()
        .ok_or("Expected app exit failure.")?;
    assert!(error.contains("exit code 7"));
    Ok(())
}

#[test]
fn graphics_backend_selection_is_explicit_and_scoped_to_the_chat_child() -> crate::Result<()> {
    let fixture = fixture("graphics-backend")?;
    for backend in [None, Some("dx12"), Some("vulkan"), Some("gl")] {
        for native in [true, false] {
            let mut request = fixture.request.clone();
            request["NativeEngine"] = json!(native);
            if let Some(backend) = backend {
                request["GraphicsBackend"] = json!(backend);
            }
            let plan = plan(request)?;
            let mut recorder = Recorder::new(fixture.root.clone());
            launch_pending(
                plan.request,
                &mut recorder,
                &fixture.working,
                &fixture.root.join("tools.exe"),
            )?;
            let (_, arguments, _) = recorder.launch()?;
            let startup = PathBuf::from(value_after(arguments, "--startup-request")?);
            let serialized: Value =
                serde_json::from_slice(&fs::read(startup).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            assert_eq!(serialized["GraphicsBackend"], json!(backend));
            assert_eq!(serialized["NativeEngine"], native);
            let expected: BTreeMap<String, String> = backend
                .map(|value| (String::from("WGPU_BACKEND"), String::from(value)))
                .into_iter()
                .collect();
            assert!(recorder.events.iter().any(|event| {
                matches!(event, Event::Launch(_, _, _, environment) if environment == &expected)
            }));
        }
    }
    Ok(())
}

#[test]
fn graphics_backend_selection_rejects_unsupported_values_before_startup() -> crate::Result<()> {
    let fixture = fixture("invalid-graphics-backend")?;
    for backend in [
        json!(""),
        json!("metal"),
        json!("dx12,vulkan"),
        json!("DX12"),
        json!(false),
        json!(1),
    ] {
        let mut request = fixture.request.clone();
        request["GraphicsBackend"] = backend;
        assert!(plan(request).is_err());
    }
    Ok(())
}

#[test]
fn renderer_staging_failure_guidance_requires_the_recorded_panic() {
    for stage in ["index", "vertex"] {
        let log = format!(
            "thread 'main' panicked at C:\\registry\\egui-wgpu-0.37.0\\src\\renderer.rs:1033:17:\nFailed to create staging buffer for {stage} data."
        );
        let guidance = graphics_failure_guidance(101, &log);
        assert!(guidance.is_some_and(|value| value.contains("WGPU_BACKEND")));
        assert!(graphics_failure_guidance(1, &log).is_none());
    }
    for log in [
        "thread 'main' panicked at src/model.rs:10:1: unknown model error",
        "Failed to create staging buffer for index data.",
        "egui-wgpu-0.37.0 renderer.rs:1033 another error",
    ] {
        assert!(graphics_failure_guidance(101, log).is_none());
    }
}

#[test]
fn renderer_guidance_uses_only_the_selected_adapter_record() -> crate::Result<()> {
    let panic =
        "egui-wgpu-0.37.0 renderer.rs:1033:17:\nFailed to create staging buffer for index data.";
    for (backend, selected, alternative) in [
        ("Vulkan", "Vulkan was selected", "WGPU_BACKEND='dx12'"),
        ("Dx12", "DX12 was selected", "WGPU_BACKEND='vulkan'"),
    ] {
        let adapter = json!({
            "kind": "bonsai-chat-graphics", "stage": "adapter-selected", "backend": backend,
        });
        let log = format!("{adapter}\n{{malformed diagnostic\n{panic}");
        let guidance = graphics_failure_guidance(101, &log).ok_or("Missing graphics guidance.")?;
        assert!(guidance.contains(selected));
        assert!(guidance.contains(alternative));
        assert!(!guidance.contains("or 'vulkan'"));
    }
    for unrelated in [
        json!({"kind": "other", "stage": "adapter-selected", "backend": "Vulkan"}),
        json!({"kind": "bonsai-chat-graphics", "stage": "device-lost", "backend": "Vulkan"}),
    ] {
        let guidance = graphics_failure_guidance(101, &format!("{unrelated}\n{panic}"))
            .ok_or("Missing graphics guidance.")?;
        assert!(!guidance.contains("was selected"));
    }
    Ok(())
}
