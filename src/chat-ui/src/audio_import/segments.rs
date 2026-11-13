//! Pause-aware PCM framing and capacity planning, independent of Windows decoding.
#![forbid(unsafe_code)]

use std::time::Duration;

pub(super) const CODEC_FAILURE: &str =
    "Could not read this audio file with the installed Windows codecs. Try a WAV or MP3 file.";
pub(super) const CHUNK_TOO_LONG: &str = "The Windows decoder returned an invalid audio segment. Transcription stopped. Your draft was kept.";
pub(super) const MEMORY_FAILURE: &str = "There is not enough available memory to decode the next audio segment. Close other apps and try again. Your draft was kept.";
pub(super) const MAX_SAMPLES: usize = 16_000 * 30;
const MIN_PAUSE_SAMPLES: usize = 16_000 * 25;
pub(super) const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_SAMPLE: i64 = TICKS_PER_SECOND / 16_000;
#[cfg(test)]
const CHUNK_TICKS: i64 = 30 * TICKS_PER_SECOND;

pub(super) fn chunk_range(start: i64, total: i64, retained_samples: usize) -> Option<(i64, i64)> {
    if start < 0 || start >= total {
        return None;
    }
    let capacity = MAX_SAMPLES.checked_sub(retained_samples)?;
    if capacity == 0 {
        return None;
    }
    let ticks = i64::try_from(capacity).ok()?.checked_mul(TICKS_PER_SAMPLE)?;
    let maximum = start.saturating_add(ticks);
    Some((start, if maximum < total { maximum } else { total }))
}

pub(super) fn pause_segment(
    pending: &mut Vec<u8>,
    wave: &[u8],
    last: bool,
) -> Result<Vec<u8>, &'static str> {
    let maximum = u32::try_from(MAX_SAMPLES).map_err(|_| CHUNK_TOO_LONG)?;
    let data = bonsai_audio::mono_pcm16_data(wave, 16_000, maximum).ok_or(CODEC_FAILURE)?;
    let length = pending.len().checked_add(data.len()).ok_or(CHUNK_TOO_LONG)?;
    if !pending.len().is_multiple_of(2) || length / 2 > MAX_SAMPLES {
        return Err(CHUNK_TOO_LONG);
    }
    pending
        .try_reserve_exact(data.len())
        .map_err(|_| MEMORY_FAILURE)?;
    pending.extend_from_slice(data);
    let samples = length / 2;
    let boundary = if last {
        samples
    } else {
        bonsai_audio::pcm16_pause_boundary(pending, 16_000, MIN_PAUSE_SAMPLES.min(samples))
            .ok_or(CODEC_FAILURE)?
    };
    let bytes = boundary * 2;
    let header = bonsai_audio::mono_pcm16_header(16_000, boundary).ok_or(CODEC_FAILURE)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes + 44)
        .map_err(|_| MEMORY_FAILURE)?;
    result.extend_from_slice(&header);
    result.extend_from_slice(&pending[..bytes]);
    pending.copy_within(bytes.., 0);
    pending.truncate(length - bytes);
    Ok(result)
}

pub(super) fn sample_duration(samples: u64) -> Result<Duration, &'static str> {
    let seconds = samples / 16_000;
    let nanos = u32::try_from((samples % 16_000) * 62_500).map_err(|_| CHUNK_TOO_LONG)?;
    Ok(Duration::new(seconds, nanos))
}

#[cfg(test)]
mod tests {
    use super::{
        CHUNK_TICKS, CHUNK_TOO_LONG, MAX_SAMPLES, TICKS_PER_SECOND, chunk_range, pause_segment,
        sample_duration,
    };
    use std::{io, time::Duration};

    #[test]
    fn segment_ranges_cover_long_recordings_and_keep_the_final_partial_segment() {
        let total = 12 * 60 * 60 * TICKS_PER_SECOND + 17;
        let mut offset = 0;
        let mut count = 0;
        while let Some((start, end)) = chunk_range(offset, total, 0) {
            assert_eq!(start, offset);
            assert!(end > start && end - start <= CHUNK_TICKS);
            offset = end;
            count += 1;
        }
        assert_eq!(offset, total);
        assert_eq!(count, 1441);
        assert_eq!(chunk_range(-1, total, 0), None);
        assert_eq!(chunk_range(0, 0, 0), None);
        assert_eq!(chunk_range(0, total, MAX_SAMPLES), None);
        assert_eq!(chunk_range(0, total, MAX_SAMPLES + 1), None);
        assert_eq!(
            chunk_range(CHUNK_TICKS, total, 16_000 * 5),
            Some((CHUNK_TICKS, 55 * TICKS_PER_SECOND))
        );
        assert_eq!(
            chunk_range(i64::MAX - 17, i64::MAX, 0),
            Some((i64::MAX - 17, i64::MAX))
        );
        assert_eq!(sample_duration(16_001), Ok(Duration::new(1, 62_500)));
    }

