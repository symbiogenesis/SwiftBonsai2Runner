//! Read-only verification of Cargo's extracted registry tree against approved bytes.

use super::{BTreeSet, Path, Result, digest, fs, json, member, reject_redirects, require};
use std::io::Read as _;

struct Bounded<R> {
    inner: R,
    remaining: u64,
}
impl<R: std::io::Read> std::io::Read for Bounded<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut extra = [0];
            return if self.inner.read(&mut extra)? == 0 {
                Ok(0)
            } else {
                Err(std::io::Error::other(
                    "Registry archive expanded beyond its stream bound.",
                ))
            };
        }
        let length = output
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let count = self.inner.read(&mut output[..length])?;
        self.remaining -= u64::try_from(count).map_err(std::io::Error::other)?;
        Ok(count)
    }
}

pub(super) fn verify(root: &Path, expected_digest: &str) -> Result<()> {
    let package = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Missing registry package name.")?;
    let index = root
        .parent()
        .and_then(Path::file_name)
        .ok_or("Missing registry index.")?;
    let registry = root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Missing registry cache.")?;
    let archive = registry
        .join("cache")
        .join(index)
        .join(format!("{package}.crate"));
    require(
        crate::identity::is_lower_hex(expected_digest, 64) && digest(&archive)? == expected_digest,
        "Registry archive differs from approved publication.",
    )?;
    let file = bonsai_artifacts::open_locked_file(&archive).map_err(|e| e.to_string())?;
    let gzip = flate2::read::GzDecoder::new(file);
    // Header, extension and directory bytes share a separate conservative stream
    // bound, so declared file extents cannot hide an unbounded metadata stream.
    let mut tar = tar::Archive::new(Bounded {
        inner: gzip,
        remaining: 536_870_912,
    });
    let mut seen = BTreeSet::new();
    let mut count = 0u64;
    let mut total = 0u64;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        count += 1;
        require(count <= 20_000, "Excessive registry archive entries.")?;
        let name = std::str::from_utf8(&entry.path_bytes())
            .map_err(|e| e.to_string())?
            .to_owned();
        let (prefix, relative) = name
            .split_once('/')
            .ok_or("Invalid registry archive source path.")?;
        require(prefix == package, "Registry archive source prefix differs.")?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if !relative.is_empty() {
                member(root, &json!(relative.trim_end_matches('/')))?;
            }
            continue;
        }
        require(
            kind.is_file(),
            "Registry archive contains nonregular member.",
        )?;
        let path = member(root, &json!(relative))?;
        require(
            seen.insert(relative.to_owned()),
            "Duplicate registry archive member.",
        )?;
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or("Registry archive extent overflow.")?;
        require(
            size <= 67_108_864 && total <= 268_435_456,
            "Registry archive exceeds preparation bounds.",
        )?;
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        require(
            metadata.is_file() && metadata.len() == size,
            "Cargo-extracted source extent differs.",
        )?;
        let mut hash = crate::identity::Hasher::new()?;
        let mut buffer = vec![0; 65_536].into_boxed_slice();
        loop {
            let count = entry.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count])?;
        }
        require(
            digest(&path)? == hash.finalize_hex()?,
            "Cargo-extracted source differs from approved archive.",
        )?;
    }
    require(!seen.is_empty(), "Empty registry source archive.")?;
    require(
        seen == inventory(root)?,
        "Cargo-extracted registry tree has missing or unexpected source files.",
    )
}

fn inventory(root: &Path) -> Result<BTreeSet<String>> {
    let mut actual = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    let mut entries = 0u64;
    while let Some(directory) = pending.pop() {
        for item in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            entries += 1;
            require(entries <= 40_000, "Excessive extracted registry tree.")?;
            let path = item.path();
            reject_redirects(&path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if metadata.is_dir() {
                pending.push(path);
            } else {
                require(metadata.is_file(), "Nonregular extracted registry member.")?;
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-Unicode registry path.")?
                    .replace('\\', "/");
                if relative != ".cargo-ok" {
                    actual.insert(relative);
                }
            }
        }
    }
    Ok(actual)
}
