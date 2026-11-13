//! Offline release, version and cross-asset identity validation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{MAX_INSTALLER, MAX_METADATA, REPOSITORY, TARGET};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Distribution {
    Installer,
    Portable,
}

impl Distribution {
    fn asset_name(self, version: &str) -> String {
        let extension = match self {
            Self::Installer => "msi",
            Self::Portable => "exe",
        };
        format!("SwiftBonsai2Runner-{version}-windows-arm64.{extension}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Version(u8, u8, u16);

impl Version {
    #[cfg(test)]
    pub(super) fn next_for_test(self) -> crate::Result<String> {
        let next = self
            .2
            .checked_add(1)
            .map(|patch| (self.0, self.1, patch))
            .or_else(|| self.1.checked_add(1).map(|minor| (self.0, minor, 0)))
            .or_else(|| self.0.checked_add(1).map(|major| (major, 0, 0)))
            .ok_or("No newer fixture version is available.")?;
        Ok(format!("{}.{}.{}", next.0, next.1, next.2))
    }

    pub(super) fn parse(text: &str) -> crate::Result<Self> {
        let parts: Vec<_> = text.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || (part.len() > 1 && part.starts_with('0'))
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            return Err("Updates require canonical numeric Windows Installer versions.".into());
        }
        Ok(Self(
            parts[0]
                .parse()
                .map_err(|_| "Major version exceeds MSI bounds.")?,
            parts[1]
                .parse()
                .map_err(|_| "Minor version exceeds MSI bounds.")?,
            parts[2]
                .parse()
                .map_err(|_| "Patch version exceeds MSI bounds.")?,
        ))
    }

    pub(super) fn newer_than_current(self) -> crate::Result<()> {
        if self <= Self::parse(env!("CARGO_PKG_VERSION"))? {
            return Err(
                "The updater never reinstalls or downgrades the current application.".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Asset {
    pub(super) name: String,
    pub(super) bytes: u64,
    pub(super) sha256: String,
    pub(super) url: String,
}

#[derive(Clone)]
pub(super) struct Candidate {
    pub(super) version: String,
    pub(super) release_url: String,
    pub(super) manifest: Asset,
    pub(super) checksums: Asset,
    pub(super) msi: Asset,
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Installer {
    schema_version: u32,
    version: String,
    source_revision: String,
    target: String,
    pub(super) asset: Asset,
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing release field: {key}"))
}

fn canonical_asset(version: &str, name: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/v{version}/{name}")
}

fn asset(release: &Value, version: &str, name: &str, limit: u64) -> crate::Result<Asset> {
    let matches: Vec<_> = release["assets"]
        .as_array()
        .ok_or("Missing published release assets.")?
        .iter()
        .filter(|asset| asset["name"] == name)
        .collect();
    let [entry] = matches.as_slice() else {
        return Err(format!("Release asset is missing or repeated: {name}"));
    };
    let bytes = entry["size"]
        .as_u64()
        .filter(|bytes| *bytes > 0 && *bytes <= limit)
        .ok_or("Release asset exceeds its byte limit.")?;
    let sha256 = text(entry, "digest")?
        .strip_prefix("sha256:")
        .filter(|digest| crate::identity::is_lower_hex(digest, 64))
        .ok_or("GitHub must report a SHA-256 identity for every update asset.")?;
    let url = text(entry, "browser_download_url")?;
    if url != canonical_asset(version, name) || entry["state"] != "uploaded" {
        return Err(
            "Update assets must be fully uploaded at the canonical project release URL.".into(),
        );
    }
    Ok(Asset {
        name: name.into(),
        bytes,
        sha256: sha256.into(),
        url: url.into(),
    })
}

pub(super) fn select(bytes: &[u8], current: &str) -> crate::Result<(bool, Option<Candidate>)> {
    select_distribution(bytes, current, Distribution::Installer)
}

pub(super) fn select_portable(
    bytes: &[u8],
    current: &str,
) -> crate::Result<(bool, Option<Candidate>)> {
    select_distribution(bytes, current, Distribution::Portable)
}

fn select_distribution(
    bytes: &[u8],
    current: &str,
    distribution: Distribution,
) -> crate::Result<(bool, Option<Candidate>)> {
    let current = Version::parse(current)?;
    let releases: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let releases = releases
        .as_array()
        // Discovery admits at most three bounded GitHub catalog pages.
        .filter(|list| list.len() <= 300)
        .ok_or("Release response must be a bounded array.")?;
    let mut eligible = std::collections::BTreeMap::new();
    let mut published = false;
    for release in releases {
        if release["draft"].as_bool().is_none() || release["prerelease"].as_bool().is_none() {
            return Err("Invalid release visibility metadata.".into());
        }
        if release["draft"] == true || release["published_at"].as_str().is_none_or(str::is_empty) {
            continue;
        }
        published = true;
        let Some(version) = release["tag_name"]
            .as_str()
            .and_then(|tag| tag.strip_prefix('v'))
        else {
            continue;
        };
        let Ok(parsed) = Version::parse(version) else {
            continue;
        };
        if parsed <= current {
            continue;
        }
        if eligible.insert(parsed, (version, release)).is_some() {
            return Err("Published release versions must be unique.".into());
        }
    }
    let Some((_, (version, release))) = eligible.pop_last() else {
        return Ok((published, None));
    };
    // GitHub's ordering is not a numeric-version guarantee. Select first so a
    // historical incomplete release cannot block a valid newer one depending
    // on its position, and never fall back if the newest release is invalid.
    let release_url = format!("https://github.com/{REPOSITORY}/releases/tag/v{version}");
    if text(release, "html_url")? != release_url {
        return Err("Release belongs to another repository.".into());
    }
    let candidate = Candidate {
        version: version.into(),
        release_url,
        manifest: asset(release, version, "release-manifest.json", MAX_METADATA)?,
        checksums: asset(release, version, "SHA256SUMS.txt", MAX_METADATA)?,
        msi: asset(
            release,
            version,
            &distribution.asset_name(version),
            MAX_INSTALLER,
        )?,
    };
    Ok((published, Some(candidate)))
}

fn checksum(bytes: &[u8], expected_name: &str) -> crate::Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut matched = None;
    let mut names = std::collections::BTreeSet::new();
    for line in text.lines() {
        let (digest, name) = line
            .split_once("  ")
            .ok_or("Invalid update checksum record.")?;
        if !crate::identity::is_lower_hex(digest, 64)
            || name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || !names.insert(name)
        {
            return Err("Malformed or repeated update checksum filename.".into());
        }
        if name == expected_name {
            matched = Some(digest.into());
        }
    }
    matched.ok_or_else(|| "The application update is absent from SHA256SUMS.txt.".into())
}

impl Installer {
    pub(super) fn release_identity(&self) -> crate::Result<String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(crate::identity::sha256(&bytes))
    }

    pub(super) fn verify_release_identity(&self, expected: &str) -> crate::Result<()> {
        if !crate::identity::is_lower_hex(expected, 64) || self.release_identity()? != expected {
            return Err(
                "The published update changed after it was downloaded. Check for updates and confirm the verified download again. No application was replaced."
                    .into(),
            );
        }
        Ok(())
    }

    pub(super) fn distribution(&self) -> Distribution {
        if std::path::Path::new(&self.asset.name)
            .extension()
            .is_some_and(|extension| extension == "exe")
        {
            Distribution::Portable
        } else {
            Distribution::Installer
        }
    }

    pub(super) fn validate(&self, version: &str) -> crate::Result<()> {
        Version::parse(version)?;
        let name = self.distribution().asset_name(version);
        if self.schema_version != 1
            || self.version != version
            || self.target != TARGET
            || !crate::identity::is_lower_hex(&self.source_revision, 40)
            || self.asset.name != name
            || self.asset.bytes == 0
            || self.asset.bytes > MAX_INSTALLER
            || !crate::identity::is_lower_hex(&self.asset.sha256, 64)
            || self.asset.url != canonical_asset(version, &name)
        {
            return Err(
                "Staged application identity is not a project Windows ARM64 release.".into(),
            );
        }
        Ok(())
    }

    pub(super) fn verify_source(&self, bytes: &[u8]) -> crate::Result<()> {
        let commit: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if commit["sha"] != self.source_revision {
            return Err("Published release tag and application source revisions disagree.".into());
        }
        Ok(())
    }
}

pub(super) fn installer(
    candidate: &Candidate,
    manifest: &[u8],
    checksums: &[u8],
) -> crate::Result<Installer> {
    application(candidate, manifest, checksums, Distribution::Installer)
}

pub(super) fn portable(
    candidate: &Candidate,
    manifest: &[u8],
    checksums: &[u8],
) -> crate::Result<Installer> {
    application(candidate, manifest, checksums, Distribution::Portable)
}

fn application(
    candidate: &Candidate,
    manifest: &[u8],
    checksums: &[u8],
    distribution: Distribution,
) -> crate::Result<Installer> {
    let manifest: Value = serde_json::from_slice(manifest).map_err(|error| error.to_string())?;
    let expected_tag = format!("v{}", candidate.version);
    let checked = match distribution {
        Distribution::Installer => {
            manifest["installerSmoke"] == "passed-install-inspect-uninstall-without-model"
        }
        Distribution::Portable => {
            manifest["distribution"] == "portable-executable"
                && manifest["executableSmoke"] == "passed-inspect-extract-verify-without-model"
                && text(&manifest, "manifestSha256")
                    .is_ok_and(|digest| crate::identity::is_lower_hex(digest, 64))
                && manifest["assets"]
                    .as_array()
                    .is_some_and(|assets| assets.len() == 1)
        }
    };
    if manifest["schemaVersion"] != 1
        || manifest["tag"] != expected_tag
        || manifest["version"] != candidate.version
        || manifest["target"] != TARGET
        || candidate.msi.name != distribution.asset_name(&candidate.version)
        || !checked
    {
        return Err(
            "Release manifest does not identify the selected checked Windows ARM64 application."
                .into(),
        );
    }
    let source = text(&manifest, "sourceRevision")?;
    if !crate::identity::is_lower_hex(source, 40) {
        return Err("Release source revision is invalid.".into());
    }
    let assets: Vec<_> = manifest["assets"]
        .as_array()
        .ok_or("Release manifest assets are missing.")?
        .iter()
        .filter(|asset| asset["name"] == candidate.msi.name)
        .collect();
    let [entry] = assets.as_slice() else {
        return Err("Manifest application asset is missing or repeated.".into());
    };
    if entry["bytes"] != candidate.msi.bytes
        || entry["sha256"] != candidate.msi.sha256
        || checksum(checksums, &candidate.msi.name)? != candidate.msi.sha256
    {
        return Err("GitHub, manifest and checksum application identities disagree.".into());
    }
    let installer = Installer {
        schema_version: 1,
        version: candidate.version.clone(),
        source_revision: source.into(),
        target: TARGET.into(),
        asset: candidate.msi.clone(),
    };
    installer.validate(&candidate.version)?;
    Ok(installer)
}

#[cfg(test)]
mod tests;
