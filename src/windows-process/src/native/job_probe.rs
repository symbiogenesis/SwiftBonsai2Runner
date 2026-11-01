//! Test-feature-only bounded failure context; never a serving admission rule.

use std::collections::BTreeMap;
use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::JobObjects::{
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
    QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId};

pub(super) fn observe(process: &super::NativeProcess) -> io::Result<String> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: The exact live Job and correctly sized writable native structure
    // remain owned for the complete synchronous query; no pointers escape.
    super::check(unsafe {
        QueryInformationJobObject(
            process.job.as_raw_handle(),
            JobObjectBasicAccountingInformation,
            ptr::from_mut(&mut accounting).cast(),
            super::win_size::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>(),
            ptr::null_mut(),
        )
    })?;
    // Reuse cleanup's bounded enumeration, retained query handles and exact Job
    // membership verification. These handles are never used to terminate by PID.
    let handles = process.members()?;
    if handles.len() > 64 {
        return Err(super::invalid(
            "Fixture Job members exceed their diagnostic bound.",
        ));
    }
    let mut members = Vec::with_capacity(handles.len());
    for handle in &handles {
        // SAFETY: The retained queryable process handle stays live throughout
        // both calls. The exit-code output is correctly sized and writable.
        let id = unsafe { GetProcessId(handle.as_raw_handle()) };
        if id == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut exit_code = 0;
        // SAFETY: The same retained handle and writable DWORD remain live.
        super::check(unsafe { GetExitCodeProcess(handle.as_raw_handle(), &raw mut exit_code) })?;
        members.push((id, exit_code));
    }
    // SAFETY: This read-only snapshot has no pointer arguments and returns a
    // fresh handle, closed by OwnedHandle on every success/error path.
    let snapshot = super::owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: super::win_size::<PROCESSENTRY32W>(),
        ..PROCESSENTRY32W::default()
    };
    let mut metadata = BTreeMap::new();
    // SAFETY: The live snapshot and correctly sized writable entry remain owned.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &raw mut entry) };
    let mut count = 0;
    while found != 0 {
        count += 1;
        if count > 16_384 {
            return Err(super::invalid(
                "Fixture process snapshot exceeds its diagnostic bound.",
            ));
        }
        if members.iter().any(|(id, _)| *id == entry.th32ProcessID) {
            let end = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .ok_or_else(|| super::invalid("Unterminated fixture process basename."))?;
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            let basename = Path::new(&name)
                .file_name()
                .ok_or_else(|| super::invalid("Missing fixture process basename."))?
                .to_string_lossy()
                .into_owned();
            metadata.insert(entry.th32ProcessID, (entry.th32ParentProcessID, basename));
        }
        // SAFETY: The same owned snapshot and initialized writable entry remain live.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &raw mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(18) {
        return Err(error);
    }
    let observations: Vec<_> = members
        .into_iter()
        .map(|(id, exit_code)| (id, exit_code, metadata.remove(&id)))
        .collect();
    Ok(format!(
        "total={},active={},limit_terminated={},members_pid_exit_parent_basename={observations:?}",
        accounting.TotalProcesses, accounting.ActiveProcesses, accounting.TotalTerminatedProcesses
    ))
}
