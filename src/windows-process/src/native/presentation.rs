//! Presentation at process startup, isolated from the command protocol.

use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType},
    System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE},
    UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
};

pub(crate) fn attach_parent_console() {
    // SAFETY: The predefined standard-handle selector takes no caller pointers.
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if !output.is_null() && output != INVALID_HANDLE_VALUE {
        // SAFETY: The borrowed handle is queried only and is never closed here.
        if matches!(
            unsafe { GetFileType(output) },
            FILE_TYPE_DISK | FILE_TYPE_PIPE
        ) {
            return;
        }
    }
    // SAFETY: This predefined selector attaches only to an existing parent
    // console. Failure is ordinary when no terminal is available.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

pub(crate) fn show_error(message: &str) {
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    // SAFETY: Both terminated strings remain live throughout the synchronous
    // modal call; no parent or retained pointer is supplied.
    let _ = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            windows_sys::w!("Swift Bonsai 2 Runner"),
            MB_OK | MB_ICONERROR,
        )
    };
}
