//! Fixed native startup failure UI, independent of graphics initialization.

use std::ptr;
use windows_sys::{
    Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND, LPARAM, WPARAM},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            LibraryLoader::GetModuleHandleW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
        UI::WindowsAndMessaging::{
            BS_DEFPUSHBUTTON, DLGTEMPLATE, DS_CENTER, DS_MODALFRAME, DS_SETFONT,
            DialogBoxIndirectParamW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, EndDialog,
            GWLP_USERDATA, GetWindowLongPtrW, IDCANCEL, IDOK, MB_ICONERROR, MB_OK, MessageBoxW,
            SetDlgItemTextW, SetWindowLongPtrW, WM_CLOSE, WM_COMMAND, WM_INITDIALOG, WS_BORDER,
            WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
    core::w,
};

const PREVIEW_ID: u16 = 1000;
const COPY_ID: u16 = 1001;
const FEEDBACK_ID: u16 = 1002;
const PREVIEW_CHARS: usize = 2048;

struct Contents {
    full: Vec<u16>,
    preview: Vec<u16>,
}

fn terminated(text: &str) -> Vec<u16> {
    // A diagnostic containing NUL must not silently lose everything after it
    // when Windows consumes null-terminated text or CF_UNICODETEXT.
    text.replace('\0', "\\0")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn preview(message: &str, limit: usize) -> String {
    let mut characters = message.chars();
    let mut text: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        text.push_str("\n\n[Preview shortened. Copy error includes the complete message.]");
    }
    text
}

pub(super) fn show(message: &str) {
    let contents = Contents {
        full: terminated(message),
        preview: terminated(&preview(message, PREVIEW_CHARS)),
    };
    let template = template();
    let parameter = isize::try_from(ptr::from_ref(&contents).expose_provenance());
    // SAFETY: A null name requests this process's module without transferring ownership.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let result = parameter.map_or(-1, |parameter| {
        // SAFETY: The DWORD-aligned template contains only standard native controls.
        // All template/text storage and the callback context remain live until this
        // synchronous modal call returns. The callback never retains these pointers.
        unsafe {
            DialogBoxIndirectParamW(
                instance,
                template.as_ptr().cast::<DLGTEMPLATE>(),
                ptr::null_mut(),
                Some(dialog_proc),
                parameter,
            )
        }
    });
    if result == -1 {
        // Even a platform failure must not put an unbounded diagnostic in a message box.
        let short: String = message
            .chars()
            .take(160)
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        let fallback = terminated(&format!(
            "Swift Bonsai could not start.\n\n{short}\n\nThe detailed error controls could not open."
        ));
        // SAFETY: Both terminated strings remain live throughout the synchronous call.
        let _ = unsafe {
            MessageBoxW(
                ptr::null_mut(),
                fallback.as_ptr(),
                w!("Swift Bonsai 2"),
                MB_OK | MB_ICONERROR,
            )
        };
    }
}

unsafe extern "system" fn dialog_proc(
    dialog: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    if message == WM_INITDIALOG {
        // SAFETY: This dialog owns its user-data slot and receives the live Contents
        // pointer only from the synchronous DialogBoxIndirectParamW invocation above.
        let _ = unsafe { SetWindowLongPtrW(dialog, GWLP_USERDATA, lparam) };
    }
    // SAFETY: Only WM_INITDIALOG above writes this dialog's private user-data slot.
    let parameter = unsafe { GetWindowLongPtrW(dialog, GWLP_USERDATA) };
    let pointer =
        ptr::with_exposed_provenance::<Contents>(usize::try_from(parameter).unwrap_or_default());
    if !pointer.is_null() {
        // SAFETY: The caller owns Contents for the complete modal lifetime. The
        // callback borrows it immutably and makes no reentrant mutation of it.
        let contents = unsafe { &*pointer };
        if message == WM_INITDIALOG {
            // SAFETY: PREVIEW_ID identifies the template's edit control; text stays live.
            let _ = unsafe {
                SetDlgItemTextW(dialog, i32::from(PREVIEW_ID), contents.preview.as_ptr())
            };
            return 1;
        }
        if message == WM_COMMAND && wparam & 0xffff == usize::from(COPY_ID) {
            let feedback = if copy(dialog, &contents.full) {
                w!("Copied the complete error.")
            } else {
                w!("Clipboard unavailable. Try Copy error again.")
            };
            // SAFETY: The template's fixed feedback control copies this static text.
            let _ = unsafe { SetDlgItemTextW(dialog, i32::from(FEEDBACK_ID), feedback) };
            return 1;
        }
    }
    if message == WM_CLOSE
        || (message == WM_COMMAND && matches!(i32::try_from(wparam & 0xffff), Ok(IDOK | IDCANCEL)))
    {
        // SAFETY: This callback runs only for its own live modal dialog.
        let _ = unsafe { EndDialog(dialog, 0) };
        return 1;
    }
    0
}

