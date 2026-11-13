//! Data-only, bounded archive decoding; no paths are extracted or executed.
#![forbid(unsafe_code)]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};
use unicode_casefold::UnicodeCaseFold as _;

fn limit(pin: &Value, name: &str) -> Result<u64, Box<dyn std::error::Error>> {
    pin["limits"][name]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid archive limit: {name}").into())
}
fn category(name: &str, pin: &Value) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let leaf = lower.rsplit('/').next()?;
    if Some(name) == pin["intendedQaicPath"].as_str() {
        Some("qaic-generator")
    } else if lower.ends_with("/build/cmake/hexagon_fun.cmake") {
        Some("build-idl-recipe")
    } else if lower.contains("/build/cmake/") && lower.ends_with(".cmake") {
        Some("build-recipe-helper")
    } else if lower.ends_with(".idl")
        && (lower.contains("/incs/")
            || lower.contains("/ipc/fastrpc/")
            || ["remote.idl", "aeestddef.idl"].contains(&leaf))
    {
        Some("sdk-idl-input-candidate")
    } else if ["remote.h", "remote_wos_ext.h", "dspqueue.h", "rpcmem.h"].contains(&leaf) {
        Some("sdk-rpc-header-candidate")
    } else if [
        "license",
        "licence",
        "eula",
        "notice",
        "copyright",
        "terms",
        "agreement",
        "copying",
    ]
    .iter()
    .any(|word| leaf.contains(word))
    {
        Some("license-review")
    } else {
        None
    }
}
fn checked_name(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    if name.len() > 1024
        || name.chars().any(|c| c < ' ' || c == '\\' || c == ':')
        || name.starts_with('/')
        || name.split('/').any(|c| c == "..")
    {
        return Err("archive entry escapes its relative namespace".into());
    }
    Ok(name
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect::<Vec<_>>()
        .join("/"))
}
struct Bounded<R> {
    inner: R,
    remaining: u64,
    deadline: Instant,
}
impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "SDK stream deadline expired",
            ));
        }
        if self.remaining == 0 {
            let mut probe = [0_u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            return Err(io::Error::other(
                "SDK decompressed stream exceeds its bound",
            ));
        }
        let cap = bytes
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut bytes[..cap])?;
        self.remaining -= u64::try_from(read).map_err(io::Error::other)?;
        Ok(read)
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        result.push(char::from(DIGITS[usize::from(b >> 4)]));
        result.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    result
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let archive = args.next().ok_or("archive required")?;
    let pin_path = args.next().ok_or("pin required")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut raw_pin = Vec::new();
    File::open(pin_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut raw_pin)?;
    if raw_pin.len() > 1024 * 1024 {
        return Err("pin exceeds bound".into());
    }
    let pin: Value = serde_json::from_slice(&raw_pin)?;
    let deadline = Instant::now() + Duration::from_secs(360);
    let mut source = File::open(archive)?;
    if source.metadata()?.len()
        != pin["asset"]["bytes"]
            .as_u64()
            .ok_or("archive size absent")?
    {
        return Err("SDK archive extent differs from its pin".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        if Instant::now() >= deadline {
            return Err("SDK stream deadline expired".into());
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    if Some(hex(&hash.finalize()).as_str()) != pin["asset"]["sha256"].as_str() {
        return Err("SDK archive digest differs from its pin".into());
    }
    source.seek(SeekFrom::Start(0))?;
    let decoded = lzma_rust2::XzReader::new_mem_limit(BufReader::new(source), true, 256 * 1024);
    let stdout = io::stdout();
    let mut output = stdout.lock();
    scan_archive(decoded, &pin, &mut output, deadline)
}
fn scan_archive<R: Read, W: Write>(
    inner: R,
    pin: &Value,
    mut output: W,
    deadline: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    let max_entries = limit(&pin, "archiveEntries")?;
    let declared_limit = limit(&pin, "declaredUncompressedBytes")?;
    // Header, GNU/PAX extension and padding overhead also has a fixed stream cap.
    let stream_limit = declared_limit
        .checked_add(
            max_entries
                .checked_mul(8192)
                .ok_or("header budget overflow")?,
        )
        .ok_or("stream budget overflow")?;
    let reader = Bounded {
        inner,
        remaining: stream_limit,
        deadline,
    };
    let mut contents = tar::Archive::new(reader);
    let mut names = HashSet::new();
    let (mut entries, mut declared, mut selected_bytes, mut selected_count, mut links) =
        (0_u64, 0_u64, 0_u64, 0_u64, 0_u64);

    for entry in contents.entries()? {
        let mut entry = entry?;
        entries += 1;
        if entries > max_entries {
            return Err("SDK archive entry count exceeds its bound".into());
        }
        let name = checked_name(std::str::from_utf8(&entry.path_bytes())?)?;
        let kind = entry.header().entry_type();
        if !kind.is_dir() && !names.insert(name.to_lowercase().case_fold().collect::<String>()) {
            return Err("SDK archive contains duplicate file identities".into());
        }
        let size = entry.size();
        declared = declared.checked_add(size).ok_or("declared size overflow")?;
        if declared > declared_limit {
            return Err("SDK declared uncompressed bytes exceed their bound".into());
        }
        let selected = category(&name, &pin);
        if kind.is_symlink() || kind.is_hard_link() {
            links += 1;
            if selected.is_some() {
                return Err("selected SDK input is a link instead of original bytes".into());
            }
            continue;
        }
        let Some(category) = selected else { continue };
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err("selected SDK input is not an ordinary file".into());
        }
        selected_count += 1;
        selected_bytes = selected_bytes
            .checked_add(size)
            .ok_or("selected size overflow")?;
        if size > limit(&pin, "selectedFileBytes")?
            || selected_count > limit(&pin, "selectedFiles")?
            || selected_bytes > limit(&pin, "selectedBytes")?
        {
            return Err("selected SDK input exceeds its bound".into());
        }
        let capacity = usize::try_from(size)?;
        let mut payload = Vec::new();
        payload.try_reserve_exact(capacity)?;
        (&mut entry).take(size + 1).read_to_end(&mut payload)?;
        if payload.len() != capacity {
            return Err("selected SDK payload extent changed".into());
        }
        serde_json::to_writer(
            &mut output,
            &json!({"path":name,"category":category,"bytes":size,"sha256":hex(&Sha256::digest(&payload)),"hex":hex(&payload)}),
        )?;
        output.write_all(b"\n")?;
    }
    // Drain the stream to validate the final XZ checksum/footer and the total cap.
    let mut reader = contents.into_inner();
    io::copy(&mut reader, &mut io::sink())?;
    serde_json::to_writer(
        &mut output,
        &json!({"archiveEntriesRead":entries,"declaredUncompressedBytes":declared,"linkEntriesNotExtracted":links,"selectedBytesRead":selected_bytes,"complete":true}),
    )?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "SDK inventory rejected: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(
        entries: &[(&str, tar::EntryType, &[u8])],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, kind, payload) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_size(u64::try_from(payload.len())?);
            header.set_mode(0o644);
            if kind.is_symlink() || kind.is_hard_link() {
                header.set_link_name("remote.h")?;
            }
            header.set_cksum();
            builder.append_data(&mut header, name, *payload)?;
        }
        Ok(builder.into_inner()?)
    }
    #[test]
    fn stream_rejects_duplicate_links_and_declared_budgets()
    -> Result<(), Box<dyn std::error::Error>> {
        let pin = json!({"intendedQaicPath":"sdk/qaic.exe","limits":{"archiveEntries":10,"declaredUncompressedBytes":1024,"selectedFileBytes":512,"selectedFiles":4,"selectedBytes":512}});
        let valid = archive(&[(
            "sdk/remote.h",
            tar::EntryType::Regular,
            b"typedef int remote_handle;",
        )])?;
        let mut output = Vec::new();
        scan_archive(
            valid.as_slice(),
            &pin,
            &mut output,
            Instant::now() + Duration::from_secs(5),
        )?;
        let text = std::str::from_utf8(&output)?;
        let lines = text
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["path"], "sdk/remote.h");
        assert_eq!(lines[1]["complete"], true);
        for entries in [
            vec![
                ("sdk/remote.h", tar::EntryType::Regular, b"a".as_slice()),
                ("sdk/REMOTE.H", tar::EntryType::Regular, b"b".as_slice()),
            ],
            vec![("sdk/remote.h", tar::EntryType::Symlink, b"".as_slice())],
            vec![("sdk/remote.h", tar::EntryType::Link, b"".as_slice())],
            vec![
                ("sdk/straße", tar::EntryType::Regular, b"a".as_slice()),
                ("sdk/STRASSE", tar::EntryType::Regular, b"b".as_slice()),
            ],
        ] {
            let bytes = archive(&entries)?;
            assert!(
                scan_archive(
                    bytes.as_slice(),
                    &pin,
                    io::sink(),
                    Instant::now() + Duration::from_secs(5)
                )
                .is_err()
            );
        }
        for (field, value) in [
            ("archiveEntries", 0),
            ("declaredUncompressedBytes", 1),
            ("selectedFileBytes", 1),
            ("selectedFiles", 0),
            ("selectedBytes", 1),
        ] {
            let mut changed = pin.clone();
            changed["limits"][field] = json!(value);
            assert!(
                scan_archive(
                    valid.as_slice(),
                    &changed,
                    io::sink(),
                    Instant::now() + Duration::from_secs(5)
                )
                .is_err()
            );
        }
        for name in [
            "../remote.h",
            "/remote.h",
            "C:/remote.h",
            "sdk\\remote.h",
            "sdk/\nremote.h",
        ] {
            assert!(checked_name(name).is_err());
        }
        Ok(())
    }
}
