//! Bounded Authenticode signer and live-module observations for device supervisors.

mod catalog;

use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CERT_SHA1_HASH_PROP_ID, CertGetCertificateContextProperty,
    CertGetNameStringW, szOID_COMMON_NAME,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
    WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
    WinVerifyTrust,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};

pub(crate) fn verify(file: &File, path: &Path) -> io::Result<crate::AuthenticodeSigner> {
    if !path.is_absolute() || path.as_os_str().encode_wide().any(|unit| unit == 0) {
        return Err(super::invalid(
            "Signature verification requires an absolute path.",
        ));
    }
    // Installed catalog membership takes precedence, matching Windows' signature
    // observation boundary for driver files that have no embedded signature.
    catalog::verify(file, path).or_else(|_| embedded(file, path))
}

fn embedded(file: &File, path: &Path) -> io::Result<crate::AuthenticodeSigner> {
    let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || wide.contains(&0) {
        return Err(super::invalid(
            "Signature verification requires an absolute path.",
        ));
    }
    wide.push(0);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: u32::try_from(size_of::<WINTRUST_FILE_INFO>())
            .map_err(|_| super::invalid("Trust structure overflow."))?,
        pcwszFilePath: wide.as_ptr(),
        hFile: file.as_raw_handle(),
        pgKnownSubject: ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: u32::try_from(size_of::<WINTRUST_DATA>())
            .map_err(|_| super::invalid("Trust structure overflow."))?,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: data, info, UTF-16 path and the borrowed file remain live throughout
    // verification and state closure. UI and network retrieval are disabled.
    let status =
        unsafe { WinVerifyTrust(ptr::null_mut(), &raw mut action, (&raw mut data).cast()) };
    let result = if status == 0 {
        signer(&data)
    } else {
        Err(super::invalid("Authenticode trust verification failed."))
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: The matching provider state and all input allocations are still live.
    // Closure is mandatory even after a rejected verification.
    let closed =
        unsafe { WinVerifyTrust(ptr::null_mut(), &raw mut action, (&raw mut data).cast()) };
    if closed != 0 {
        return Err(super::invalid("Authenticode provider state did not close."));
    }
    result
}

fn signer(data: &WINTRUST_DATA) -> io::Result<crate::AuthenticodeSigner> {
    // SAFETY: data owns a successful, unclosed WinVerifyTrust provider state.
    let provider = unsafe { WTHelperProvDataFromStateData(data.hWVTStateData) };
    if provider.is_null() {
        return Err(super::invalid("Trust provider omitted signer state."));
    }
    // SAFETY: The live provider owns its primary signer chain; no counter-signer is selected.
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return Err(super::invalid("Trust provider omitted the primary signer."));
    }
    // SAFETY: The live primary signer owns its leaf certificate at chain index zero.
    let certificate = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return Err(super::invalid(
            "Trust provider omitted the signer certificate.",
        ));
    }
    // SAFETY: The returned certificate record remains owned by the unclosed provider.
    let context = unsafe { (*certificate).pCert };
    if context.is_null() {
        return Err(super::invalid("Signer certificate context is missing."));
    }
    let mut common_name = [0_u16; 1024];
    // SAFETY: context is live, the OID is a static terminated ANSI string and the
    // UTF-16 destination is writable for its explicit bounded element count.
    let length = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_ATTR_TYPE,
            0,
            szOID_COMMON_NAME.cast(),
            common_name.as_mut_ptr(),
            1024,
        )
    };
    if !(2..=1024).contains(&length) {
        return Err(super::invalid("Signer common name exceeds its bound."));
    }
    let end =
        usize::try_from(length - 1).map_err(|_| super::invalid("Signer name length overflow."))?;
    if common_name[end] != 0 {
        return Err(super::invalid("Signer name was not terminated."));
    }
    let common_name = String::from_utf16(&common_name[..end])
        .map_err(|_| super::invalid("Signer name is not valid UTF-16."))?;
    let mut digest = [0_u8; 20];
    let mut size = 20_u32;
    // SAFETY: context remains live; digest and size are writable for SHA-1's
    // fixed certificate-thumbprint property. This does not select a file hash.
    super::check(unsafe {
        CertGetCertificateContextProperty(
            context,
            CERT_SHA1_HASH_PROP_ID,
            digest.as_mut_ptr().cast(),
            &raw mut size,
        )
    })?;
    if size != 20 {
        return Err(super::invalid("Signer thumbprint length changed."));
    }
    let thumbprint = digest
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|part| char::from(b"0123456789abcdef"[usize::from(part)]))
        .collect();
    Ok(crate::AuthenticodeSigner {
        common_name,
        thumbprint,
    })
}

pub(crate) fn modules(id: u32, basename: &str) -> io::Result<Vec<PathBuf>> {
    if id == 0
        || basename.is_empty()
        || basename.len() > 260
        || basename.contains(['/', '\\', '\0'])
    {
        return Err(super::invalid("Invalid process module selection."));
    }
    // SAFETY: This creates a read-only snapshot for a numeric process identifier.
    let snapshot = super::owned(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, id)
    })?;
    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(size_of::<MODULEENTRY32W>())
            .map_err(|_| super::invalid("Module entry overflow."))?,
        ..MODULEENTRY32W::default()
    };
    let mut paths = Vec::new();
    // SAFETY: The owned snapshot and correctly sized writable record remain live.
    let mut found = unsafe { Module32FirstW(snapshot.as_raw_handle(), &raw mut entry) };
    let mut count = 0;
    while found != 0 {
        count += 1;
        if count > 2048 {
            return Err(super::invalid(
                "Process module inventory exceeds its bound.",
            ));
        }
        let name_end = entry
            .szModule
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| super::invalid("Unterminated module name."))?;
        if String::from_utf16_lossy(&entry.szModule[..name_end]).eq_ignore_ascii_case(basename) {
            let path_end = entry
                .szExePath
                .iter()
                .position(|unit| *unit == 0)
                .ok_or_else(|| super::invalid("Unterminated module path."))?;
            paths.push(std::ffi::OsString::from_wide(&entry.szExePath[..path_end]).into());
        }
        // SAFETY: The same snapshot and initialized writable record remain live.
        found = unsafe { Module32NextW(snapshot.as_raw_handle(), &raw mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(18) {
        return Err(error);
    }
    Ok(paths)
}