struct Clipboard;

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after OpenClipboard succeeded on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

struct Allocation(HGLOBAL);

impl Drop for Allocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Non-null storage is unlocked and still owned by this guard;
            // successful SetClipboardData clears it before Windows takes ownership.
            let _ = unsafe { GlobalFree(self.0) };
        }
    }
}

fn copy(dialog: HWND, text: &[u16]) -> bool {
    let Some(bytes) = text.len().checked_mul(size_of::<u16>()) else {
        return false;
    };
    // SAFETY: The byte count is checked and GMEM_MOVEABLE is required for clipboard transfer.
    let mut allocation = Allocation(unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) });
    if allocation.0.is_null() {
        return false;
    }
    // SAFETY: This is a live, exclusively owned movable allocation.
    let destination = unsafe { GlobalLock(allocation.0) }.cast::<u16>();
    if destination.is_null() {
        return false;
    }
    // SAFETY: The source and newly allocated destination do not overlap; the
    // checked allocation size covers every UTF-16 unit including the terminator.
    unsafe { ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len()) };
    // SAFETY: The allocation has exactly the one lock obtained above.
    let _ = unsafe { GlobalUnlock(allocation.0) };
    // SAFETY: The live dialog provides the valid owner required by EmptyClipboard.
    if unsafe { OpenClipboard(dialog) } == 0 {
        return false;
    }
    let _clipboard = Clipboard;
    // SAFETY: This thread holds the clipboard open and intentionally replaces its contents.
    if unsafe { EmptyClipboard() } == 0 {
        return false;
    }
    // SAFETY: Format 13 is CF_UNICODETEXT; unlocked movable storage contains
    // terminated UTF-16. Ownership transfers only when this call succeeds.
    if unsafe { SetClipboardData(13, allocation.0) }.is_null() {
        return false;
    }
    allocation.0 = ptr::null_mut();
    true
}

fn push_dword(words: &mut Vec<u16>, value: u32) {
    let bytes = value.to_le_bytes();
    words.extend([
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
    ]);
}

fn control(words: &mut Vec<u16>, id: u16, class: u16, style: u32, rect: [u16; 4], text: &str) {
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    push_dword(words, WS_CHILD | WS_VISIBLE | style);
    push_dword(words, 0);
    words.extend(rect);
    words.extend([id, 0xffff, class]);
    words.extend(terminated(text));
    words.push(0); // No creation data.
}

fn template() -> Vec<u32> {
    let mut words = Vec::new();
    push_dword(
        &mut words,
        WS_CAPTION | WS_SYSMENU | (DS_MODALFRAME | DS_SETFONT | DS_CENTER).cast_unsigned(),
    );
    push_dword(&mut words, 0);
    words.extend([6, 0, 0, 280, 164, 0, 0]); // Count, fixed bounds, no menu/default dialog class.
    words.extend(terminated("Swift Bonsai 2"));
    words.push(9);
    words.extend(terminated("Segoe UI"));
    control(
        &mut words,
        1003,
        0x82,
        0,
        [12, 10, 256, 14],
        "Swift Bonsai could not start",
    );
    control(
        &mut words,
        1004,
        0x82,
        0,
        [12, 29, 256, 12],
        "Error details (preview)",
    );
    control(
        &mut words,
        PREVIEW_ID,
        0x81,
        WS_BORDER
            | WS_TABSTOP
            | WS_VSCROLL
            | (ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY).cast_unsigned(),
        [12, 44, 256, 66],
        "",
    );
    control(
        &mut words,
        FEEDBACK_ID,
        0x82,
        0,
        [12, 115, 256, 18],
        "Copy error keeps the complete message. Review paths before sharing.",
    );
    control(
        &mut words,
        COPY_ID,
        0x80,
        WS_TABSTOP,
        [116, 140, 82, 16],
        "&Copy error",
    );
    control(
        &mut words,
        1,
        0x80,
        WS_TABSTOP | BS_DEFPUSHBUTTON.cast_unsigned(),
        [206, 140, 62, 16],
        "Close",
    );
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    // Vec<u32> guarantees the DWORD alignment required by dialog templates.
    words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_error_preview_is_bounded_without_losing_full_copy_text() {
        let message = format!("{}\0tail", "錯誤🌱\n".repeat(1000));
        let shortened = preview(&message, PREVIEW_CHARS);
        assert!(shortened.chars().count() < PREVIEW_CHARS + 100);
        assert!(
            shortened.ends_with("[Preview shortened. Copy error includes the complete message.]")
        );
        let full = terminated(&message);
        assert_eq!(full.last(), Some(&0));
        assert_eq!(full.iter().filter(|unit| **unit == 0).count(), 1);
        assert!(String::from_utf16_lossy(&full).ends_with("\\0tail\0"));
        assert_eq!(preview("Short error", PREVIEW_CHARS), "Short error");
    }
}
