//! Reject external speech/MSVC runtime imports when creating new distributions.
//!
//! This reads PE metadata only; it never loads the candidate image or a DLL.

use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAX_DIRECTORIES: usize = 16;
const MAX_SECTIONS: usize = 96;
const MAX_IMPORTS: usize = 4096;
const MAX_IMPORT_BYTES: usize = 1024 * 1024;
const MAX_DLL_NAME: usize = 256;

pub(super) fn verify(path: &Path) -> crate::Result<()> {
    let file = super::open(path)?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    Image::parse(file, length)?.verify_imports()
}

struct Section {
    rva: u32,
    mapped_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

struct Image<R> {
    source: R,
    length: u64,
    headers: u32,
    image_base: u64,
    directories: [(u32, u32); MAX_DIRECTORIES],
    sections: Vec<Section>,
}

fn invalid() -> String {
    "Invalid or unsupported portable PE import metadata.".into()
}

fn u16_at(bytes: &[u8], offset: usize) -> crate::Result<u16> {
    let value = bytes.get(offset..offset + 2).ok_or_else(invalid)?;
    Ok(u16::from_le_bytes(value.try_into().map_err(|_| invalid())?))
}

fn u32_at(bytes: &[u8], offset: usize) -> crate::Result<u32> {
    let value = bytes.get(offset..offset + 4).ok_or_else(invalid)?;
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| invalid())?))
}

impl<R: Read + Seek> Image<R> {
    fn read(&mut self, offset: u64, output: &mut [u8]) -> crate::Result<()> {
        let size = u64::try_from(output.len()).map_err(|_| invalid())?;
        if offset.checked_add(size).is_none_or(|end| end > self.length) {
            return Err(invalid());
        }
        self.source
            .seek(SeekFrom::Start(offset))
            .map_err(|_| invalid())?;
        self.source.read_exact(output).map_err(|_| invalid())
    }

    fn parse(source: R, length: u64) -> crate::Result<Self> {
        let mut image = Self {
            source,
            length,
            headers: 0,
            image_base: 0,
            directories: [(0, 0); MAX_DIRECTORIES],
            sections: Vec::new(),
        };
        let mut dos = [0; 64];
        image.read(0, &mut dos)?;
        let pe_offset = u64::from(u32_at(&dos, 60)?);
        if &dos[..2] != b"MZ" || pe_offset < 64 {
            return Err(invalid());
        }
        let mut pe = [0; 24];
        image.read(pe_offset, &mut pe)?;
        if &pe[..4] != b"PE\0\0" || u16_at(&pe, 4)? != 0xaa64 {
            return Err(invalid());
        }
        let section_count = usize::from(u16_at(&pe, 6)?);
        let optional_size = usize::from(u16_at(&pe, 20)?);
        if section_count > MAX_SECTIONS || !(112..=4096).contains(&optional_size) {
            return Err(invalid());
        }
        let mut optional = vec![0; optional_size];
        image.read(pe_offset + 24, &mut optional)?;
        if u16_at(&optional, 0)? != 0x020b {
            return Err(invalid());
        }
        image.headers = u32_at(&optional, 60)?;
        image.image_base = u64::from_le_bytes(optional[24..32].try_into().map_err(|_| invalid())?);
        let directory_count = usize::try_from(u32_at(&optional, 108)?).map_err(|_| invalid())?;
        if directory_count > MAX_DIRECTORIES || 112 + directory_count * 8 > optional_size {
            return Err(invalid());
        }
        for (index, directory) in image
            .directories
            .iter_mut()
            .take(directory_count)
            .enumerate()
        {
            *directory = (
                u32_at(&optional, 112 + index * 8)?,
                u32_at(&optional, 116 + index * 8)?,
            );
        }
        let sections_offset =
            pe_offset + 24 + u64::try_from(optional_size).map_err(|_| invalid())?;
        let sections_end =
            sections_offset + u64::try_from(section_count * 40).map_err(|_| invalid())?;
        if u64::from(image.headers) < sections_end || u64::from(image.headers) > length {
            return Err(invalid());
        }
        for index in 0..section_count {
            let mut section = [0; 40];
            image.read(
                sections_offset + u64::try_from(index * 40).map_err(|_| invalid())?,
                &mut section,
            )?;
            let raw_size = u32_at(&section, 16)?;
            let entry = Section {
                rva: u32_at(&section, 12)?,
                mapped_size: u32_at(&section, 8)?.max(raw_size),
                raw_offset: u32_at(&section, 20)?,
                raw_size,
            };
            if u64::from(entry.raw_offset) + u64::from(raw_size) > length
                || entry.rva.checked_add(entry.mapped_size).is_none()
                || (entry.mapped_size > 0 && entry.rva < image.headers)
                || image.sections.iter().any(|other| overlap(&entry, other))
            {
                return Err(invalid());
            }
            image.sections.push(entry);
        }
        Ok(image)
    }

