//! Bounded static ARM64 PE dependency inspection; no image is loaded or executed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;

type Aliases = BTreeMap<String, Vec<(String, String)>>;
const GUI_MODULES: &[&str] = &[
    "shell32.dll",
    "user32.dll",
    "gdi32.dll",
    "gdi32full.dll",
    "win32u.dll",
    "comdlg32.dll",
    "comctl32.dll",
    "dwmapi.dll",
    "uxtheme.dll",
    "imm32.dll",
];

struct Budget(Instant);

impl Budget {
    fn checkpoint(&self) -> Result<()> {
        crate::startup_cancel::checkpoint()?;
        if self.0.elapsed() >= Duration::from_secs(120) {
            return Err("Static PE audit exceeded its 120-second allowance".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    bundle_directory: PathBuf,
    system_directory: PathBuf,
    aliases: Option<Aliases>,
}

struct Section {
    name: String,
    virtual_size: u64,
    virtual_address: u64,
    raw_size: usize,
    raw: usize,
}

struct Image {
    data: Vec<u8>,
    optional: usize,
    base: u64,
    sections: Vec<Section>,
}

fn extent(data: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    data.get(offset..offset.checked_add(size).ok_or("PE extent outside file")?)
        .ok_or_else(|| "PE extent outside file".into())
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        extent(data, offset, 2)?
            .try_into()
            .map_err(|_| "PE extent outside file")?,
    ))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        extent(data, offset, 4)?
            .try_into()
            .map_err(|_| "PE extent outside file")?,
    ))
}

fn offset_at(data: &[u8], offset: usize) -> Result<usize> {
    usize::try_from(u32_at(data, offset)?).map_err(|error| error.to_string())
}

fn u64_at(data: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        extent(data, offset, 8)?
            .try_into()
            .map_err(|_| "PE extent outside file")?,
    ))
}

fn ordinary_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || bonsai_artifacts::is_redirect(&metadata) {
        return Err("Redirected audit directory".into());
    }
    Ok(())
}

fn read_image(path: &Path, budget: &Budget) -> Result<Image> {
    budget.checkpoint()?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) {
        return Err("Dependency must be a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) {
        return Err("Dependency must be a regular file".into());
    }
    if metadata.len() > 33_554_432 {
        return Err("PE exceeds static read bound".into());
    }
    let mut reader = file.take(33_554_433);
    let mut data = Vec::new();
    let mut buffer = [0; 65_536];
    loop {
        budget.checkpoint()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
        if data.len() > 33_554_432 {
            return Err("PE exceeds static read bound".into());
        }
    }
    parse_image(data)
}

fn parse_image(data: Vec<u8>) -> Result<Image> {
    extent(&data, 0, 64)?;
    if data.get(..2) != Some(b"MZ") {
        return Err("Missing DOS signature".into());
    }
    let pe = offset_at(&data, 60)?;
    extent(&data, pe, 24)?;
    if u32_at(&data, pe)? != 0x4550 || u16_at(&data, pe + 4)? != 0xaa64 {
        return Err("Expected native ARM64 PE".into());
    }
    let optional = pe + 24;
    let size = usize::from(u16_at(&data, pe + 20)?);
    if size < 112 {
        return Err("Truncated PE32+ optional header".into());
    }
    extent(&data, optional, size)?;
    if u16_at(&data, optional)? != 0x20b {
        return Err("Expected PE32+".into());
    }
    let directories = offset_at(&data, optional + 108)?;
    if !(14..=(size - 112) / 8).contains(&directories) {
        return Err("PE directory count exceeds optional header or omits import tables".into());
    }
    let count = usize::from(u16_at(&data, pe + 6)?);
    if count > 96 {
        return Err("Too many PE sections".into());
    }
    let section = optional + size;
    extent(&data, section, count * 40)?;
    let mut sections = Vec::new();
    for index in 0..count {
        let header = section + index * 40;
        let entry = Section {
            name: String::from_utf8_lossy(extent(&data, header, 8)?)
                .trim_end_matches('\0')
                .into(),
            virtual_size: u64::from(u32_at(&data, header + 8)?),
            virtual_address: u64::from(u32_at(&data, header + 12)?),
            raw_size: offset_at(&data, header + 16)?,
            raw: offset_at(&data, header + 20)?,
        };
        extent(&data, entry.raw, entry.raw_size)?;
        sections.push(entry);
    }
    let base = u64_at(&data, optional + 24)?;
    Ok(Image {
        data,
        optional,
        base,
        sections,
    })
}

