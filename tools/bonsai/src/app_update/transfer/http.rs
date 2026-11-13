//! Conditional public metadata requests with bounded gzip decoding and storage.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Read as _,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::super::policy::Asset;
use super::{MAX_METADATA, TemporaryBody, external};

const MAX_HEADERS: u64 = 64 * 1024;
const API_VERSION: &str = "2022-11-28";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cached {
    url: String,
    sha256: String,
    etag: Option<String>,
    modified: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cooldown {
    until: u64,
    failures: u32,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn header_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && value.bytes().all(|byte| (32..127).contains(&byte))
}

fn slot(url: &str) -> crate::Result<&'static str> {
    let api = format!("https://api.github.com/repos/{}/", super::super::REPOSITORY);
    for (page, slot) in [(1, "releases-1"), (2, "releases-2"), (3, "releases-3")] {
        if url == format!("{api}releases?per_page=100&page={page}") {
            return Ok(slot);
        }
    }
    if let Some(version) = url.strip_prefix(&format!("{api}commits/v")) {
        super::super::policy::Version::parse(version)?;
        return Ok("source");
    }
    let prefix = format!(
        "https://github.com/{}/releases/download/v",
        super::super::REPOSITORY
    );
    if let Some((version, name)) = url
        .strip_prefix(&prefix)
        .and_then(|tail| tail.split_once('/'))
    {
        super::super::policy::Version::parse(version)?;
        return match name {
            "release-manifest.json" => Ok("manifest"),
            "SHA256SUMS.txt" => Ok("checksums"),
            _ => Err("Unsupported update metadata asset URL.".into()),
        };
    }
    Err("Unsupported update metadata URL.".into())
}

fn cached(root: &Path, slot: &str, url: &str) -> Option<(Cached, Vec<u8>)> {
    let descriptor = super::read_bounded(&root.join(format!("{slot}.json")), MAX_HEADERS).ok()?;
    let entry: Cached = serde_json::from_slice(&descriptor).ok()?;
    if entry.url != url
        || !crate::identity::is_lower_hex(&entry.sha256, 64)
        || entry
            .etag
            .as_deref()
            .is_some_and(|value| !header_value(value))
        || entry
            .modified
            .as_deref()
            .is_some_and(|value| !header_value(value))
    {
        return None;
    }
    let body = super::read_bounded(&root.join(format!("{slot}.body")), MAX_METADATA).ok()?;
    (crate::identity::sha256(&body) == entry.sha256).then_some((entry, body))
}

fn discovery_cache(
    root: &Path,
    slot: &str,
    url: &str,
    conditional: bool,
) -> Option<(Cached, Vec<u8>)> {
    conditional.then(|| cached(root, slot, url)).flatten()
}

fn headers(bytes: &[u8]) -> crate::Result<BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut headers = BTreeMap::new();
    for line in text.lines() {
        // Ignore proxy CONNECT, informational and redirect response headers.
        if line.starts_with("HTTP/") {
            headers.clear();
        } else if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            if header_value(value) {
                headers.insert(name.to_ascii_lowercase(), value.into());
            }
        }
    }
    Ok(headers)
}

fn decoded(bytes: &[u8], encoding: Option<&String>) -> crate::Result<Vec<u8>> {
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_METADATA {
        return Err("Update metadata exceeds its byte limit.".into());
    }
    match encoding.map(String::as_str).unwrap_or("identity") {
        "identity" => Ok(bytes.to_vec()),
        "gzip" => {
            let mut body = Vec::new();
            flate2::read::MultiGzDecoder::new(bytes)
                .take(MAX_METADATA + 1)
                .read_to_end(&mut body)
                .map_err(|error| error.to_string())?;
            if u64::try_from(body.len()).map_err(|error| error.to_string())? > MAX_METADATA {
                return Err("Expanded update metadata exceeds its byte limit.".into());
            }
            Ok(body)
        }
        _ => Err("Unsupported update metadata content encoding.".into()),
    }
}

