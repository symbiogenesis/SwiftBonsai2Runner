//! Complete hosted-only paired comparison, with fixed public listening fixtures.

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[cfg(any(windows, test))]
const MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";
#[cfg(windows)]
const TEXT: &str = "The quick brown fox crossed the quiet garden.";
#[cfg(windows)]
const VOICE: &str = "A calm, clear narrator with a natural speaking pace.";
#[cfg(any(windows, test))]
const MAX_PCM: usize = 5_760_000;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Format {
    #[default]
    Pcm,
    Wav,
}

#[cfg(any(windows, test))]
impl Format {
    const fn name(self) -> &'static str {
        match self {
            Self::Pcm => "pcm",
            Self::Wav => "wav",
        }
    }
    const fn media(self) -> &'static str {
        match self {
            Self::Pcm => "audio/pcm",
            Self::Wav => "audio/wav",
        }
    }
    const fn limit(self) -> usize {
        match self {
            Self::Pcm => MAX_PCM,
            Self::Wav => MAX_PCM + 44,
        }
    }
}

const fn repetitions() -> u8 {
    2
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    not(windows),
    expect(
        dead_code,
        reason = "Decode the same evaluation contract on unsupported platforms before rejecting execution."
    )
)]
pub(super) struct Request {
    repository_root: PathBuf,
    data_directory: PathBuf,
    tts_path: Option<PathBuf>,
    evidence_directory: Option<PathBuf>,
    #[serde(default = "repetitions")]
    repetitions: u8,
    #[serde(default)]
    response_format: Format,
}

pub(super) fn thread_grid(available: usize) -> crate::Result<Vec<usize>> {
    if !(1..=256).contains(&available) {
        return Err("Evaluation CPU count exceeds the supported bound.".into());
    }
    let threads = [
        (available / 2).max(1),
        available.saturating_sub(2).max(1),
        available,
    ];
    let mut unique = Vec::new();
    for count in threads {
        if !unique.contains(&count) {
            unique.push(count);
        }
    }
    Ok(unique)
}

#[cfg(any(windows, test))]
fn wave(format: Format, bytes: &[u8]) -> crate::Result<Vec<u8>> {
    let wave = match format {
        Format::Pcm => {
            if bytes.is_empty() || bytes.len() > MAX_PCM || !bytes.len().is_multiple_of(2) {
                return Err("TTS returned empty, oversized or unaligned PCM.".into());
            }
            let header = bonsai_audio::mono_pcm16_header(24_000, bytes.len() / 2)
                .ok_or("Invalid evaluation PCM extent.")?;
            let mut wave = Vec::with_capacity(bytes.len() + 44);
            wave.extend_from_slice(&header);
            wave.extend_from_slice(bytes);
            wave
        }
        Format::Wav => bytes.to_vec(),
    };
    bonsai_audio::mono_pcm16_data(&wave, 24_000, 2_880_000)
        .ok_or("Evaluation output was not a complete bounded mono 24 kHz PCM16 WAV.")?;
    Ok(wave)
}

#[cfg(any(windows, test))]
fn ready(value: &Value) -> bool {
    value["ready"].as_bool() == Some(true)
        && value["consistent_voice"].as_bool() == Some(true)
        && value["model"].as_str() == Some(MODEL)
}

#[cfg(any(windows, test))]
fn comparisons(
    sessions: &[Value],
    threads: &[usize],
    repetitions: u8,
) -> crate::Result<Vec<Value>> {
    let mut output = Vec::new();
    for count in threads {
        for repetition in 1..=repetitions {
            let pair: Vec<_> = sessions
                .iter()
                .filter(|session| {
                    session["Threads"] == *count && session["Repetition"] == repetition
                })
                .collect();
            if pair.len() != 2
                || pair.iter().any(|session| {
                    session["Status"] != "completed" || session["Memory"]["Status"] != "completed"
                })
            {
                return Err("Only complete paired TTS sessions can enter comparisons.".into());
            }
            let arm = |name: &str| {
                pair.iter()
                    .find(|session| session["Quantization"] == name)
                    .ok_or("Missing distinct paired TTS quantization.")
            };
            let q8 = arm("Q8_0")?;
            let q4 = arm("Q4_K_M")?;
            let difference = |field: &str| -> crate::Result<i64> {
                let counter = |session: &Value| {
                    session["Memory"][field]
                        .as_u64()
                        .and_then(|value| i64::try_from(value).ok())
                        .ok_or("Invalid TTS memory counter.")
                };
                counter(q8)?
                    .checked_sub(counter(q4)?)
                    .ok_or_else(|| "TTS counter difference overflowed.".into())
            };
            output.push(serde_json::json!({"Threads": count, "Repetition": repetition,
                "Q8MinusQ4PeakWorkingSetBytes": difference("PeakWorkingSetBytes")?,
                "Q8MinusQ4PeakJobMemoryBytes": difference("PeakJobMemoryBytes")?,
                "Definition": "Paired process-lifetime counter differences; positive means lower Q4 usage. Not isolated allocation or quality evidence."}));
        }
    }
    Ok(output)
}

pub(super) fn run(request: &Request) -> crate::Result<Value> {
    #[cfg(windows)]
    {
        windows::run(request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("TTS evaluation requires native Windows ARM64 hosted main CI.".into())
    }
}
