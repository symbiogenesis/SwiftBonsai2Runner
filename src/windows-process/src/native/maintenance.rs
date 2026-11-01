//! Small operating-system observations for native maintenance commands.

use std::io;
use std::path::Path;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
use windows_sys::Win32::System::SystemInformation::GetSystemTime;

pub(crate) fn current_creation_time() -> io::Result<u64> {
    // SAFETY: The process ID belongs to this live process; the returned handle is owned below.
    let raw = unsafe {
        super::OpenProcess(
            super::PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            std::process::id(),
        )
    };
    let handle = super::owned(raw)?;
    super::creation_file_time(&handle)
}

pub(crate) fn utc_now() -> String {
    let mut time = SYSTEMTIME::default();
    // SAFETY: The initialized structure is writable for the complete call.
    unsafe { GetSystemTime(&raw mut time) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}0000Z",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

pub(crate) fn local_drive(path: &Path) -> io::Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| super::invalid("Use a Unicode local drive path."))?;
    let bytes = text.as_bytes();
    if bytes.len() <= 3
        || path.parent().is_none()
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'/' | b'\\')
    {
        return Err(super::invalid(
            "Use a dedicated absolute local drive directory.",
        ));
    }
    let root = [u16::from(bytes[0]), u16::from(b':'), u16::from(b'\\'), 0];
    // SAFETY: root is a terminated three-character drive-root string.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    if !matches!(kind, 2 | 3 | 6) {
        return Err(super::invalid(
            "The directory must be on a fixed, removable or RAM disk.",
        ));
    }
    Ok(())
}

pub(crate) fn available_disk_bytes(path: &Path) -> io::Result<u64> {
    let path = super::wide(path.as_os_str());
    if path[..path.len() - 1].contains(&0) {
        return Err(super::invalid("Disk query path contains a NUL."));
    }
    let mut available = 0;
    // SAFETY: path is terminated and available points to a writable u64; optional outputs are null.
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &raw mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}
