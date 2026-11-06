//! Audited runtime registration of the signed Windows ARM64 Hexagon NPU backend.

use std::{
    env,
    ffi::{CStr, OsStr, c_char, c_int, c_void},
    fs,
    os::windows::{ffi::OsStrExt as _, fs::MetadataExt as _},
    path::{Path, PathBuf},
    ptr::{self, NonNull},
    sync::OnceLock,
};

const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn SetEnvironmentVariableW(name: *const u16, value: *const u16) -> c_int;
}

unsafe extern "C" {
    fn _wputenv_s(name: *const u16, value: *const u16) -> c_int;
    fn ggml_backend_register(reg: *mut c_void);
    fn ggml_backend_dev_by_name(name: *const c_char) -> *mut c_void;
}

type LogCallback = extern "C" fn(c_int, *const c_char, *mut c_void);
type LogSet = unsafe extern "C" fn(Option<LogCallback>, *mut c_void);
type Registry = unsafe extern "C" fn() -> *mut c_void;
type DeviceCount = unsafe extern "C" fn(*mut c_void) -> usize;

struct PinnedRuntime {
    _base: NonNull<c_void>,
    _hexagon: NonNull<c_void>,
}

// SAFETY: Module handles remain pinned for the lifetime of the process and are
// never mutated or unloaded after one-time initialization.
unsafe impl Send for PinnedRuntime {}
// SAFETY: Shared references never mutate or close the pinned module handles.
unsafe impl Sync for PinnedRuntime {}

static HEXAGON_READY: OnceLock<Option<PinnedRuntime>> = OnceLock::new();

pub(super) fn select_device(discard_log: LogCallback) -> &'static CStr {
    if let Ok(requested) = env::var("BONSAIRUNNER_TTS_DEVICE") {
        let trimmed = requested.trim();
        if trimmed.eq_ignore_ascii_case("CPU") {
            return c"CPU";
        }
    }
    if ensure_registered(discard_log) {
        c"HTP0"
    } else {
        c"CPU"
    }
}

fn ensure_registered(discard_log: LogCallback) -> bool {
    HEXAGON_READY
        .get_or_init(|| register_hexagon(discard_log))
        .is_some()
}

fn register_hexagon(discard_log: LogCallback) -> Option<PinnedRuntime> {
    let runtime_dir = runtime_directory()?;
    if !valid_directory(&runtime_dir) {
        return None;
    }
    for required in ["ggml-base.dll", "ggml-hexagon.dll", "libggml-htp-v81.so"] {
        if !regular_file(&runtime_dir.join(required)) {
            return None;
        }
    }
    if env::var_os("ADSP_LIBRARY_PATH").is_none() && !set_env_wide("ADSP_LIBRARY_PATH", &runtime_dir)
    {
        return None;
    }
    let base = load_library(&runtime_dir.join("ggml-base.dll"))?;
    let hexagon = load_library(&runtime_dir.join("ggml-hexagon.dll"))?;
    let log_set: LogSet = unsafe {
        // SAFETY: ggml_log_set in the pinned ggml-base.dll matches theLogSet C ABI.
        std::mem::transmute::<*mut c_void, LogSet>(symbol(base, b"ggml_log_set\0")?)
    };
    let dev_count: DeviceCount = unsafe {
        // SAFETY: ggml_backend_reg_dev_count matches the DeviceCount C ABI.
        std::mem::transmute::<*mut c_void, DeviceCount>(symbol(
            base,
            b"ggml_backend_reg_dev_count\0",
        )?)
    };
    let hexagon_reg: Registry = unsafe {
        // SAFETY: ggml_backend_hexagon_reg in ggml-hexagon.dll matches Registry C ABI.
        std::mem::transmute::<*mut c_void, Registry>(symbol(
            hexagon,
            b"ggml_backend_hexagon_reg\0",
        )?)
    };
    // SAFETY: Install the non-unwinding discard logger into the DLL before
    // querying or initializing the Hexagon registry.
    unsafe { log_set(Some(discard_log), ptr::null_mut()) };
    // SAFETY: Invokes the Hexagon registry constructor once under OnceLock.
    let reg = unsafe { hexagon_reg() };
    if reg.is_null() {
        return None;
    }
    // SAFETY: reg is the non-null registry handle returned by ggml_backend_hexagon_reg.
    let count = unsafe { dev_count(reg) };
    if count == 0 {
        return None;
    }
    // SAFETY: Registers the ABI-compatible Hexagon backend registry with the
    // statically linked ggml-base registry.
    unsafe { ggml_backend_register(reg) };
    // SAFETY: Queries the statically linked ggml-base registry by static C string.
    if unsafe { ggml_backend_dev_by_name(c"HTP0".as_ptr()) }.is_null() {
        return None;
    }
    Some(PinnedRuntime {
        _base: base,
        _hexagon: hexagon,
    })
}