    // Only initialized file-backed bytes can contain import tables or names.
    fn location(&self, rva: u32, size: usize) -> crate::Result<(u64, usize)> {
        let size = u64::try_from(size).map_err(|_| invalid())?;
        if rva < self.headers {
            let available = self.headers - rva;
            if size <= u64::from(available) {
                return Ok((
                    u64::from(rva),
                    usize::try_from(available).map_err(|_| invalid())?,
                ));
            }
            return Err(invalid());
        }
        for section in &self.sections {
            let Some(delta) = rva.checked_sub(section.rva) else {
                continue;
            };
            if delta < section.mapped_size {
                let available = section.raw_size.checked_sub(delta).ok_or_else(invalid)?;
                if size > u64::from(available) {
                    return Err(invalid());
                }
                return Ok((
                    u64::from(section.raw_offset) + u64::from(delta),
                    usize::try_from(available).map_err(|_| invalid())?,
                ));
            }
        }
        Err(invalid())
    }

    fn dll_name(&mut self, rva: u32) -> crate::Result<()> {
        if rva == 0 {
            return Err(invalid());
        }
        let (offset, available) = self.location(rva, 1)?;
        let mut bytes = [0; MAX_DLL_NAME];
        let count = available.min(bytes.len());
        self.read(offset, &mut bytes[..count])?;
        let end = bytes[..count]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?;
        let name = &bytes[..end];
        if name.is_empty()
            || name
                .iter()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b"._-".contains(byte))
        {
            return Err(invalid());
        }
        let name = String::from_utf8(name.to_ascii_lowercase()).map_err(|_| invalid())?;
        if forbidden(&name) {
            return Err(format!(
                "Portable executable imports {name}. Rebuild all application components with the static runtime and Rust TTS before packaging."
            ));
        }
        Ok(())
    }

    fn import_table(&mut self, directory: (u32, u32), delay: bool) -> crate::Result<()> {
        let (rva, size) = directory;
        if rva == 0 && size == 0 {
            return Ok(());
        }
        let size = usize::try_from(size).map_err(|_| invalid())?;
        let width = if delay { 32 } else { 20 };
        if rva == 0 || size < width || size > MAX_IMPORT_BYTES {
            return Err(invalid());
        }
        let (offset, _) = self.location(rva, size)?;
        for index in 0..(size / width).min(MAX_IMPORTS) {
            let mut descriptor = [0; 32];
            self.read(
                offset + u64::try_from(index * width).map_err(|_| invalid())?,
                &mut descriptor[..width],
            )?;
            if descriptor[..width].iter().all(|byte| *byte == 0) {
                return Ok(());
            }
            let name = if delay {
                let attributes = u32_at(&descriptor, 0)?;
                if attributes & !1 != 0 {
                    return Err(invalid());
                }
                let name = u32_at(&descriptor, 4)?;
                if attributes == 1 {
                    name
                } else {
                    u32::try_from(
                        u64::from(name)
                            .checked_sub(self.image_base)
                            .ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?
                }
            } else {
                u32_at(&descriptor, 12)?
            };
            self.dll_name(name)?;
        }
        Err(invalid())
    }

    fn verify_imports(&mut self) -> crate::Result<()> {
        self.import_table(self.directories[1], false)?;
        self.import_table(self.directories[13], true)
    }
}

fn overlap(a: &Section, b: &Section) -> bool {
    let ranges = |start: u32, size: u32, other_start: u32, other_size: u32| {
        size > 0
            && other_size > 0
            && u64::from(start) < u64::from(other_start) + u64::from(other_size)
            && u64::from(other_start) < u64::from(start) + u64::from(size)
    };
    ranges(a.rva, a.mapped_size, b.rva, b.mapped_size)
        || ranges(a.raw_offset, a.raw_size, b.raw_offset, b.raw_size)
}

