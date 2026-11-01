//! Native command input, including direct chat options and bounded JSON streams.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
// Legacy scoring permits 24 responses of 1 Mi UTF-16 units, including JSON escapes.
const MAX_QUALIFICATION_REQUEST_BYTES: u64 = 160 * 1024 * 1024;

fn decode(reader: impl Read, maximum: u64) -> crate::Result<Value> {
    let mut bytes = Vec::new();
    reader
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err(format!("Command request exceeds {maximum} bytes."));
    }
    let request: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !request.is_object() {
        return Err("Command request must be a JSON object.".into());
    }
    Ok(request)
}

fn file_request(path: &Path, maximum: u64) -> crate::Result<Value> {
    if !path.is_absolute() {
        return Err("Request path must be expanded and absolute.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > maximum {
        return Err(format!("Command request exceeds {maximum} bytes."));
    }
    decode(file, maximum)
}

/// Reads one JSON object from an absolute file, inline JSON, or standard input.
///
/// # Errors
/// Rejects unknown options, trailing arguments, non-object JSON and oversized input.
pub fn request(command: &str, arguments: &[OsString]) -> crate::Result<Value> {
    let maximum = if command == "qualification" {
        MAX_QUALIFICATION_REQUEST_BYTES
    } else {
        MAX_REQUEST_BYTES
    };
    match arguments {
        [option, source] if option == "--request" && source == "-" => {
            decode(std::io::stdin().lock(), maximum)
        }
        [option, source] if option == "--request" => file_request(Path::new(source), maximum),
        [option, source] if option == "--json" => decode(
            source
                .to_str()
                .ok_or("Inline JSON must be Unicode.")?
                .as_bytes(),
            maximum,
        ),
        _ => Err("Use --request ABSOLUTE_JSON_PATH, --request -, or --json JSON_OBJECT.".into()),
    }
}

fn repository() -> crate::Result<PathBuf> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    current
        .ancestors()
        .find(|path| {
            path.join("tools/bonsai/Cargo.toml").is_file()
                && path.join("config/prism-inference.lock.json").is_file()
        })
        .map(Path::to_owned)
        .ok_or_else(|| "Run chat from a Runner checkout or supply --repository-root PATH.".into())
}

fn chat_options(arguments: &[OsString]) -> crate::Result<Map<String, Value>> {
    let mut request = Map::new();
    let mut values = arguments.iter();
    while let Some(option) = values.next() {
        let option = option.to_str().ok_or("Chat options must be Unicode.")?;
        let (key, boolean, numeric) = match option {
            "--data-directory" => ("DataDirectory", false, false),
            "--repository-root" => ("RepositoryRoot", false, false),
            "--working-directory" => ("WorkingDirectory", false, false),
            "--chat-path" => ("ChatPath", false, false),
            "--broker-path" => ("BrokerPath", false, false),
            "--asr-path" => ("AsrPath", false, false),
            "--tts-path" => ("TtsPath", false, false),
            "--tts-quantization" => ("TtsQuantization", false, false),
            "--tts-threads" => ("TtsThreads", false, true),
            "--native-row-workers" => ("NativeRowWorkers", false, true),
            "--frontend-port" => ("FrontendPort", false, true),
            "--backend-port" => ("BackendPort", false, true),
            "--canary-port" => ("CanaryPort", false, true),
            "--tts-port" => ("TtsPort", false, true),
            "--canary-key-file" => ("CanaryKeyFile", false, false),
            "--tts-key-file" => ("TtsKeyFile", false, false),
            "--native-engine" => ("NativeEngine", true, false),
            "--use-repacked-ptq1" => ("UseRepackedPtq1", true, false),
            "--use-original-pq2" => ("UseRepackedPtq1", true, false),
            "--no-abliterate-lora" => ("NoAbliterateLora", true, false),
            "--no-tts" => ("NoTts", true, false),
            "--no-speech-models" => ("NoSpeechModels", true, false),
            "--speech-models-only" => ("SpeechModelsOnly", true, false),
            "--force-build" => ("ForceBuild", true, false),
            _ => return Err(format!("Unknown chat option: {option}")),
        };
        let value = if boolean {
            Value::Bool(option != "--use-original-pq2")
        } else {
            let text = values
                .next()
                .and_then(|value| value.to_str())
                .ok_or_else(|| format!("Missing Unicode value for {option}."))?;
            if numeric {
                Value::from(
                    text.parse::<u64>()
                        .map_err(|_| format!("{option} requires an unsigned integer."))?,
                )
            } else {
                Value::from(text)
            }
        };
        if request.insert(key.into(), value).is_some() {
            return Err(format!("Repeated or conflicting chat option: {option}"));
        }
    }
    Ok(request)
}

/// Builds the native launch request from human-facing command-line options.
///
/// # Errors
/// Rejects unknown, repeated or incomplete options and unavailable default paths.
/// Launch policy validates the complete request before starting an application.
pub fn chat(arguments: &[OsString]) -> crate::Result<Value> {
    let mut request = chat_options(arguments)?;
    if !request.contains_key("RepositoryRoot") {
        request.insert(
            "RepositoryRoot".into(),
            serde_json::to_value(repository()?).map_err(|error| error.to_string())?,
        );
    }
    if !request.contains_key("DataDirectory") {
        let data = PathBuf::from(
            std::env::var_os("LOCALAPPDATA")
                .ok_or("Supply --data-directory or set LOCALAPPDATA.")?,
        )
        .join("SwiftBonsai2");
        request.insert(
            "DataDirectory".into(),
            serde_json::to_value(data).map_err(|error| error.to_string())?,
        );
    }
    Ok(Value::Object(request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_input_rejects_nonobjects_truncation_and_overflow() -> crate::Result<()> {
        assert_eq!(decode(&b"{\"a\":1}"[..], 7)?["a"], 1);
        for bytes in [&b"[]"[..], b"null", b"{", b"{} trailing", b"{}       "] {
            assert!(decode(bytes, 8).is_err());
        }
        assert!(decode(&b"{}"[..], 1).is_err());
        Ok(())
    }

    #[test]
    fn chat_options_preserve_types_and_reject_ambiguous_input() -> crate::Result<()> {
        let options = chat_options(&[
            "--native-engine".into(),
            "--native-row-workers".into(),
            "4".into(),
            "--data-directory".into(),
            "C:\\Data with spaces".into(),
        ])?;
        assert_eq!(options["NativeEngine"], true);
        assert_eq!(options["NativeRowWorkers"], 4);
        assert_eq!(options["DataDirectory"], "C:\\Data with spaces");
        for arguments in [
            vec!["--no-tts", "--no-tts"],
            vec!["--native-row-workers", "-1"],
            vec!["--data-directory"],
            vec!["--typo"],
            vec!["--no-tts", "false"],
        ] {
            assert!(
                chat_options(&arguments.into_iter().map(Into::into).collect::<Vec<_>>()).is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn weight_options_preserve_explicit_false_and_reject_conflicting_selections()
    -> crate::Result<()> {
        assert_eq!(
            chat_options(&["--use-original-pq2".into()])?["UseRepackedPtq1"],
            false
        );
        assert_eq!(
            chat_options(&["--use-repacked-ptq1".into()])?["UseRepackedPtq1"],
            true
        );
        for first in ["--use-original-pq2", "--use-repacked-ptq1"] {
            for second in ["--use-original-pq2", "--use-repacked-ptq1"] {
                assert!(chat_options(&[first.into(), second.into()]).is_err());
            }
        }
        Ok(())
    }
}