fn runtime_directory() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("BONSAI_HEXAGON_RUNTIME_DIR") {
        let path = PathBuf::from(explicit);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    let local_app_data = env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local_app_data)
            .join("SwiftBonsai2")
            .join("hexagon")
            .join("runtime"),
    )
}

fn valid_directory(path: &Path) -> bool {
    let spelling = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if spelling.len() < 3
        || spelling.len() > 260
        || spelling[1] != u16::from(b':')
        || !matches!(spelling[2], 47 | 92)
        || !path.is_absolute()
        || spelling.contains(&0)
    {
        return false;
    }
    path.ancestors().all(|ancestor| {
        fs::symlink_metadata(ancestor).is_ok_and(|metadata| {
            metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        })
    })
}

fn regular_file(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    })
}

fn encode_wide(text: &OsStr) -> Option<Vec<u16>> {
    let wide = text.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    if wide[..wide.len().saturating_sub(1)].contains(&0) {
        return None;
    }
    Some(wide)
}

fn set_env_wide(name: &str, value: &Path) -> bool {
    let Some(name_wide) = encode_wide(OsStr::new(name)) else {
        return false;
    };
    let Some(value_wide) = encode_wide(value.as_os_str()) else {
        return false;
    };
    // SAFETY: Both buffers are valid NUL-terminated UTF-16 strings that outlive
    // the call. _wputenv_s updates the UCRT narrow/wide environment tables
    // read by libcdsprpc.dll's getenv("ADSP_LIBRARY_PATH").
    let ucrt_ok = unsafe { _wputenv_s(name_wide.as_ptr(), value_wide.as_ptr()) == 0 };
    // SAFETY: Both buffers are valid NUL-terminated UTF-16 strings that outlive
    // the call. SetEnvironmentVariableW updates the Win32 process environment block.
    let win32_ok = unsafe { SetEnvironmentVariableW(name_wide.as_ptr(), value_wide.as_ptr()) != 0 };
    ucrt_ok && win32_ok
}

fn load_library(path: &Path) -> Option<NonNull<c_void>> {
    let wide = encode_wide(path.as_os_str())?;
    // SAFETY: The path is an existing absolute non-reparse file; DLL search is
    // restricted to its own directory and Windows System32.
    let raw = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    NonNull::new(raw)
}

fn symbol(module: NonNull<c_void>, name: &'static [u8]) -> Option<*mut c_void> {
    if name.last() != Some(&0) {
        return None;
    }
    // SAFETY: module is a live loaded library and name is NUL-terminated ASCII.
    let raw = unsafe { GetProcAddress(module.as_ptr(), name.as_ptr()) };
    (!raw.is_null()).then_some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_device_registers_htp0_when_runtime_present() {
        let device = select_device(super::super::discard_log);
        if runtime_directory()
            .as_deref()
            .is_some_and(|dir| regular_file(&dir.join("ggml-hexagon.dll")))
        {
            assert_eq!(device, c"HTP0");
        } else {
            assert_eq!(device, c"CPU");
        }
    }
}