fn forbidden(name: &str) -> bool {
    [
        "vcruntime",
        "msvcp",
        "concrt",
        "vcomp",
        "ggml",
        "qwen",
        "onnxruntime",
        "ucrtbased",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || (name.starts_with("msvcr") && name != "msvcrt.dll")
}

#[cfg(test)]
mod tests {
    use super::Image;
    use std::io::Cursor;

    fn image(normal: Option<&str>, delayed: Option<&str>) -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
        bytes[148..150].copy_from_slice(&240_u16.to_le_bytes());
        bytes[152..154].copy_from_slice(&0x20b_u16.to_le_bytes());
        bytes[212..216].copy_from_slice(&512_u32.to_le_bytes());
        bytes[260..264].copy_from_slice(&16_u32.to_le_bytes());
        bytes[400..404].copy_from_slice(&512_u32.to_le_bytes());
        bytes[404..408].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[408..412].copy_from_slice(&512_u32.to_le_bytes());
        bytes[412..416].copy_from_slice(&512_u32.to_le_bytes());
        if let Some(name) = normal {
            bytes[272..276].copy_from_slice(&0x1000_u32.to_le_bytes());
            bytes[276..280].copy_from_slice(&40_u32.to_le_bytes());
            bytes[524..528].copy_from_slice(&0x1080_u32.to_le_bytes());
            bytes[640..640 + name.len()].copy_from_slice(name.as_bytes());
        }
        if let Some(name) = delayed {
            bytes[368..372].copy_from_slice(&0x1100_u32.to_le_bytes());
            bytes[372..376].copy_from_slice(&64_u32.to_le_bytes());
            bytes[768..772].copy_from_slice(&1_u32.to_le_bytes());
            bytes[772..776].copy_from_slice(&0x1180_u32.to_le_bytes());
            bytes[896..896 + name.len()].copy_from_slice(name.as_bytes());
        }
        bytes
    }

    fn check(bytes: Vec<u8>) -> crate::Result<()> {
        let length = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;
        Image::parse(Cursor::new(bytes), length)?.verify_imports()
    }

    #[test]
    fn system_crt_imports_pass_and_external_runtimes_fail_in_both_tables() -> crate::Result<()> {
        check(image(None, None))?;
        for name in [
            "KERNEL32.dll",
            "ucrtbase.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll",
            "msvcrt.dll",
        ] {
            check(image(Some(name), Some(name)))?;
        }
        for name in [
            "VCRUNTIME140.dll",
            "msvcp140.dll",
            "concrt140.dll",
            "vcomp140.dll",
            "ggml-base.dll",
            "qwen-core.dll",
            "onnxruntime.dll",
            "ucrtbased.dll",
            "msvcr120.dll",
        ] {
            assert!(check(image(Some(name), None)).is_err());
            assert!(check(image(Some("KERNEL32.dll"), Some(name))).is_err());
        }
        Ok(())
    }

    #[test]
    fn delayed_virtual_addresses_are_checked_without_truncating_the_image_base() -> crate::Result<()>
    {
        let mut bytes = image(None, Some("vcruntime140.dll"));
        bytes[176..184].copy_from_slice(&0x1000_0000_u64.to_le_bytes());
        bytes[768..772].fill(0);
        bytes[772..776].copy_from_slice(&0x1000_1180_u32.to_le_bytes());
        assert!(check(bytes.clone()).is_err());
        bytes[896..912].fill(0);
        bytes[896..908].copy_from_slice(b"KERNEL32.dll");
        check(bytes.clone())?;
        bytes[176..184].copy_from_slice(&0x1_1000_0000_u64.to_le_bytes());
        assert!(check(bytes).is_err());
        Ok(())
    }

    #[test]
    fn malformed_tables_names_and_optional_headers_fail_closed() {
        let mut bytes = image(Some("KERNEL32.dll"), None);
        bytes[276..280].copy_from_slice(&20_u32.to_le_bytes());
        assert!(check(bytes).is_err()); // No terminating descriptor in the directory.
        let mut bytes = image(Some("KERNEL32.dll"), None);
        bytes[524..528].copy_from_slice(&0x2000_u32.to_le_bytes());
        assert!(check(bytes).is_err()); // No initialized bytes at the DLL-name RVA.
        let mut bytes = image(Some("KERNEL32.dll"), None);
        bytes[272..276].copy_from_slice(&0x11f8_u32.to_le_bytes());
        assert!(check(bytes).is_err()); // Table straddles the section's raw end.
        let mut bytes = image(None, Some("KERNEL32.dll"));
        bytes[768..772].copy_from_slice(&2_u32.to_le_bytes());
        assert!(check(bytes).is_err()); // Unknown delay-descriptor attributes.
        let mut bytes = image(Some("KERNEL32.dll"), None);
        bytes[640..896].fill(b'a');
        assert!(check(bytes).is_err()); // Bounded names must terminate.
        let mut bytes = image(None, None);
        bytes[152..154].copy_from_slice(&0x10b_u16.to_le_bytes());
        assert!(check(bytes).is_err());
        assert!(check(vec![0; 63]).is_err());
    }
}
