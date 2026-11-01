//! Small operating-system observations for native maintenance commands.

use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::SystemInformation::GetSystemTime;

pub(crate) fn monotonic_ticks() -> io::Result<(u64, u64)> {
    let mut ticks = 0;
    let mut frequency = 0;
    // SAFETY: Both calls receive live writable i64 outputs and retain no pointers.
    super::check(unsafe { QueryPerformanceCounter(&raw mut ticks) })?;
    // SAFETY: The frequency output remains writable for the complete synchronous call.
    super::check(unsafe { QueryPerformanceFrequency(&raw mut frequency) })?;
    let ticks = u64::try_from(ticks).map_err(|_| super::invalid("Negative monotonic counter."))?;
    let frequency = u64::try_from(frequency)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| super::invalid("Invalid monotonic frequency."))?;
    Ok((ticks, frequency))
}

pub(crate) fn has_running_process(names: &[&str]) -> io::Result<bool> {
    if names.len() > 64
        || names
            .iter()
            .any(|name| name.is_empty() || !name.is_ascii() || name.len() > 260)
    {
        return Err(super::invalid(
            "Process-name selection exceeds its fixed bound.",
        ));
    }
    // SAFETY: A process-only snapshot takes no caller pointers and returns a fresh handle.
    let snapshot = super::owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            .map_err(|_| super::invalid("Process entry size overflow."))?,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: snapshot remains owned; entry is correctly sized and writable.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &raw mut entry) };
    let mut count = 0;
    while found != 0 {
        count += 1;
        if count > 16_384 {
            return Err(super::invalid("Process inventory exceeds its fixed bound."));
        }
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| super::invalid("Unterminated process name."))?;
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if names
            .iter()
            .any(|selected| name.eq_ignore_ascii_case(selected))
        {
            return Ok(true);
        }
        // SAFETY: The same owned snapshot and initialized entry remain live for the next record.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &raw mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(18) {
        return Err(error);
    }
    Ok(false)
}

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