fn mapped(image: &Image, rva: u64, size: usize) -> Result<(usize, usize)> {
    for section in &image.sections {
        let raw_size = u64::try_from(section.raw_size).map_err(|error| error.to_string())?;
        if rva >= section.virtual_address
            && rva < section.virtual_address + section.virtual_size.max(raw_size)
        {
            let delta = usize::try_from(rva - section.virtual_address)
                .map_err(|error| error.to_string())?;
            if delta > section.raw_size || size > section.raw_size - delta {
                return Err("PE RVA extent outside raw section".into());
            }
            let offset = section.raw + delta;
            extent(&image.data, offset, size)?;
            return Ok((offset, section.raw_size - delta));
        }
    }
    Err("Unmapped PE RVA".into())
}

fn safe_module(name: &str) -> bool {
    name.len() > 4
        && name.ends_with(".dll")
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
}

fn module(image: &Image, rva: u64) -> Result<String> {
    let (offset, available) = mapped(image, rva, 1)?;
    let data = extent(&image.data, offset, available.min(260))?;
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .filter(|end| *end > 0)
        .ok_or("Unterminated import name")?;
    let name = std::str::from_utf8(&data[..end])
        .map_err(|_| "Unsafe import module name")?
        .to_ascii_lowercase();
    if !safe_module(&name) {
        return Err("Unsafe import module name".into());
    }
    Ok(name)
}

fn imports(image: &Image, delayed: bool) -> Result<Vec<String>> {
    let directory_index = if delayed { 13 } else { 1 };
    let directory = image.optional + 112 + directory_index * 8;
    let rva = u64::from(u32_at(&image.data, directory)?);
    let size = offset_at(&image.data, directory + 4)?;
    if rva == 0 && size == 0 {
        return Ok(Vec::new());
    }
    let width = if delayed { 32 } else { 20 };
    if rva == 0 || !(width..=65_536).contains(&size) {
        return Err("Invalid import directory extent".into());
    }
    let (start, _) = mapped(image, rva, size)?;
    let mut result = Vec::new();
    for position in (0..=size - width).step_by(width) {
        let record = extent(&image.data, start + position, width)?;
        if record.iter().all(|byte| *byte == 0) {
            return Ok(result);
        }
        let mut name = u64::from(u32_at(record, if delayed { 4 } else { 12 })?);
        if delayed && u32_at(record, 0)? & 1 == 0 {
            name = name.checked_sub(image.base).ok_or("Unmapped PE RVA")?;
        }
        result.push(module(image, name)?);
    }
    Err("Unterminated import directory".into())
}

fn api_text(data: &[u8], offset: usize, length: usize) -> Result<String> {
    if !length.is_multiple_of(2) {
        return Err("Invalid API-set name extent".into());
    }
    let units: Vec<_> = extent(data, offset, length)?
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect();
    String::from_utf16(&units)
        .map(|value| value.to_lowercase())
        .map_err(|error| error.to_string())
}