// GitHub documents numeric Retry-After. Also accept the standard IMF-fixdate
// form without adding a date/time runtime or interpreting local timezone data.
fn http_date(value: &str) -> Option<u64> {
    let words: Vec<_> = value.split_ascii_whitespace().collect();
    if words.len() != 6 || words[5] != "GMT" {
        return None;
    }
    let day: u64 = words[1].parse().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|month| *month == words[2])?;
    let year: u64 = words[3].parse().ok()?;
    let clock: Vec<u64> = words[4]
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [hour, minute, second] = clock.as_slice() else {
        return None;
    };
    if !(1970..=9999).contains(&year) || day == 0 || *hour > 23 || *minute > 59 || *second > 59 {
        return None;
    }
    let leap = |year: u64| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day > months[month] {
        return None;
    }
    let leaps = |year: u64| year / 4 - year / 100 + year / 400;
    let days = (year - 1970) * 365 + leaps(year - 1) - leaps(1969)
        + months[..month].iter().sum::<u64>()
        + day
        - 1;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn cooldown_delay(headers: &BTreeMap<String, String>, now: u64, failures: u32) -> u64 {
    let retry = headers.get("retry-after").and_then(|value| {
        value
            .parse::<u64>()
            .ok()
            .map(|seconds| now.saturating_add(seconds))
            .or_else(|| http_date(value))
    });
    let reset = (headers.get("x-ratelimit-remaining").map(String::as_str) == Some("0"))
        .then(|| {
            headers
                .get("x-ratelimit-reset")
                .and_then(|value| value.parse::<u64>().ok())
        })
        .flatten();
    // No immediate retries. Repeated failures grow from 15 minutes to six hours.
    let default = now.saturating_add(900_u64.saturating_mul(1_u64 << failures.min(5)).min(21600));
    default.max(retry.unwrap_or(0)).max(reset.unwrap_or(0))
}

