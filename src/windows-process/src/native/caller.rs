//! Read-only parent identity for direct native maintenance invocations.

use std::io;
use std::os::windows::io::AsRawHandle as _;

use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

pub(crate) fn identity() -> io::Result<(u32, u64)> {
    // SAFETY: A process snapshot has no pointer arguments and returns a fresh
    // owning handle. The standard OwnedHandle closes it on every return path.
    let snapshot = super::owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            .map_err(|_| super::invalid("Process entry size overflow."))?,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: The live snapshot and correctly sized writable entry are retained.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &raw mut entry) };
    for _ in 0..65_536 {
        if found == 0 {
            return Err(io::Error::last_os_error());
        }
        if entry.th32ProcessID == std::process::id() {
            let id = entry.th32ParentProcessID;
            let creation = super::process_creation_time(id)?;
            let own_creation = super::process_creation_time(std::process::id())?;
            if creation > own_creation {
                return Err(super::invalid("The original caller PID has been reused."));
            }
            // Verify liveness and exact creation under a retained handle before
            // returning; command admission immediately retains its own guard.
            let parent = super::ParentProcess::open(id, creation)?;
            parent.ensure_alive()?;
            return Ok((id, creation));
        }
        // SAFETY: The same snapshot and initialized writable entry remain live.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &raw mut entry) };
    }
    Err(super::invalid(
        "Caller process inventory exceeds its bound.",
    ))
}
