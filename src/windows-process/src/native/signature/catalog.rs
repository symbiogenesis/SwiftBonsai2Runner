//! Installed catalog member verification using the retained file handle.
//!
//! Reference: <https://learn.microsoft.com/windows/win32/api/mscat/nf-mscat-cryptcatadminenumcatalogfromhash>

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Security::Cryptography::Catalog::{
    CATALOG_INFO, CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
};
use windows_sys::Win32::Security::WinTrust::{
    DRIVER_ACTION_VERIFY, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO, WINTRUST_DATA,
    WINTRUST_DATA_0, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_CATALOG,
    WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
};

struct Admin(isize);
impl Drop for Admin {
    fn drop(&mut self) {
        // SAFETY: The acquired catalog administration context is owned exactly once.
        let _ = unsafe { CryptCATAdminReleaseContext(self.0, 0) };
    }
}
struct Catalog<'a> {
    admin: &'a Admin,
    context: isize,
}
impl Drop for Catalog<'_> {
    fn drop(&mut self) {
        // SAFETY: This catalog context was obtained from the still-live matching admin.
        let _ = unsafe { CryptCATAdminReleaseCatalogContext(self.admin.0, self.context, 0) };
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep catalog verification and provider state closure in one auditable lifetime scope."
)]
pub(super) fn verify(file: &File, path: &Path) -> io::Result<crate::AuthenticodeSigner> {
    let mut admin = 0;
    let subsystem = DRIVER_ACTION_VERIFY;
    let algorithm = windows_sys::core::w!("SHA256");
    // SAFETY: The output is writable; static algorithm and subsystem inputs remain live.
    super::super::check(unsafe {
        CryptCATAdminAcquireContext2(
            &raw mut admin,
            &raw const subsystem,
            algorithm,
            ptr::null(),
            0,
        )
    })?;
    let admin = Admin(admin);
    let mut hash = [0_u8; 64];
    let mut hash_bytes = 64_u32;
    // SAFETY: The retained file and catalog context are live; the destination has
    // exactly the declared writable capacity and the API is synchronous.
    super::super::check(unsafe {
        CryptCATAdminCalcHashFromFileHandle2(
            admin.0,
            file.as_raw_handle(),
            &raw mut hash_bytes,
            hash.as_mut_ptr(),
            0,
        )
    })?;
    if hash_bytes != 32 {
        return Err(super::super::invalid("Catalog SHA-256 length changed."));
    }
    // SAFETY: The live administrator receives the exact SHA-256 bytes and starts
    // a fresh enumeration. The single selected context is released by RAII.
    let context = unsafe {
        CryptCATAdminEnumCatalogFromHash(admin.0, hash.as_ptr(), hash_bytes, 0, ptr::null_mut())
    };
    if context == 0 {
        return Err(super::super::invalid(
            "No installed catalog binds the retained file hash.",
        ));
    }
    let catalog = Catalog {
        admin: &admin,
        context,
    };
    let mut info = CATALOG_INFO {
        cbStruct: u32::try_from(size_of::<CATALOG_INFO>())
            .map_err(|_| super::super::invalid("Catalog structure overflow."))?,
        ..CATALOG_INFO::default()
    };
    // SAFETY: The selected catalog remains owned; info is correctly sized and writable.
    super::super::check(unsafe {
        CryptCATCatalogInfoFromContext(catalog.context, &raw mut info, 0)
    })?;
    if !info.wszCatalogFile.contains(&0) {
        return Err(super::super::invalid("Catalog path was not terminated."));
    }
    let mut member: Vec<u16> = hash[..32]
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|part| u16::from(b"0123456789ABCDEF"[usize::from(part)]))
        .collect();
    member.push(0);
    let filename: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut catalog_info = WINTRUST_CATALOG_INFO {
        cbStruct: u32::try_from(size_of::<WINTRUST_CATALOG_INFO>())
            .map_err(|_| super::super::invalid("Catalog trust structure overflow."))?,
        pcwszCatalogFilePath: info.wszCatalogFile.as_ptr(),
        pcwszMemberTag: member.as_ptr(),
        pcwszMemberFilePath: filename.as_ptr(),
        hMemberFile: file.as_raw_handle(),
        pbCalculatedFileHash: hash.as_mut_ptr(),
        cbCalculatedFileHash: hash_bytes,
        hCatAdmin: admin.0,
        ..WINTRUST_CATALOG_INFO::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: u32::try_from(size_of::<WINTRUST_DATA>())
            .map_err(|_| super::super::invalid("Catalog trust structure overflow."))?,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_CATALOG,
        Anonymous: WINTRUST_DATA_0 {
            pCatalog: &raw mut catalog_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: Every input buffer, administrator, catalog and retained file remains
    // live through synchronous verification and the matching provider-state close.
    let status =
        unsafe { WinVerifyTrust(ptr::null_mut(), &raw mut action, (&raw mut data).cast()) };
    let result = if status == 0 {
        super::signer(&data)
    } else {
        Err(super::super::invalid(
            "Installed catalog trust or membership was rejected.",
        ))
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: The original trust state and all backing buffers are still live.
    let closed =
        unsafe { WinVerifyTrust(ptr::null_mut(), &raw mut action, (&raw mut data).cast()) };
    if closed != 0 {
        return Err(super::super::invalid(
            "Installed catalog trust state did not close.",
        ));
    }
    result
}
