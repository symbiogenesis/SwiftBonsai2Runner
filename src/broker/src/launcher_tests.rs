//! Synthetic owned-backend lifecycle regressions; no model process is executed.

use super::*;
use std::{
    fs,
    future::pending,
    io::{Read, Write},
    net::{TcpListener as BlockingListener, TcpStream as BlockingStream},
};

const PROBE_NAME: &str = "launcher::lifecycle_tests::synthetic_backend_probe";
const PROBE_MEMORY: usize = 512 * 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    process: OwnedProcess,
    credentials: PrivateCredentials,
    port: u16,
}

fn probe() -> io::Result<Fixture> {
    let parent: PathBuf = std::env::temp_dir().components().collect();
    let credentials = PrivateCredentials::create(&parent)?;
    let reservation = BlockingListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = reservation.local_addr()?.port();
    fs::write(credentials.directory().join("probe.port"), port.to_string())?;
    let specification = ProcessSpec {
        executable: std::env::current_exe()?,
        arguments: ["--exact", PROBE_NAME, "--ignored", "--nocapture"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        working_directory: credentials.directory().to_path_buf(),
        stdout: credentials.directory().join("probe.stdout"),
        stderr: credentials.directory().join("probe.stderr"),
        memory_limit_bytes: PROBE_MEMORY,
        active_process_limit: 1,
    };
    drop(reservation);
    let process = OwnedProcess::start(&specification)?;
    Ok(Fixture {
        process,
        credentials,
        port,
    })
}

fn check(test: impl Future<Output = io::Result<()>>) -> io::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            timeout(PROBE_TIMEOUT, test).await.map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "Lifecycle probe timed out.")
            })?
        })
}

#[test]
fn missing_native_runtime_is_distinct_from_missing_model_artifacts() -> io::Result<()> {
    let data = std::env::temp_dir().join(format!(
        "Bonsai-NativeRuntimePreflight-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos()
    ));
    fs::create_dir(&data)?;
    let mut config = LaunchConfig::parse([
        OsString::from("--launch-development"),
        "--allow-unisolated-backend".into(),
        "--native-engine".into(),
        "--profile".into(),
        "swift-pq2-assessment".into(),
        "--data-directory".into(),
        data.as_os_str().to_owned(),
        "--evidence-parent".into(),
        data.as_os_str().to_owned(),
        "--frontend-port".into(),
        "0".into(),
        "--backend-port".into(),
        "8081".into(),
    ])
    .map_err(|_| io::Error::other("Invalid fixture launch configuration."))?;
    assert_eq!(
        config.native_executable_guard().err(),
        Some(LaunchError::NativeRuntime)
    );
    let plan = LaunchPlan::for_profile_with_options(
        &data,
        config.backend_port,
        &data.join("not-created.key"),
        config.profile,
        false,
        false,
    )
    .map_err(|_| io::Error::other("Invalid fixture launch policy."))?;
    assert_eq!(
        config.verify_artifacts(&plan).err(),
        Some(LaunchError::Artifacts)
    );
    config.native_engine = false;
    assert!(
        config
            .native_executable_guard()
            .is_ok_and(|value| value.is_none())
    );
    fs::remove_dir(data)?;
    Ok(())
}

