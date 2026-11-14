//! Complete SDK receipt policy, operating only on bounded original archive bytes.

use crate::{functional, hex, limit, recipe, scan_entries};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn pe(bytes: &[u8]) -> Value {
    if bytes.len() < 64 || &bytes[..2] != b"MZ" {
        return json!({"validPeHeader":false});
    }
    let offset = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
    let Ok(offset) = usize::try_from(offset) else {
        return json!({"validPeHeader":false});
    };
    if offset < 64
        || offset.checked_add(24).is_none_or(|end| end > bytes.len())
        || &bytes[offset..offset + 4] != b"PE\0\0"
    {
        return json!({"validPeHeader":false});
    }
    let machine = u16::from_le_bytes([bytes[offset + 4], bytes[offset + 5]]);
    json!({"validPeHeader":true,"machine":machine,"machineHex":format!("{machine:04x}")})
}

pub(crate) fn notice(bytes: &[u8]) -> Value {
    let (text, observation) = if pe(bytes)["validPeHeader"] == true {
        (
            None,
            json!({"encoding":null,"basis":"validated-PE-header; text-decoding-not-attempted","uncertain":false}),
        )
    } else {
        functional::decode(bytes)
    };
    let lower = text.as_ref().map_or_else(
        || {
            bytes
                .iter()
                .map(|b| char::from(*b))
                .collect::<String>()
                .to_lowercase()
        },
        |text| text.to_lowercase(),
    );
    let markers: Vec<_> = [
        ("copyright", "copyright"),
        ("all-rights-reserved", "all rights reserved"),
        ("qualcomm-name", "qualcomm"),
        ("confidential-marker", "confidential"),
        ("proprietary-marker", "proprietary"),
        ("redistribution-word", "redistribution"),
        ("distribution-word", "distribution"),
        ("license-word", "license"),
        ("agreement-word", "agreement"),
        ("terms-word", "terms"),
        ("bsd-name", "bsd"),
        ("apache-name", "apache"),
        ("gpl-name", "general public license"),
    ]
    .into_iter()
    .filter(|(_, marker)| lower.contains(marker))
    .map(|(name, _)| name)
    .collect();
    let mut result = json!({"markersPresent":markers,"licenseRightsInterpretationReviewed":false,"encodingObservation":observation});
    if let Some(text) = text {
        let mut identifiers = BTreeSet::new();
        for (index, _) in text.match_indices("SPDX-License-Identifier:") {
            let rest = text[index + "SPDX-License-Identifier:".len()..].trim_start();
            let identifier: String = rest
                .chars()
                .take_while(|ch| !['\r', '\n', '*'].contains(ch))
                .take(128)
                .collect();
            if !identifier.is_empty() {
                identifiers.insert(identifier.trim().to_owned());
            }
        }
        result["spdxIdentifiersMentioned"] =
            json!(identifiers.into_iter().take(16).collect::<Vec<_>>());
        result["copyrightMarkerLines"] = json!(
            text.lines()
                .enumerate()
                .filter(|(_, line)| line.to_lowercase().contains("copyright"))
                .take(32)
                .map(|(index, _)| index + 1)
                .collect::<Vec<_>>()
        );
        result["classificationScope"] = json!("text-markers-only-not-a-license-grant");
    } else {
        result["classificationScope"] = json!(
            "raw-byte-markers-only-not-a-license-grant; no-binary-classification-from-decode-failure"
        );
    }
    result
}

