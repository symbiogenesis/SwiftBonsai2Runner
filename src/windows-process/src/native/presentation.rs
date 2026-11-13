//! Presentation at process startup, isolated from the command protocol.

use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType},
    System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE},
};

mod error_dialog;

pub(crate) fn attach_parent_console() {
    // SAFETY: The predefined standard-handle selector takes no caller pointers.
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if !output.is_null() && output != INVALID_HANDLE_VALUE {
        // SAFETY: The borrowed handle is queried only and is never closed here.
        let output_type = unsafe { GetFileType(output) };
        if matches!(output_type, FILE_TYPE_DISK | FILE_TYPE_PIPE) {
            return;
        }
    }
    // SAFETY: This predefined selector attaches only to an existing parent
    // console. Failure is ordinary when no terminal is available.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

pub(crate) fn show_error(message: &str) {
    error_dialog::show(message);
}