fn load_cooldown(root: &Path) -> Cooldown {
    super::read_bounded(&root.join("cooldown.json"), MAX_HEADERS)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn store_cooldown(root: &Path, cooldown: &Cooldown) -> crate::Result<()> {
    super::write_current(
        &root.join("cooldown.json"),
        &serde_json::to_vec(cooldown).map_err(|error| error.to_string())?,
    )
}

fn arguments(url: &str, body: &Path, headers: &Path, cached: Option<&Cached>) -> Vec<OsString> {
    let mut arguments = vec![
        "--disable".into(),
        "--silent".into(),
        "--show-error".into(),
        "--location".into(),
        "--max-redirs".into(),
        "3".into(),
        "--proto".into(),
        "=https".into(),
        "--proto-redir".into(),
        "=https".into(),
        "--disallow-username-in-url".into(),
        "--tlsv1.2".into(),
        "--connect-timeout".into(),
        "5".into(),
        "--max-time".into(),
        "10".into(),
        "--max-filesize".into(),
        MAX_METADATA.to_string().into(),
        "--header".into(),
        "Accept: application/vnd.github+json".into(),
        "--header".into(),
        format!("X-GitHub-Api-Version: {API_VERSION}").into(),
        "--header".into(),
        format!(
            "User-Agent: SwiftBonsai2Runner/{}",
            env!("CARGO_PKG_VERSION")
        )
        .into(),
        // Decode gzip natively: older Windows curl only bounds the compressed
        // bytes with --compressed. Both input and expansion stay independently bounded.
        "--header".into(),
        "Accept-Encoding: gzip, identity".into(),
        "--write-out".into(),
        "%{http_code}".into(),
        "--dump-header".into(),
        headers.as_os_str().to_owned(),
        "--output".into(),
        body.as_os_str().to_owned(),
    ];
    if let Some(cached) = cached {
        if let Some(etag) = &cached.etag {
            arguments.extend(["--header".into(), format!("If-None-Match: {etag}").into()]);
        } else if let Some(modified) = &cached.modified {
            arguments.extend([
                "--header".into(),
                format!("If-Modified-Since: {modified}").into(),
            ]);
        }
    }
    arguments.push(url.into());
    arguments
}

pub(super) fn metadata(
    url: &str,
    working: &Path,
    asset: Option<&Asset>,
    conditional: bool,
) -> crate::Result<Vec<u8>> {
    let slot = slot(url)?;
    let root = super::super::cache_root()?.join("http");
    external(&root)?;
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    external(&root)?;
    let _lock = crate::startup_cache::lock_file(&root.join("cache.lock"))?;
    // User-writable cache is only an efficiency hint for read-only discovery.
    // Every mutating gate requires new unconditional HTTPS publication bytes.
    let cached = discovery_cache(&root, slot, url, conditional);
    if let (Some(asset), Some((entry, body))) = (asset, &cached)
        && entry.sha256 == asset.sha256
        && u64::try_from(body.len()).map_err(|error| error.to_string())? == asset.bytes
    {
        // The freshly revalidated catalog authenticates these immutable bytes.
        return Ok(body.clone());
    }
    let cooldown = load_cooldown(&root);
    if cooldown.until > now() {
        return Err(format!(
            "GitHub update requests are paused until Unix time {} after a server or network failure. Your installed app is unchanged.",
            cooldown.until
        ));
    }
    #[cfg(windows)]
    let directory = bonsai_windows_credentials::PrivateDirectory::create(working)
        .map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let working = directory.path();
    let body = working.join(format!(
        "metadata-{}.body",
        crate::startup_process::unique()
    ));
    let header = body.with_extension("headers");
    external(&body)?;
    external(&header)?;
    let _body = TemporaryBody(body.clone());
    let _headers = TemporaryBody(header.clone());
    let curl = super::curl(working)?;
    let result = crate::startup_process::run(
        curl.as_os_str(),
        &arguments(url, &body, &header, cached.as_ref().map(|(entry, _)| entry)),
        working,
        &BTreeMap::new(),
        Duration::from_secs(11),
        &body.with_extension("log"),
    );
    let headers = super::read_bounded(&header, MAX_HEADERS).and_then(|bytes| headers(&bytes));
    let failed = !result
        .as_ref()
        .is_ok_and(|result| result.code == 0 && matches!(result.stdout.trim(), "200" | "304"));
    if failed {
        let empty = BTreeMap::new();
        store_cooldown(
            &root,
            &Cooldown {
                until: cooldown_delay(headers.as_ref().unwrap_or(&empty), now(), cooldown.failures),
                failures: cooldown.failures.saturating_add(1),
            },
        )?;
    }
    let result = result?;
    let headers = headers?;
    if result.code == 0 && result.stdout.trim() == "304" {
        let (_, body) = cached
            .filter(|(entry, _)| entry.etag.is_some() || entry.modified.is_some())
            .ok_or("GitHub returned 304 without a valid conditional cache entry.")?;
        if cooldown.failures != 0 || cooldown.until != 0 {
            store_cooldown(&root, &Cooldown::default())?;
        }
        return Ok(body);
    }
    super::require_successful_http(result.code, &result.stdout)?;
    let body = decoded(
        &super::read_bounded(&body, MAX_METADATA)?,
        headers.get("content-encoding"),
    )?;
    let descriptor = Cached {
        url: url.into(),
        sha256: crate::identity::sha256(&body),
        etag: headers.get("etag").cloned(),
        modified: headers.get("last-modified").cloned(),
    };
    super::write_current(&root.join(format!("{slot}.body")), &body)?;
    super::write_current(
        &root.join(format!("{slot}.json")),
        &serde_json::to_vec(&descriptor).map_err(|error| error.to_string())?,
    )?;
    if cooldown.failures != 0 || cooldown.until != 0 {
        store_cooldown(&root, &Cooldown::default())?;
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn update_http_cache_admits_only_exact_bounded_slots_and_conditional_headers()
    -> crate::Result<()> {
        let api = format!(
            "https://api.github.com/repos/{}/",
            super::super::super::REPOSITORY
        );
        for page in 1..=3 {
            assert!(slot(&format!("{api}releases?per_page=100&page={page}")).is_ok());
        }
        assert!(slot(&format!("{api}releases?per_page=100&page=4")).is_err());
        assert!(slot("https://evil.invalid/releases").is_err());
        assert!(!header_value("\"etag\"\r\nAuthorization: secret"));
        let entry = Cached {
            url: String::new(),
            sha256: String::new(),
            etag: Some("W/\"version\"".into()),
            modified: Some("date".into()),
        };
        let arguments = arguments(
            "https://example.invalid",
            Path::new("body"),
            Path::new("headers"),
            Some(&entry),
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "If-None-Match: W/\"version\"")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.to_string_lossy().starts_with("If-Modified-Since:"))
        );
        Ok(())
    }

    #[test]
    fn update_http_gzip_bounds_expansion_and_rejects_corruption() -> crate::Result<()> {
        let compress = |bytes: &[u8]| -> crate::Result<Vec<u8>> {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder
                .write_all(bytes)
                .map_err(|error| error.to_string())?;
            encoder.finish().map_err(|error| error.to_string())
        };
        let gzip = Some(String::from("gzip"));
        assert_eq!(decoded(&compress(b"[]")?, gzip.as_ref())?, b"[]");
        assert!(decoded(b"corrupt", gzip.as_ref()).is_err());
        let oversized =
            vec![0; usize::try_from(MAX_METADATA).map_err(|error| error.to_string())? + 1];
        assert!(decoded(&compress(&oversized)?, gzip.as_ref()).is_err());
        assert!(decoded(b"[]", Some(&String::from("br"))).is_err());
        Ok(())
    }

    #[test]
    fn update_http_backoff_honors_server_deadlines_and_grows_without_retrying() {
        let mut headers = BTreeMap::from([("retry-after".into(), "7200".into())]);
        assert_eq!(cooldown_delay(&headers, 100, 0), 7300);
        headers.insert("x-ratelimit-remaining".into(), "0".into());
        headers.insert("x-ratelimit-reset".into(), "9000".into());
        assert_eq!(cooldown_delay(&headers, 100, 0), 9000);
        assert_eq!(cooldown_delay(&BTreeMap::new(), 100, 0), 1000);
        assert_eq!(cooldown_delay(&BTreeMap::new(), 100, 1), 1900);
        assert_eq!(cooldown_delay(&BTreeMap::new(), 100, 30), 21700);
        assert_eq!(http_date("Thu, 01 Jan 1970 00:00:01 GMT"), Some(1));
        assert_eq!(
            http_date("Sat, 29 Feb 2020 00:00:00 GMT"),
            Some(1_582_934_400)
        );
        assert!(http_date("Tue, 29 Feb 2022 00:00:00 GMT").is_none());
    }

    #[test]
    fn update_http_headers_use_only_the_final_response() -> crate::Result<()> {
        let headers = headers(b"HTTP/1.1 302 Found\r\nETag: old\r\nRetry-After: 9999\r\n\r\nHTTP/2 200\r\nETag: new\r\nContent-Encoding: gzip\r\n\r\n")?;
        assert_eq!(headers.get("etag").map(String::as_str), Some("new"));
        assert!(!headers.contains_key("retry-after"));
        Ok(())
    }

    #[test]
    fn update_http_fresh_gates_ignore_forged_cache_and_discovery_rejects_partial_pairs()
    -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Http-Cache-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=100&page=1",
            super::super::super::REPOSITORY
        );
        let body = b"locally forged publication bytes";
        let descriptor = Cached {
            url: url.clone(),
            sha256: crate::identity::sha256(body),
            etag: Some("\"genuine-server-validator\"".into()),
            modified: None,
        };
        super::super::write_current(&root.join("releases-1.body"), body)?;
        super::super::write_current(
            &root.join("releases-1.json"),
            &serde_json::to_vec(&descriptor).map_err(|error| error.to_string())?,
        )?;
        assert!(discovery_cache(&root, "releases-1", &url, true).is_some());
        // A new HTTPS request must authenticate every authoritative byte. The
        // writable body/hash/validator triple cannot authorize a mutating gate.
        let fresh = discovery_cache(&root, "releases-1", &url, false);
        assert!(fresh.is_none());
        let arguments = arguments(
            &url,
            Path::new("body"),
            Path::new("headers"),
            fresh.as_ref().map(|(entry, _)| entry),
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.to_string_lossy().starts_with("If-"))
        );
        assert!(discovery_cache(&root, "releases-1", "different-url", true).is_none());
        super::super::write_current(&root.join("releases-1.body"), b"partial publication")?;
        assert!(discovery_cache(&root, "releases-1", &url, true).is_none());
        fs::remove_file(root.join("releases-1.body")).map_err(|error| error.to_string())?;
        fs::remove_file(root.join("releases-1.json")).map_err(|error| error.to_string())?;
        fs::remove_dir(root).map_err(|error| error.to_string())
    }
}