fn legal_terms(text: &str) -> BTreeMap<&'static str, Vec<usize>> {
    let words: Vec<_> = text
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .collect();
    let mut found: BTreeMap<_, Vec<usize>> = [
        "qaic-name",
        "IDL-compiler-phrase",
        "SDK-phrase",
        "license-agreement-phrase",
        "product-license-phrase",
        "redistribution-word",
        "patent-word",
        "confidential-word",
        "proprietary-word",
        "GHC-Haskell-name",
    ]
    .map(|name| (name, Vec::new()))
    .into();
    // Byte positions allow bounded line metadata without retaining full legal text.
    let mut offset = 0;
    let mut positions = Vec::new();
    for word in &words {
        let start = offset + text[offset..].find(word).unwrap_or(0);
        positions.push(start);
        offset = start + word.len();
    }
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        let next = words.get(index + 1).map(|word| word.to_lowercase());
        let term = match lower.as_str() {
            "qaic" => Some("qaic-name"),
            "sdk" => Some("SDK-phrase"),
            "redistribution" => Some("redistribution-word"),
            "patent" => Some("patent-word"),
            "confidential" => Some("confidential-word"),
            "proprietary" => Some("proprietary-word"),
            "ghc" | "haskell" => Some("GHC-Haskell-name"),
            "idl" if next.as_deref() == Some("compiler") => Some("IDL-compiler-phrase"),
            "license" | "licence" if next.as_deref() == Some("agreement") => {
                Some("license-agreement-phrase")
            }
            "product" if matches!(next.as_deref(), Some("license" | "licence")) => {
                Some("product-license-phrase")
            }
            _ => None,
        };
        if let Some(term) = term {
            found
                .get_mut(term)
                .expect("fixed term key")
                .push(positions[index]);
        }
    }
    found
}

pub(crate) fn legal(bytes: &[u8], words: usize) -> Value {
    let (text, observation) = functional::decode(bytes);
    let mut result = json!({"encodingObservation":observation,"sdkToolTermsQualified":false,"generatedOutputRightsQualified":false,"keywordMatchesEstablishGrant":false,"absenceOfTermsEstablished":false});
    let Some(text) = text else {
        result["status"] = json!("pending-text-decoding");
        return result;
    };
    let controls = text
        .chars()
        .filter(|ch| *ch < ' ' && !['\r', '\n', '\t', '\u{c}'].contains(ch))
        .count();
    result["nonTextControlCharacters"] = json!(controls);
    if controls > 2.max(text.encode_utf16().count() / 100) {
        result["status"] = json!("pending-text-plausibility; no-binary-classification-inferred");
        return result;
    }
    let terms = legal_terms(&text);
    let mut occurrences = serde_json::Map::new();
    for (name, matches) in &terms {
        occurrences.insert((*name).to_owned(), json!({"count":matches.len(),"firstLines":matches.iter().take(8).map(|index| text[..*index].matches('\n').count()+1).collect::<Vec<_>>()}));
    }
    result["termOccurrences"] = Value::Object(occurrences);
    let focus = terms["qaic-name"].first().copied().or_else(|| {
        terms["license-agreement-phrase"]
            .iter()
            .chain(&terms["product-license-phrase"])
            .min()
            .copied()
    });
    let (excerpt, line) = if let Some(index) = focus {
        let before = text[..index]
            .split_whitespace()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        (
            before
                .into_iter()
                .chain(text[index..].split_whitespace())
                .take(words)
                .collect::<Vec<_>>()
                .join(" "),
            text[..index].matches('\n').count() + 1,
        )
    } else {
        (
            text.split_whitespace()
                .take(words)
                .collect::<Vec<_>>()
                .join(" "),
            1,
        )
    };
    result["necessaryReviewExcerpt"] = json!({"line":line,"wordsMaximum":words,"text":excerpt});
    result["status"] = json!("decoded-data-only-review; applicability-and-grant-unresolved");
    result
}

fn review(bytes: &[u8], kind: &str, words: usize) -> Option<Value> {
    if kind == "license-review" {
        return Some(legal(bytes, words));
    }
    if kind != "build-idl-recipe" {
        return None;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Some(json!({"textEncoding":"not-UTF8","excerpt":null}));
    };
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<_> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains("qaic"))
        .collect();
    let excerpt = lines
        .iter()
        .flat_map(|(_, line)| line.split_whitespace())
        .take(words)
        .collect::<Vec<_>>()
        .join(" ");
    let mut flags = BTreeSet::new();
    for (index, _) in text.match_indices('-') {
        if index > 0 && text.as_bytes()[index - 1].is_ascii_alphanumeric()
            || !text
                .as_bytes()
                .get(index + 1)
                .is_some_and(u8::is_ascii_alphabetic)
        {
            continue;
        }
        let end = 1 + text[index + 1..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
            .count();
        flags.insert(&text[index..index + end]);
    }
    Some(
        json!({"textEncoding":"UTF8","qaicReferenceLines":lines.iter().take(64).map(|(index, _)| index+1).collect::<Vec<_>>(),"qaicReferenceExcerpt":excerpt,"flagSpellingsMentionedInRecipe":flags.into_iter().take(128).collect::<Vec<_>>(),"commandAndImportInterpretationReviewed":false}),
    )
}