    fn pcm_wave(data: &[u8]) -> io::Result<Vec<u8>> {
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, data.len() / 2)
            .ok_or_else(|| io::Error::other("Invalid PCM fixture"))?
            .to_vec();
        wave.extend_from_slice(data);
        Ok(wave)
    }

    #[test]
    fn pause_segments_preserve_samples_across_cuts_and_the_final_tail() -> io::Result<()> {
        // Distinct changing sample bytes make a dropped, duplicated or reordered
        // suffix observable independently of the segmenter's chosen boundaries.
        let count = MAX_SAMPLES * 3 + 17;
        let mut original = Vec::with_capacity(count * 2);
        for index in 0..count {
            let sample = if (16_000 * 27..16_000 * 28).contains(&index)
                || (16_000 * 54..16_000 * 55).contains(&index)
            {
                0
            } else {
                1_000_i16 + i16::try_from(index % 20_000).unwrap_or_default()
            };
            original.extend_from_slice(&sample.to_le_bytes());
        }
        let mut pending = Vec::new();
        let mut emitted = Vec::new();
        let mut decoded = 0;
        let mut lengths = Vec::new();
        while decoded < original.len() {
            let capacity = MAX_SAMPLES * 2 - pending.len();
            let end = (decoded + capacity).min(original.len());
            let wave = pcm_wave(&original[decoded..end])?;
            let segment =
                pause_segment(&mut pending, &wave, end == original.len()).map_err(io::Error::other)?;
            let data = bonsai_audio::mono_pcm16_data(&segment, 16_000, 480_000)
                .ok_or_else(|| io::Error::other("Invalid segment"))?;
            lengths.push(data.len() / 2);
            emitted.extend_from_slice(data);
            decoded = end;
        }
        assert!((16_000 * 27..16_000 * 28).contains(&lengths[0]));
        assert!(lengths.iter().all(|length| *length <= MAX_SAMPLES));
        assert!(pending.is_empty());
        assert_eq!(emitted, original);
        Ok(())
    }

    #[test]
    fn pause_segments_keep_continuous_speech_and_short_final_audio() -> io::Result<()> {
        let mut pending = Vec::new();
        let original = i16::MIN.to_le_bytes().repeat(MAX_SAMPLES);
        let wave = pcm_wave(&original)?;
        let segment = pause_segment(&mut pending, &wave, false).map_err(io::Error::other)?;
        assert_eq!(segment, wave);
        assert!(pending.is_empty());
        pending.extend_from_slice(&[1, 2, 3, 4]);
        let tail = pcm_wave(&[5, 6])?;
        let segment = pause_segment(&mut pending, &tail, true).map_err(io::Error::other)?;
        assert_eq!(
            bonsai_audio::mono_pcm16_data(&segment, 16_000, 480_000),
            Some(&[1, 2, 3, 4, 5, 6][..])
        );
        assert!(pending.is_empty());
        Ok(())
    }

    #[test]
    fn pause_segments_reject_corrupt_or_overlength_pcm_without_dropping_tail() -> io::Result<()> {
        let mut pending = vec![1, 2];
        assert_eq!(
            pause_segment(&mut pending, &[], false),
            Err(super::CODEC_FAILURE)
        );
        let wave = pcm_wave(&vec![0; MAX_SAMPLES * 2])?;
        assert_eq!(
            pause_segment(&mut pending, &wave, false),
            Err(CHUNK_TOO_LONG)
        );
        assert_eq!(pending, vec![1, 2]);
        pending.push(3);
        let wave = pcm_wave(&[4, 5])?;
        assert_eq!(
            pause_segment(&mut pending, &wave, true),
            Err(CHUNK_TOO_LONG)
        );
        assert_eq!(pending, vec![1, 2, 3]);
        Ok(())
    }
}