#[test]
fn owned_ready_backend_stops_with_service() -> io::Result<()> {
    check(async {
        let Fixture {
            process,
            credentials,
            port,
        } = probe()?;
        assert!(process.try_wait()?.is_none());
        assert_eq!(
            wait_ready(
                &process,
                port,
                credentials.backend_key(),
                PROBE_MEMORY,
                pending()
            )
            .await,
            Ok(())
        );
        assert_eq!(listener_process_id(port)?, Some(process.id()));
        assert!(healthy(port, credentials.backend_key(), process.id()).await);
        assert!(authenticated(port, credentials.backend_key(), process.id()).await);
        let wrong_key = if credentials.backend_key().starts_with('0') {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        };
        assert!(!authenticated(port, &wrong_key, process.id()).await);
        let mut service = LocalChatService::bind(
            0,
            credentials.frontend_key(),
            port,
            credentials.backend_key(),
            0,
        )
        .await
        .map_err(|_| io::Error::other("Synthetic service bind failed."))?;
        service.require_backend_process(process.id());
        let launched = LaunchedService {
            service,
            process,
            credentials,
            _artifacts: VerifiedArtifacts::synthetic_for_test(),
            _adapter: None,
            _native_executable: None,
            memory_limit_bytes: PROBE_MEMORY,
            backend_port: port,
        };
        let frontend_port = launched.port();
        let report = launched
            .run(async { Ok(()) })
            .await
            .map_err(|_| io::Error::other("Synthetic service cleanup failed."))?;
        assert_eq!(report.exit, RunExit::Shutdown);
        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.peak_in_flight, 0);
        let resources = report
            .backend_resources
            .ok_or_else(|| io::Error::other("Missing owned resource observation."))?;
        assert!(
            resources
                .os_peak_job_commit_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert!(
            resources
                .os_peak_root_working_set_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert_eq!(listener_process_id(port)?, None);
        assert_eq!(listener_process_id(frontend_port)?, None);
        Ok(())
    })
}

#[test]
fn resource_observations_keep_sampled_and_lifetime_counters_distinct() {
    let observed = BackendResources {
        os_peak_job_commit_bytes: Some(1000),
        os_peak_root_working_set_bytes: Some(900),
        ..BackendResources::default()
    };
    let first = resource_sample(
        observed,
        ProcessMemory {
            private_bytes: 80,
            working_set_bytes: 70,
            peak_working_set_bytes: 900,
        },
        5000,
    );
    let second = resource_sample(
        first,
        ProcessMemory {
            private_bytes: 60,
            working_set_bytes: 85,
            peak_working_set_bytes: 900,
        },
        6000,
    );
    assert_eq!(second.sample_count, 2);
    assert_eq!(second.sampled_peak_root_private_bytes, 80);
    assert_eq!(second.sampled_peak_root_working_set_bytes, 85);
    assert_eq!(second.sampled_min_available_physical_bytes, Some(5000));
    assert_eq!(second.os_peak_job_commit_bytes, Some(1000));
    assert_eq!(second.os_peak_root_working_set_bytes, Some(900));
    let saturated = resource_sample(
        BackendResources {
            sample_count: u64::MAX,
            ..second
        },
        ProcessMemory {
            private_bytes: 0,
            working_set_bytes: 0,
            peak_working_set_bytes: 900,
        },
        4000,
    );
    assert_eq!(saturated.sample_count, u64::MAX);
    assert_eq!(saturated.sampled_min_available_physical_bytes, Some(4000));
    assert_eq!(saturated.sampled_peak_root_private_bytes, 80);
}

#[test]
fn readiness_rejects_another_listener_and_cancellation() -> io::Result<()> {
    check(async {
        let fixture = probe()?;
        let impostor = BlockingListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let other_port = impostor.local_addr()?.port();
        assert_eq!(listener_process_id(other_port)?, Some(std::process::id()));
        assert_eq!(
            wait_ready(
                &fixture.process,
                other_port,
                fixture.credentials.backend_key(),
                PROBE_MEMORY,
                pending()
            )
            .await,
            Err(LaunchError::Backend)
        );
        assert_eq!(
            wait_ready(
                &fixture.process,
                fixture.port,
                fixture.credentials.backend_key(),
                PROBE_MEMORY,
                async { Ok(()) }
            )
            .await,
            Err(LaunchError::Cancelled)
        );
        fixture.process.stop(STOP_TIMEOUT)?;
        assert!(fixture.process.try_wait()?.is_some());
        assert_eq!(listener_process_id(fixture.port)?, None);
        Ok(())
    })
}

// The parent invokes this ignored test in a separate, atomically Job-owned child.
// Port discovery uses the protected native cwd; inherited environment is untouched.
#[test]
#[ignore = "subprocess entry point; invoked only with a protected fixture cwd"]
fn synthetic_backend_probe() -> io::Result<()> {
    let directory = std::env::current_dir()?;
    let port = fs::read_to_string(directory.join("probe.port"))?
        .parse::<u16>()
        .map_err(io::Error::other)?;
    let key = fs::read_to_string(directory.join("backend.key"))?;
    if key.len() != 64 || port < 1024 {
        return Err(io::Error::other("Invalid synthetic backend fixture."));
    }
    let listener = BlockingListener::bind((Ipv4Addr::LOCALHOST, port))?;
    for incoming in listener.incoming() {
        serve_probe(&mut incoming?, &key)?;
    }
    Ok(())
}

fn serve_probe(stream: &mut BlockingStream, key: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut buffer)?;
        if count == 0 || request.len() + count > 16 * 1024 {
            return Err(io::Error::other("Invalid synthetic backend request."));
        }
        request.extend_from_slice(&buffer[..count]);
    }
    let request = std::str::from_utf8(&request).map_err(io::Error::other)?;
    let authorized = request
        .split("\r\n")
        .any(|line| line == format!("Authorization: Bearer {key}"));
    let (status, body) = if request.starts_with("GET /health HTTP/1.1\r\n") {
        ("200 OK", r#"{"status":"ok"}"#)
    } else if request.starts_with("GET /v1/models HTTP/1.1\r\n") && authorized {
        ("200 OK", r#"{"data":[]}"#)
    } else {
        ("401 Unauthorized", r#"{"error":"unauthorized"}"#)
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}
