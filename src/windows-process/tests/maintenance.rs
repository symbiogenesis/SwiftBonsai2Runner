//! Model-free native observations used by bounded maintenance orchestration.
#![cfg(windows)]
#![forbid(unsafe_code)]

use std::path::Path;

#[test]
fn current_identity_retains_the_running_process() -> std::io::Result<()> {
    let created = bonsai_windows_process::current_process_creation_time()?;
    assert_ne!(created, 0);
    bonsai_windows_process::ParentProcess::open(std::process::id(), created)?.ensure_alive()?;
    assert!(bonsai_windows_process::ParentProcess::open(std::process::id(), created + 1).is_err());
    Ok(())
}

#[test]
fn observations_preserve_canonical_time_and_reject_nonlocal_storage() -> std::io::Result<()> {
    let stamp = bonsai_windows_process::utc_now();
    assert_eq!(stamp.len(), 28);
    for (index, expected) in [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'.'),
        (27, b'Z'),
    ] {
        assert_eq!(stamp.as_bytes()[index], expected);
    }
    for path in [
        "relative",
        "C:relative",
        "C:\\",
        "C:\\\\",
        "\\\\server.invalid\\share",
        "//?/C:/Bonsai",
    ] {
        assert!(bonsai_windows_process::require_local_data_drive(Path::new(path)).is_err());
    }
    bonsai_windows_process::require_local_data_drive(&std::env::temp_dir())?;
    assert!(bonsai_windows_process::available_disk_bytes(&std::env::temp_dir())? > 0);
    assert!(bonsai_windows_process::available_disk_bytes(Path::new("C:\\nul\0tail")).is_err());
    Ok(())
}

#[test]
fn shared_clock_and_process_snapshot_observe_the_live_test_process() -> std::io::Result<()> {
    let (first, frequency) = bonsai_windows_process::monotonic_ticks()?;
    let (second, next_frequency) = bonsai_windows_process::monotonic_ticks()?;
    assert!(frequency > 0 && second >= first);
    assert_eq!(frequency, next_frequency);
    let executable = std::env::current_exe()?;
    let name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("Missing test executable name."))?;
    assert!(bonsai_windows_process::has_running_process(&[name])?);
    assert!(bonsai_windows_process::has_running_process(&[""]).is_err());
    Ok(())
}