fn api_sets(system: &Path, budget: &Budget) -> Result<Aliases> {
    let image = read_image(&system.join("apisetschema.dll"), budget)?;
    let sections: Vec<_> = image
        .sections
        .iter()
        .filter(|section| section.name == ".apiset")
        .collect();
    let [section] = sections.as_slice() else {
        return Err("Expected one API-set namespace".into());
    };
    let data = extent(&image.data, section.raw, section.raw_size)?;
    let size = offset_at(data, 4)?;
    if data.len() < 28 || u32_at(data, 0)? != 6 || !(28..=data.len()).contains(&size) {
        return Err("Unsupported API-set namespace".into());
    }
    let data = &data[..size];
    let count = offset_at(data, 12)?;
    let offset = offset_at(data, 16)?;
    if count > 4096 || offset < 28 {
        return Err("API-set entry table outside declared namespace".into());
    }
    extent(data, offset, count * 24)?;
    let mut result = Aliases::new();
    for index in 0..count {
        budget.checkpoint()?;
        let entry = offset + index * 24;
        let length = offset_at(data, entry + 8)?;
        let hashed_length = offset_at(data, entry + 12)?;
        if hashed_length == 0 || hashed_length > length {
            return Err("Invalid API-set hashed name extent".into());
        }
        let name_offset = offset_at(data, entry + 4)?;
        let name = api_text(data, name_offset, length)? + ".dll";
        let hashed = api_text(data, name_offset, hashed_length)? + "-*.dll";
        let values = offset_at(data, entry + 16)?;
        let host_count = offset_at(data, entry + 20)?;
        if host_count > 64 {
            return Err("API-set host table outside declared namespace".into());
        }
        extent(data, values, host_count * 20)?;
        let mut hosts = Vec::new();
        for host in 0..host_count {
            let value = values + host * 20;
            hosts.push((
                api_text(
                    data,
                    offset_at(data, value + 4)?,
                    offset_at(data, value + 8)?,
                )?,
                api_text(
                    data,
                    offset_at(data, value + 12)?,
                    offset_at(data, value + 16)?,
                )?,
            ));
        }
        result.insert(name, hosts.clone());
        if result
            .get(&hashed)
            .is_some_and(|previous| previous != &hosts)
        {
            return Err("Ambiguous API-set hashed contract".into());
        }
        result.insert(hashed, hosts);
    }
    Ok(result)
}

fn resolve<'a>(aliases: &'a Aliases, dependency: &'a str, importer: &str) -> Result<&'a str> {
    let hashed = dependency
        .rsplit_once('-')
        .map(|(prefix, _)| format!("{prefix}-*.dll"));
    let hosts = aliases
        .get(dependency)
        .or_else(|| hashed.as_ref().and_then(|key| aliases.get(key)));
    let Some(hosts) = hosts else {
        return Ok(dependency);
    };
    let mut matches: BTreeSet<_> = hosts
        .iter()
        .filter(|(name, _)| {
            name == importer || Some(name.as_str()) == importer.strip_suffix(".dll")
        })
        .map(|(_, host)| host.as_str())
        .collect();
    if matches.is_empty() {
        matches = hosts
            .iter()
            .filter(|(name, _)| name.is_empty())
            .map(|(_, host)| host.as_str())
            .collect();
    }
    if matches.len() != 1 {
        return Err(format!("Unresolved API-set host: {dependency}"));
    }
    matches
        .into_iter()
        .next()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| format!("Unresolved API-set host: {dependency}"))
}

fn candidate_inventory(directory: &Path, budget: &Budget) -> Result<BTreeMap<String, PathBuf>> {
    let mut candidates = BTreeMap::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        budget.checkpoint()?;
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("dll") || extension.eq_ignore_ascii_case("exe")
        }) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Non-Unicode candidate module")?
                .to_ascii_lowercase();
            if candidates.insert(name, path).is_some() {
                return Err("Ambiguous candidate module name".into());
            }
            if candidates.len() > 64 {
                return Err("Candidate inventory outside bound".into());
            }
        }
    }
    if candidates.is_empty() {
        return Err("Candidate inventory outside bound".into());
    }
    Ok(candidates)
}

