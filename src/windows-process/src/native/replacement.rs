//! Atomic publication uses the verified open file rather than its former name.

use std::{
    fs::File,
    io,
    mem::offset_of,
    os::windows::{ffi::OsStrExt as _, io::AsRawHandle as _},
    path::Path,
    ptr,
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
};

pub(crate) fn replace(file: &File, target: &Path) -> io::Result<()> {
    let mut name: Vec<_> = target.as_os_str().encode_wide().collect();
    if !target.is_absolute() || name.is_empty() || name.len() > 32_767 || name.contains(&0) {
        return Err(super::invalid(
            "File replacement requires an absolute destination without NUL characters.",
        ));
    }
    let name_bytes = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| super::invalid("Replacement name length overflow."))?;
    // FILE_RENAME_INFO documents FileName as NUL-terminated even though its
    // separate byte count excludes the terminator. Supply both explicitly:
    // relying on trailing allocation padding can publish a different filename.
    name.push(0);
    let encoded_bytes = name_bytes + size_of::<u16>();
    let name_offset = offset_of!(FILE_RENAME_INFO, FileName);
    let required = name_offset
        .checked_add(encoded_bytes)
        .ok_or_else(|| super::invalid("Replacement buffer size overflow."))?
        .max(size_of::<FILE_RENAME_INFO>());
    // Typed allocation supplies the SDK structure's alignment on every Windows
    // architecture; its trailing filename may extend past the first element.
    let mut information =
        vec![FILE_RENAME_INFO::default(); required.div_ceil(size_of::<FILE_RENAME_INFO>())];
    information[0].Anonymous.ReplaceIfExists = true;
    information[0].RootDirectory = ptr::null_mut();
    information[0].FileNameLength = u32::try_from(name_bytes)
        .map_err(|_| super::invalid("Replacement name exceeds Windows bounds."))?;
    let destination = information
        .as_mut_ptr()
        .cast::<u8>()
        .wrapping_add(name_offset);
    // SAFETY: information owns at least name_offset + encoded_bytes initialized
    // bytes, with the SDK's alignment for FileName. Copying the native UTF-16
    // representation as bytes requires no pointer alignment increase. The
    // separate source allocation cannot overlap this destination.
    unsafe { ptr::copy_nonoverlapping(name.as_ptr().cast::<u8>(), destination, encoded_bytes) };
    let required = u32::try_from(required)
        .map_err(|_| super::invalid("Replacement buffer exceeds Windows bounds."))?;
    // SAFETY: The borrowed File and aligned information buffer remain alive
    // throughout this synchronous call. FileNameLength counts exactly the
    // copied UTF-16 path bytes, excluding the explicit terminator; RootDirectory is null for the
    // absolute path. Windows checks DELETE access and all sharing constraints.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            information.as_ptr().cast(),
            required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
