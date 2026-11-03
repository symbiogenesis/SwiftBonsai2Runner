use super::*;
use bonsai_windows_credentials::PrivateDirectory;

fn storage() -> io::Result<PrivateDirectory> {
    let parent: PathBuf = std::env::temp_dir().components().collect();
    PrivateDirectory::create(&parent)
}

fn arguments(directory: &PrivateDirectory) -> Vec<OsString> {
    vec![
        "--launch-development".into(),
        OPTION.into(),
        directory.path().as_os_str().to_owned(),
        "--native-engine".into(),
    ]
}

#[test]
fn owner_shutdown_arguments_preserve_launch_selection_and_reject_ambiguous_storage()
-> io::Result<()> {
    let original = vec![OsString::from("--native-engine")];
    let (remaining, control) = OwnerShutdown::take_arguments(original.clone())?;
    assert_eq!(remaining, original);
    assert!(control.is_none());
    let directory = storage()?;
    let (remaining, control) = OwnerShutdown::take_arguments(arguments(&directory))?;
    let expected: Vec<OsString> = ["--launch-development", "--native-engine"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert_eq!(remaining, expected);
    assert!(control.is_some());
    let mut duplicate = arguments(&directory);
    duplicate.extend([OPTION.into(), directory.path().as_os_str().to_owned()]);
    for invalid_args in [
        duplicate,
        vec![OPTION.into()],
        vec![OPTION.into(), "relative-directory".into()],
    ] {
        assert!(OwnerShutdown::take_arguments(invalid_args).is_err());
    }
    assert!(fs::read_dir(directory.path())?.next().is_none());
    Ok(())
}

#[test]
fn owner_shutdown_early_request_survives_startup_intake_and_requires_empty_regular_file()
-> io::Result<()> {
    let directory = storage()?;
    let request = directory.path().join(REQUEST);
    fs::write(&request, [])?;
    let (_, control) = OwnerShutdown::take_arguments(arguments(&directory))?;
    let control = control.ok_or_else(invalid)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(100), control.requested())
            .await
            .map_err(io::Error::other)?
    })?;
    fs::write(&request, b"not-a-shutdown-signal")?;
    assert!(empty_signal(&request).is_err());
    assert!(OwnerShutdown::take_arguments(arguments(&directory)).is_err());
    fs::remove_file(&request)?;
    fs::create_dir(&request)?;
    assert!(empty_signal(&request).is_err());
    Ok(())
}

#[test]
fn owner_shutdown_same_pending_future_observes_request_after_startup_handoff() -> io::Result<()> {
    let directory = storage()?;
    let (_, control) = OwnerShutdown::take_arguments(arguments(&directory))?;
    let control = control.ok_or_else(invalid)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut shutdown = pin!(control.requested());
        assert!(poll_fn(|cx| Poll::Ready(shutdown.as_mut().poll(cx).is_pending())).await);
        fs::write(directory.path().join(REQUEST), [])?;
        tokio::time::timeout(Duration::from_millis(150), shutdown)
            .await
            .map_err(io::Error::other)?
    })
}

#[test]
fn owner_shutdown_confirmation_is_fresh_atomic_and_never_reused_by_another_attempt()
-> io::Result<()> {
    let directory = storage()?;
    let (_, control) = OwnerShutdown::take_arguments(arguments(&directory))?;
    let control = control.ok_or_else(invalid)?;
    assert!(!empty_signal(&directory.path().join(CONFIRMED))?);
    control.confirm_cleanup()?;
    assert!(empty_signal(&directory.path().join(CONFIRMED))?);
    assert_eq!(
        control.confirm_cleanup().err().map(|error| error.kind()),
        Some(io::ErrorKind::AlreadyExists)
    );
    assert!(OwnerShutdown::take_arguments(arguments(&directory)).is_err());
    let other = storage()?;
    assert!(!empty_signal(&other.path().join(CONFIRMED))?);
    fs::write(other.path().join("unexpected"), [])?;
    assert!(OwnerShutdown::take_arguments(arguments(&other)).is_err());
    Ok(())
}

#[test]
fn owner_shutdown_creation_and_stop_failures_cannot_confirm_model_release() {
    for error in [
        LaunchError::Process,
        LaunchError::Sandbox,
        LaunchError::Stop,
    ] {
        assert!(!startup_cleanup_confirmed(error));
    }
    for error in [
        LaunchError::Memory,
        LaunchError::Artifacts,
        LaunchError::NativeRuntime,
        LaunchError::Cancelled,
        LaunchError::Backend,
        LaunchError::Service,
    ] {
        assert!(startup_cleanup_confirmed(error));
    }
}
