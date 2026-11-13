//! Allocation-free pause selection over borrowed little-endian mono PCM16.

/// Selects a near-end pause boundary as a count of complete PCM16 samples.
///
/// `minimum_samples` is the earliest permitted cut; the payload length is the
/// latest. Returns `None` for empty/partial PCM, a zero rate or a zero minimum.
/// A payload no longer than the minimum, or too short for analysis, stays whole.
/// The caller retains both sides of the cut without dropping or repeating audio
/// and separately enforces its recognition request's maximum duration.
///
/// The heuristic searches 100 ms energy windows at 10 ms intervals, including
/// the final window. A pause must have RMS at most 256 PCM units and at most a
/// quarter of the loudest searched window's RMS. The quietest window wins, with
/// later ties preferred; its midpoint is the cut. If the final window is equally
/// quiet, the payload stays whole. Constant-energy audio also stays whole.
/// Window sizes have a two-sample minimum for unusually small sample rates.
/// This is a deterministic energy heuristic, not a speech/accuracy guarantee.
#[must_use]
pub fn pcm16_pause_boundary(
    data: &[u8],
    sample_rate: u32,
    minimum_samples: usize,
) -> Option<usize> {
    if data.is_empty() || !data.len().is_multiple_of(2) || sample_rate == 0 || minimum_samples == 0
    {
        return None;
    }
    let samples = data.len() / 2;
    let rate = usize::try_from(sample_rate).ok()?;
    let window = (rate / 10).max(2);
    if samples <= minimum_samples || samples < window {
        return Some(samples);
    }
    let half = window / 2;
    let first = minimum_samples.saturating_sub(half);
    if first > samples - window {
        return Some(samples);
    }
    let step = (rate / 100).max(1);
    // A u32 rate bounds a 100 ms window to fewer than 430 million samples.
    // Even i16::MIN squared across that window fits comfortably in u64.
    let mut energy: u64 = (first..first + window)
        .map(|index| sample_energy(data, index))
        .sum();
    let mut quietest = energy;
    let mut loudest = energy;
    let mut boundary = first + half;
    for index in first + window..samples {
        energy -= sample_energy(data, index - window);
        energy += sample_energy(data, index);
        let start = index + 1 - window;
        if (start - first).is_multiple_of(step) || index + 1 == samples {
            loudest = loudest.max(energy);
            if energy <= quietest {
                quietest = energy;
                boundary = start + half;
            }
        }
    }
    let absolute_limit = u64::try_from(window).ok()?.checked_mul(256 * 256)?;
    if quietest == energy
        || quietest > absolute_limit
        || loudest == 0
        || quietest.saturating_mul(16) > loudest
    {
        Some(samples)
    } else {
        Some(boundary)
    }
}

fn sample_energy(data: &[u8], index: usize) -> u64 {
    let offset = index * 2;
    let sample = i16::from_le_bytes([data[offset], data[offset + 1]]);
    u64::from(sample.unsigned_abs()).pow(2)
}

#[cfg(test)]
mod tests {
    use super::pcm16_pause_boundary;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn pause_boundary_finds_a_quiet_interval_without_changing_either_side() {
        let mut samples = vec![4_000_i16; 480_000];
        samples[432_000..436_800].fill(0); // 27.0 through 27.3 seconds.
        let data = pcm(&samples);
        let boundary = pcm16_pause_boundary(&data, 16_000, 400_000).unwrap_or_default();
        // The cut is protected by at least half an analysis window on each side.
        assert!((432_800..=436_000).contains(&boundary));
        assert_eq!(
            data[..boundary * 2].len() + data[boundary * 2..].len(),
            data.len()
        );
        assert_eq!(pcm(&samples[..boundary]), data[..boundary * 2]);
        assert_eq!(pcm(&samples[boundary..]), data[boundary * 2..]);
    }

    #[test]
    fn pause_boundary_prefers_later_equal_pauses_but_keeps_a_quiet_tail_whole() {
        let mut samples = vec![2_000_i16; 30_000];
        samples[26_000..26_300].fill(0);
        samples[28_000..28_300].fill(0);
        let boundary = pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000).unwrap_or_default();
        assert!((28_050..=28_250).contains(&boundary));
        assert_eq!(
            pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000),
            Some(boundary)
        );
        samples[29_500..].fill(0);
        assert_eq!(
            pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000),
            Some(30_000)
        );
    }

    #[test]
    fn pause_boundary_near_the_limit_remains_within_the_quiet_interval() {
        let mut samples = vec![i16::MIN; 30_000];
        samples[29_600..29_900].fill(20);
        let boundary = pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000).unwrap_or_default();
        assert!((29_650..=29_850).contains(&boundary));
        assert!(boundary <= 30_000);
    }

    #[test]
    fn pause_boundary_preserves_continuous_silent_and_full_scale_audio() {
        for level in [0, 5, 256, 500, 8_000, i16::MIN, i16::MAX] {
            let data = pcm(&vec![level; 30_000]);
            assert_eq!(pcm16_pause_boundary(&data, 1_000, 25_000), Some(30_000));
        }
        // A relative dip alone is insufficient if it is still well above silence.
        let mut samples = vec![12_000_i16; 30_000];
        samples[27_000..27_500].fill(2_000);
        assert_eq!(
            pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000),
            Some(30_000)
        );
        // A small amplitude change alone is insufficient to establish a pause.
        samples.fill(200);
        samples[27_000..27_500].fill(100);
        assert_eq!(
            pcm16_pause_boundary(&pcm(&samples), 1_000, 25_000),
            Some(30_000)
        );
    }

    #[test]
    fn pause_boundary_keeps_short_final_and_narrow_ranges_whole() {
        let data = pcm(&[1, 0, 0, 1]);
        for minimum in [4, 5, usize::MAX] {
            assert_eq!(pcm16_pause_boundary(&data, 16_000, minimum), Some(4));
        }
        assert_eq!(pcm16_pause_boundary(&data, 16_000, 1), Some(4));
        assert_eq!(pcm16_pause_boundary(&data, 10, 3), Some(4));
        assert_eq!(pcm16_pause_boundary(&pcm(&[0]), 1, 1), Some(1));
        assert_eq!(pcm16_pause_boundary(&pcm(&[0, 0]), 1, 1), Some(2));
        assert_eq!(pcm16_pause_boundary(&data, u32::MAX, 1), Some(4));
    }

    #[test]
    fn pause_boundary_rejects_malformed_payload_and_zero_parameters() {
        assert_eq!(pcm16_pause_boundary(&[], 16_000, 1), None);
        assert_eq!(pcm16_pause_boundary(&[0], 16_000, 1), None);
        assert_eq!(pcm16_pause_boundary(&[0, 0, 0], 16_000, 1), None);
        assert_eq!(pcm16_pause_boundary(&[0, 0], 0, 1), None);
        assert_eq!(pcm16_pause_boundary(&[0, 0], 16_000, 0), None);
    }
}
