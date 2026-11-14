//! Locked source notice selection and exact immutable upstream fallback policy.
use super::{Result, bytes, path, read_json, resolve, text};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn notice_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "COPYRIGHT",
        "NOTICE",
        "AUTHORS",
    ]
    .into_iter()
    .any(|prefix| {
        upper
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(['.', '_', '-']))
    }) || [
        "HACK-REGULAR.TXT",
        "UFL.TXT",
        "OFL.TXT",
        "EMOJI-ICON-FONT-MIT-LICENSE.TXT",
        "EGUI-ICONS.TXT",
    ]
    .contains(&upper.as_str())
}

fn decoded(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        if bytes.len() % 2 != 0 {
            return Err("Invalid UTF-16 notice extent.".into());
        }
        let units: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|error| error.to_string())
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|error| error.to_string())
    }
}

pub(super) fn provenance(value: &Value) -> Result<Value> {
    let repository = text(value, "repository")?;
    let rest = repository
        .strip_prefix("https://github.com/")
        .ok_or("Missing notices require reviewed GitHub repository provenance.")?
        .trim_end_matches('/');
    if rest.contains(['\r', '\n', '?', '#']) {
        return Err("Invalid upstream notice repository.".into());
    }
    let parts: Vec<_> = rest.split('/').collect();
    let valid = |name: &str| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    };
    if parts.len() < 2
        || !valid(parts[0])
        || !valid(parts[1])
        || (parts.len() > 2 && (parts.len() < 4 || parts[2] != "tree"))
    {
        return Err("Invalid upstream notice repository.".into());
    }
    let upstream = format!(
        "{}/{}",
        parts[0],
        parts[1].strip_suffix(".git").unwrap_or(parts[1])
    );
    let record = read_json(&path(value, "directory")?.join(".cargo_vcs_info.json"))?;
    let recorded = text(&record["git"], "sha1")?;
    if !crate::identity::is_lower_hex(recorded, 40) {
        return Err("Missing notices require exact source commit.".into());
    }
    let revision = if upstream == "emilk/egui"
        && recorded == "7c0b40402271e8d40cda69f87cf3c2617c2e4382"
        && value["packageVersion"] == "0.37.0"
        && [
            "ecolor",
            "egui",
            "egui-wgpu",
            "egui-winit",
            "emath",
            "epaint",
        ]
        .contains(&text(value, "packageName")?)
    {
        "841a328ba2dd8f6996beb7189ebb7fc00e668a2a"
    } else {
        recorded
    };
    Ok(json!({"upstream":upstream,"revision":revision,"recordedRevision":recorded}))
}

pub(super) fn tree(value: &Value) -> Result<Value> {
    let response = &value["response"];
    let files = response["tree"]
        .as_array()
        .ok_or("Invalid upstream notice tree.")?;
    if files.len() > 100_000 || response["truncated"] == true {
        return Err("Incomplete upstream notice tree.".into());
    }
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for file in files {
        let name = text(file, "path")?;
        if file["type"] == "blob"
            && file["mode"] == "100644"
            && !name.contains('/')
            && notice_name(name)
        {
            if !seen.insert(name)
                || !crate::identity::is_lower_hex(text(file, "sha")?, 40)
                || file["size"]
                    .as_u64()
                    .is_none_or(|size| size == 0 || size > 4 * 1024 * 1024)
            {
                return Err("Invalid upstream notice identity or size.".into());
            }
            selected.push(file);
        }
    }
    if selected.is_empty() {
        return Err("No repository-root license material at immutable source.".into());
    }
    selected.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!(selected))
}

fn base64(source: &str) -> Result<Vec<u8>> {
    let data: Vec<_> = source
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if data.len() % 4 != 0 || data.len() > 6 * 1024 * 1024 {
        return Err("Invalid notice base64 extent.".into());
    }
    let mut bytes = Vec::new();
    for (index, part) in data.as_chunks::<4>().0.iter().enumerate() {
        let decode = |b: u8| match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("Invalid notice base64.".to_owned()),
        };
        let a = decode(part[0])?;
        let b = decode(part[1])?;
        let c = if part[2] == b'=' { 0 } else { decode(part[2])? };
        let d = if part[3] == b'=' { 0 } else { decode(part[3])? };
        if (part[2] == b'=' && part[3] != b'=')
            || (part[3] == b'=' && index + 1 != data.len() / 4)
            || (part[2] == b'=' && b & 15 != 0)
            || (part[3] == b'=' && c & 3 != 0)
        {
            return Err("Invalid notice base64 padding.".into());
        }
        bytes.push((a << 2) | (b >> 4));
        if part[2] != b'=' {
            bytes.push((b << 4) | (c >> 2));
        }
        if part[3] != b'=' {
            bytes.push((c << 6) | d);
        }
    }
    Ok(bytes)
}

pub(super) fn blob(value: &Value) -> Result<Value> {
    let response = &value["response"];
    let file = &value["file"];
    if response["sha"] != file["sha"]
        || response["encoding"] != "base64"
        || response["size"] != file["size"]
        || file["size"]
            .as_u64()
            .is_none_or(|size| size == 0 || size > 4 * 1024 * 1024)
    {
        return Err("Upstream notice identity changed.".into());
    }
    let raw = base64(text(response, "content")?)?;
    if u64::try_from(raw.len()).ok() != file["size"].as_u64() {
        return Err("Upstream notice size changed.".into());
    }
    Ok(json!({"text":decoded(&raw)?,"sha256":crate::identity::sha256(&raw)}))
}