fn normalize_aliases(aliases: Aliases) -> Result<Aliases> {
    if aliases.len() > 8192 || aliases.values().any(|hosts| hosts.len() > 64) {
        return Err("API-set inventory outside bound".into());
    }
    let mut result = Aliases::new();
    for (name, hosts) in aliases {
        let key = name.to_ascii_lowercase();
        if result.get(&key).is_some_and(|previous| previous != &hosts) {
            return Err("Ambiguous API-set contract casing".into());
        }
        result.insert(key, hosts);
    }
    Ok(result)
}

fn audit(request: Request) -> Result<Value> {
    let budget = Budget(Instant::now());
    ordinary_directory(&request.bundle_directory)?;
    ordinary_directory(&request.system_directory)?;
    let candidates = candidate_inventory(&request.bundle_directory, &budget)?;
    let aliases = normalize_aliases(
        request
            .aliases
            .map_or_else(|| api_sets(&request.system_directory, &budget), Ok)?,
    )?;
    let mut pending: Vec<_> = candidates
        .keys()
        .map(|name| (name.clone(), "candidate"))
        .collect();
    let mut modules = BTreeMap::new();
    let mut edges = Vec::new();
    while let Some((name, location)) = pending.pop() {
        budget.checkpoint()?;
        let key = (location, name.clone());
        if modules.contains_key(&key) {
            continue;
        }
        if GUI_MODULES.contains(&name.as_str()) {
            return Err(format!("GUI dependency: {name}"));
        }
        if modules.len() >= 128 {
            return Err("Dependency closure exceeds module bound".into());
        }
        let path = if location == "candidate" {
            candidates
                .get(&name)
                .ok_or("Missing candidate module")?
                .clone()
        } else {
            request.system_directory.join(&name)
        };
        let image = read_image(&path, &budget)?;
        let normal = imports(&image, false)?;
        let delayed = imports(&image, true)?;
        let mut hasher = crate::identity::Hasher::new()?;
        hasher.update(&image.data)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Non-Unicode module name")?;
        modules.insert(key, json!({"name":file_name, "bytes":image.data.len(), "sha256":hasher.finalize_hex()?, "machine":"0xAA64", "normalImports":normal, "delayImports":delayed, "location":location}));
        for dependency in normal
            .iter()
            .chain(delayed.iter().filter(|_| location == "candidate"))
        {
            budget.checkpoint()?;
            let resolved = resolve(&aliases, dependency, &name)?;
            if !safe_module(resolved) {
                return Err("Unsafe resolved module name".into());
            }
            let destination = if candidates.contains_key(resolved) {
                "candidate"
            } else {
                "system"
            };
            edges.push(
                json!({"from":name,"import":dependency,"resolved":resolved,"location":destination}),
            );
            pending.push((resolved.into(), destination));
        }
    }
    budget.checkpoint()?;
    Ok(
        json!({"schemaVersion":1,"status":"candidate-and-mandatory-named-import-closure-gui-free",
        "scope":"All candidate normal/delay imports and recursive system normal imports; optional OS delays are recorded, not followed or claimed absent. No DLL was loaded.",
        "forwardedExportsResolved":false,"modules":modules.into_values().collect::<Vec<_>>(),"requiredEdges":edges,"modelExecuted":false,"workerPolicyChanged":false}),
    )
}

/// Inspect candidate normal/delay imports and recursive system normal imports.
///
/// An omitted or null `aliases` reads the system's API-set namespace. Explicit
/// aliases support independently supplied replay fixtures without loading a DLL.
///
/// # Errors
/// Rejects malformed requests, redirected inputs, unsupported images, extent or
/// inventory bounds, unresolved dependencies and mandatory GUI imports.
pub fn run(request: Value) -> Result<Value> {
    audit(serde_json::from_value(request).map_err(|error| error.to_string())?)
}

#[cfg(test)]
mod tests;