fn check_identity(bytes: &[u8], size: &Value, digest: &Value) -> Result<()> {
    if size.as_u64() != Some(u64::try_from(bytes.len())?)
        || digest.as_str() != Some(hex(&Sha256::digest(bytes)).as_str())
    {
        return Err("SDK input differs from reviewed byte identity".into());
    }
    Ok(())
}

pub(crate) fn collect<R: Read>(
    decoded: R,
    pin: &Value,
    public: &Path,
    deadline: Instant,
) -> Result<Value> {
    if pin["schemaVersion"] != 1
        || pin["generatorExecutionAllowed"] != false
        || pin["deviceExecutionAllowed"] != false
        || pin["profileEligible"] != false
    {
        return Err("SDK inventory pin does not authorize data-only operation".into());
    }
    let (mut selected, mut recipes, mut comparisons) =
        (Vec::new(), BTreeMap::new(), BTreeMap::new());
    let words = usize::try_from(limit(pin, "reviewWordsPerFile")?)?;
    let metrics = scan_entries(decoded, pin, deadline, |mut entry, bytes| {
        if Instant::now() >= deadline {
            return Err("SDK inventory aggregate allowance expired".into());
        }
        let name = entry["path"].as_str().ok_or("missing SDK path")?.to_owned();
        let kind = entry["category"]
            .as_str()
            .ok_or("missing SDK category")?
            .to_owned();
        if Some(name.as_str()) == pin["requiredRecipePath"].as_str() {
            check_identity(
                &bytes,
                &pin["requiredRecipeBytes"],
                &pin["requiredRecipeSha256"],
            )?;
        }
        if Some(name.as_str()) == pin["requiredNotice"]["path"].as_str() {
            check_identity(
                &bytes,
                &pin["requiredNotice"]["bytes"],
                &pin["requiredNotice"]["sha256"],
            )?;
        }
        for comparison in pin["functionalComparisons"]
            .as_array()
            .ok_or("comparison set absent")?
        {
            if comparison["sdkPath"] == name {
                check_identity(&bytes, &comparison["sdkBytes"], &comparison["sdkSha256"])?;
                comparisons.insert(name.clone(), bytes.clone());
            }
        }
        entry["noticeClassification"] = notice(&bytes);
        if ["sdk-idl-input-candidate", "sdk-rpc-header-candidate"].contains(&kind.as_str()) {
            entry["functionalMetadata"] = functional::metadata(&bytes, pin)?;
        }
        if kind == "qaic-generator" {
            entry["peHeader"] = pe(&bytes);
        }
        if let Some(review) = review(&bytes, &kind, words) {
            entry["review"] = review;
        }
        if ["build-idl-recipe", "build-recipe-helper"].contains(&kind.as_str()) {
            recipes.insert(name, bytes);
        }
        selected.push(entry);
        Ok(())
    })?;
    for required in [
        &pin["intendedQaicPath"],
        &pin["requiredRecipePath"],
        &pin["requiredNotice"]["path"],
    ] {
        if selected
            .iter()
            .filter(|entry| entry["path"] == *required)
            .count()
            != 1
        {
            return Err("required SDK input absent from checked archive".into());
        }
    }
    let recipe = recipe::metadata(&recipes, pin).unwrap_or_else(|error| json!({"status":"pending-functional-token-review","reason":error.to_string(),"variableExpansionExecuted":false,"generatorCommandAuthorized":false}));
    let mut compared = Vec::new();
    for comparison in pin["functionalComparisons"]
        .as_array()
        .ok_or("comparison set absent")?
    {
        if Instant::now() >= deadline {
            return Err("SDK inventory aggregate allowance expired".into());
        }
        let sdk_path = comparison["sdkPath"]
            .as_str()
            .ok_or("SDK comparison path absent")?;
        let name = comparison["publicName"]
            .as_str()
            .ok_or("public comparison name absent")?;
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err("public comparison name escaped namespace".into());
        }
        let path = public.join(name);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err("public comparison input cannot redirect".into());
        }
        let size = comparison["publicBytes"]
            .as_u64()
            .filter(|size| *size <= 1024 * 1024)
            .ok_or("public comparison extent invalid")?;
        let mut original = Vec::new();
        File::open(&path)?
            .take(size + 1)
            .read_to_end(&mut original)?;
        check_identity(
            &original,
            &comparison["publicBytes"],
            &comparison["publicSha256"],
        )?;
        let sdk = comparisons
            .get(sdk_path)
            .ok_or("SDK/public comparison omitted fixed input")?;
        compared.push(json!({"sdkPath":sdk_path,"publicName":name,"sdkRawSha256":hex(&Sha256::digest(sdk)),"publicRawSha256":hex(&Sha256::digest(&original)),"comparison":functional::compare(sdk, &original, pin)?}));
    }
    selected.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(
        json!({"schemaVersion":1,"kind":"hexagon-SDK-data-only-inventory","archive":pin["asset"],"archiveEntriesRead":metrics["archiveEntriesRead"],"declaredUncompressedBytes":metrics["declaredUncompressedBytes"],"linkEntriesNotExtracted":metrics["linkEntriesNotExtracted"],"selectedBytesRead":metrics["selectedBytesRead"],"selectedFiles":selected,"buildIdlRecipe":recipe,"sdkPublicFunctionalComparisons":compared,"functionalComparisonScope":"unpreprocessed-lexical-token-and-declaration-candidates-only; no-ABI-layout-or-runtime-equivalence","WindowsExtensionDeclarationsQualified":false,"licenseFilenameSearch":{"scope":"all-regular-archive-entry-leaf-names-without-depth-or-prefix-restrictions","substringIndicators":["license","licence","eula","notice","copyright","terms","agreement","copying"],"matchingNamesEstablishLicenseAbsenceOrRights":false},"sdkContentsExtracted":false,"sdkCodeExecuted":false,"generatorExecuted":false,"generatedSourceUploaded":false,"licenseReview":"pending; bounded necessary review excerpts and file identities only","generatorInvocationReviewed":false,"deviceCodeLoaded":false,"profileEligible":false}),
    )
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn write(output: &Path, mut receipt: Value, pin_path: &Path, pin: &Value) -> Result<()> {
    let source = |name: &str| hex(&Sha256::digest(name.replace("\r\n", "\n").as_bytes()));
    receipt["inventorySourceSha256"] = json!(source(include_str!("inventory.rs")));
    receipt["functionalReviewSourceSha256"] = json!(source(include_str!("functional.rs")));
    receipt["recipeReviewSourceSha256"] = json!(source(include_str!("recipe.rs")));
    receipt["inventoryPinSha256"] = json!(hex(&Sha256::digest(fs::read(pin_path)?)));
    let executable = std::env::current_exe()?;
    let mut hash = Sha256::new();
    let mut file = File::open(&executable)?;
    let mut buffer = [0_u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    receipt["archiveReader"] = json!({"name":executable.file_name().and_then(|s| s.to_str()).ok_or("invalid executable name")?,"bytes":file.metadata()?.len(),"sha256":hex(&hash.finalize())});
    let mut encoded = functional::ascii_json(&receipt)?;
    encoded.push(b'\n');
    if u64::try_from(encoded.len())? > limit(pin, "receiptBytes")? {
        return Err("SDK inventory receipt exceeds byte bound".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    Ok(())
}