fn collect(
    root: &Path,
    current: &Path,
    files: &mut BTreeSet<PathBuf>,
    count: &mut usize,
    depth: u32,
) -> Result<()> {
    if depth > 64 {
        return Err("Notice source nesting exceeds bound.".into());
    }
    resolve(current, true, false)?;
    for entry in fs::read_dir(current).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        *count += 1;
        if *count > 100_000 {
            return Err("Notice source inventory exceeds bound.".into());
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if bonsai_artifacts::is_redirect(&metadata) {
            return Err("Redirected notice source.".into());
        }
        if metadata.is_dir() {
            collect(root, &entry.path(), files, count, depth + 1)?;
        } else if metadata.is_file() && notice_name(&entry.file_name().to_string_lossy()) {
            files.insert(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn source(package: &Value) -> Result<Value> {
    let root = path(package, "manifest_path")?
        .parent()
        .ok_or("Missing package source root.")?
        .to_owned();
    let mut files = BTreeSet::new();
    collect(&root, &root, &mut files, &mut 0, 0)?;
    if let Some(license) = package["license_file"].as_str().filter(|s| !s.is_empty()) {
        let declared = PathBuf::from(license);
        let declared = if declared.is_absolute() {
            declared
        } else {
            root.join(declared)
        };
        resolve(&declared, false, false)?;
        files.insert(declared);
    }
    let mut content = String::new();
    for file in &files {
        let path = root.join(file);
        let raw = bytes(&path, 4 * 1024 * 1024)?;
        write!(
            content,
            "\n--- {} ---\n{}\n",
            file.display(),
            decoded(&raw)?
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(
        json!({"label":format!("{} {} [{}] source={}",text(package,"name")?,text(package,"version")?,package["license"].as_str().unwrap_or(""),text(package,"source")?),"text":content,"upstreamRequired":files.is_empty(),"directory":root,"repository":package["repository"],"packageName":package["name"],"packageVersion":package["version"]}),
    )
}

pub(super) fn sources(value: &Value) -> Result<Value> {
    let metadata = &value["metadata"];
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Missing Cargo packages.")?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("Missing resolved Cargo inventory.")?;
    if packages.len() > 10_000 || nodes.len() > 10_000 {
        return Err("Cargo notice inventory exceeds bound.".into());
    }
    let resolved = nodes
        .iter()
        .map(|node| text(node, "id"))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut ordered: Vec<_> = packages.iter().collect();
    ordered.sort_by(|a, b| {
        (a["name"].as_str(), a["version"].as_str())
            .cmp(&(b["name"].as_str(), b["version"].as_str()))
    });
    let mut result = Vec::new();
    for package in ordered {
        if package["source"].is_null() || !resolved.contains(text(package, "id")?) {
            continue;
        }
        if package["license"].as_str().is_none_or(str::is_empty)
            && package["license_file"].as_str().is_none_or(str::is_empty)
        {
            return Err("Missing source license declaration.".into());
        }
        result.push(source(package)?);
    }
    let fonts: Vec<_> = packages
        .iter()
        .filter(|p| p["name"] == "epaint_default_fonts")
        .collect();
    if fonts.len() != 1 {
        return Err("Expected exactly one locked epaint_default_fonts package.".into());
    }
    let font_root = path(fonts[0], "manifest_path")?
        .parent()
        .ok_or("Missing font source root.")?
        .join("fonts");
    for name in [
        "Hack-Regular.txt",
        "UFL.txt",
        "emoji-icon-font-mit-license.txt",
        "egui-icons.txt",
    ] {
        resolve(&font_root.join(name), false, false)?;
    }
    let repository = path(value, "repositoryRoot")?;
    for name in ["LICENSE.qwen-core", "LICENSE.ggml"] {
        result.push(json!({"label":format!("Rust speech port provenance: {name}"),"text":decoded(&bytes(&repository.join("src/speech-tts").join(name),4*1024*1024)?)?,"upstreamRequired":false}));
    }
    Ok(json!(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_notice_admits_ordinary_source_files_inside_the_checkout() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("Missing source root.")?;
        let license = bytes(&root.join("src/speech-tts/LICENSE.ggml"), 4 * 1024 * 1024)?;
        assert!(!license.is_empty());
        Ok(())
    }
    #[test]
    fn release_notice_base64_and_tree_reject_changed_size_redirects_and_padding() -> Result<()> {
        assert_eq!(base64("Y W J j\n")?, b"abc");
        for bad in ["YQ=Z", "YQ==YQ==", "YR==", "YQ="] {
            assert!(base64(bad).is_err());
        }
        let file = json!({"path":"LICENSE-MIT","type":"blob","mode":"100644","sha":"a".repeat(40),"size":3});
        assert_eq!(tree(&json!({"response":{"tree":[file]}}))?, json!([file]));
        assert!(tree(&json!({"response":{"tree":[file],"truncated":true}})).is_err());
        assert!(blob(&json!({"file":file,"response":{"sha":"a".repeat(40),"encoding":"base64","size":3,"content":"YQ=="}})).is_err());
        Ok(())
    }
}
