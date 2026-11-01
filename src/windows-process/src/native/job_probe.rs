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
    JobObjectBasicProcessIdList, QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId};

pub(super) fn observe(process: &super::NativeProcess) -> String {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: The exact live Job and correctly sized writable native structure
    // remain owned for the complete synchronous query; no pointers escape.
    let accounting_result = super::check(unsafe {
        QueryInformationJobObject(
            process.job.as_raw_handle(),
            JobObjectBasicAccountingInformation,
            ptr::from_mut(&mut accounting).cast(),
            super::win_size::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>(),
            ptr::null_mut(),
        )
    });
    let accounting_observation = match accounting_result {
        Ok(()) => format!(
            "total={},active={},limit_terminated={}",
            accounting.TotalProcesses,
            accounting.ActiveProcesses,
            accounting.TotalTerminatedProcesses
        ),
        Err(error) => format!("accounting_error={:?}", error.raw_os_error()),
    };
    // Independent stages preserve earlier counters and raw IDs even if strict
    // cleanup enumeration cannot reconcile assigned/listed counts or open a PID.
    let (raw_observation, mut ids) = raw_members(process);
    let strict_result = retained_members(process);
    if let Ok(members) = &strict_result {
        for (id, _) in members {
            if ids.len() < 64 && !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    let metadata_result = snapshot_metadata(&ids);
    format!(
        "{accounting_observation}; raw_job_list={raw_observation}; retained_pid_exit={strict_result:?}; snapshot_pid_parent_basename={metadata_result:?}"
    )
}

fn raw_members(process: &super::NativeProcess) -> (String, Vec<u32>) {
    const CAPACITY: usize = 64;
    let header_units = 8_usize.div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; header_units + CAPACITY];
    // SAFETY: The usize-aligned initialized allocation contains the two DWORD
    // counts plus exactly CAPACITY pointer-sized IDs. Only one bounded query is
    // made; ERROR_MORE_DATA preserves partial raw diagnostic data without retry.
    let result = unsafe {
        QueryInformationJobObject(
            process.job.as_raw_handle(),
            JobObjectBasicProcessIdList,
            storage.as_mut_ptr().cast(),
            super::win_size::<[usize; 64]>() + 8,
            ptr::null_mut(),
        )
    };
    let error = (result == 0).then(io::Error::last_os_error);
    let error_code = error.as_ref().and_then(io::Error::raw_os_error);
    let valid = result != 0 || error_code == i32::try_from(super::ERROR_MORE_DATA).ok();
    // SAFETY: Initialized storage always includes the aligned eight-byte header.
    // A non-success/non-MORE_DATA query labels its fields unknown below.
    let [assigned, listed] = unsafe { ptr::read(storage.as_ptr().cast::<[u32; 2]>()) };
    let count = if valid {
        usize::try_from(listed).unwrap_or(CAPACITY).min(CAPACITY)
    } else {
        0
    };
    let raw_ids = &storage[header_units..header_units + count];
    let ids = raw_ids
        .iter()
        .filter_map(|id| u32::try_from(*id).ok())
        .collect();
    (
        format!(
            "query_ok={},win32={error_code:?},header_valid={valid},assigned={:?},listed={:?},raw_pids={raw_ids:?},listed_exceeds_buffer={}",
            result != 0,
            valid.then_some(assigned),
            valid.then_some(listed),
            valid && listed > 64
        ),
        ids,
    )
}

fn retained_members(process: &super::NativeProcess) -> io::Result<Vec<(u32, u32)>> {
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
    Ok(members)
}

fn snapshot_metadata(ids: &[u32]) -> io::Result<BTreeMap<u32, (u32, String)>> {
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
        if ids.contains(&entry.th32ProcessID) {
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
    Ok(metadata)
}
