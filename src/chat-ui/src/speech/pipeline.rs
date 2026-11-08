//! Bounded PCM playback with sequential model requests and audio prefetch.

use super::{Arc, AtomicU64, JoinHandle, Ordering, Receiver, VecDeque, io, mpsc, thread, tts};
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::AtomicBool,
        mpsc::{SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::Media::Audio::{
    HWAVEOUT, WAVEFORMATEX, WAVEHDR, waveOutClose, waveOutOpen, waveOutPause, waveOutPrepareHeader,
    waveOutReset, waveOutRestart, waveOutUnprepareHeader, waveOutWrite,
};

const AUDIO_FAILURE: tts::Failure =
    tts::Failure::Message("Audio playback failed. Check the Windows audio output device.");
const POLL: Duration = Duration::from_millis(5);
/// Default 24 kHz mono output sample rate.
pub(super) const SAMPLE_RATE: u32 = 24_000;
/// Upgraded 48 kHz mono output sample rate (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`).
pub(super) const SAMPLE_RATE_48K: u32 = 48_000;
// Normalize callbacks into 100 ms of mono PCM16 (4,800 bytes at 24 kHz; 9,600 bytes
// at 48 kHz), so queue bounds describe audio duration even when chunks are tiny.
const PACKET_BYTES: usize = 4_800;
pub(super) const PACKET_BYTES_48K: usize = 9_600;
/// Hann-window half-width (in samples) for smoothing 256-character UI sentence boundaries.
pub(super) const HANN_BOUNDARY_SAMPLES: usize = 64;
const PREFETCH_PACKETS: usize = 160; // 16 seconds / 768,000 bytes at 24 kHz, outside WinMM.
const PLAYBACK_PACKETS: usize = 8;
// Only released packets enter this pool. Full queues discard excess buffers,
// keeping retained spare PCM capacity to one WinMM queue.
const RECYCLED_PACKETS: usize = PLAYBACK_PACKETS;
// Cover the native codec's 640 ms steady bursts, including after an underrun.
const PREROLL_PACKETS: usize = 7;
const CODEC_BURST: Duration = Duration::from_millis(640);
const MAX_BUFFER_BYTES: usize = 48_000 * 120;

pub(super) const fn packet_bytes_for_rate(sample_rate: u32) -> usize {
    if sample_rate == SAMPLE_RATE_48K {
        PACKET_BYTES_48K
    } else {
        PACKET_BYTES
    }
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Clamped i16 PCM range [-32768, 32767] fits in i16 after rounding."
)]
fn float_to_i16(value: f32) -> i16 {
    value
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
        .round() as i16
}

/// Apply a symmetric raised-cosine (Hann) C1-continuous crossfade across the
/// trailing samples of `prev_tail_pcm16` and the leading samples of `next_head_pcm16`.
///
/// The Hann weights `w_in(i) = 0.5 * (1 - cos(pi * (i + 1) / (N + 1)))` and
/// `w_out(i) = 1 - w_in(i)` form an exact partition of unity (`w_in + w_out == 1.0`),
/// guiding both segment edges smoothly to their shared midpoint at the splice
/// without altering segment duration or distorting continuous signals.
pub(super) fn smooth_segment_boundary(prev_tail_pcm16: &mut [u8], next_head_pcm16: &mut [u8]) {
    let prev_samples = prev_tail_pcm16.len() / 2;
    let next_samples = next_head_pcm16.len() / 2;
    let count = prev_samples.min(next_samples).min(HANN_BOUNDARY_SAMPLES);
    if count == 0 {
        return;
    }
    let prev_start = (prev_samples - count) * 2;
    let prev_end_idx = (prev_samples - 1) * 2;
    let prev_last = f32::from(i16::from_le_bytes([
        prev_tail_pcm16[prev_end_idx],
        prev_tail_pcm16[prev_end_idx + 1],
    ]));
    let next_first = f32::from(i16::from_le_bytes([next_head_pcm16[0], next_head_pcm16[1]]));
    let midpoint = f32::midpoint(prev_last, next_first);
    let denom = f32::from(u16::try_from(count + 1).unwrap_or(2));
    for index in 0..count {
        let step = f32::from(u16::try_from(index + 1).unwrap_or(1));
        let phase = core::f32::consts::PI * step / denom;
        let w_in = 0.5_f32 * (1.0_f32 - phase.cos());
        let w_out = 1.0_f32 - w_in;

        let p_off = prev_start + index * 2;
        let p_val = f32::from(i16::from_le_bytes([
            prev_tail_pcm16[p_off],
            prev_tail_pcm16[p_off + 1],
        ]));
        let p_smoothed = float_to_i16(p_val.mul_add(w_out, midpoint * w_in)).to_le_bytes();
        prev_tail_pcm16[p_off] = p_smoothed[0];
        prev_tail_pcm16[p_off + 1] = p_smoothed[1];

        let n_off = index * 2;
        let n_val = f32::from(i16::from_le_bytes([
            next_head_pcm16[n_off],
            next_head_pcm16[n_off + 1],
        ]));
        let n_smoothed = float_to_i16(n_val.mul_add(w_in, midpoint * w_out)).to_le_bytes();
        next_head_pcm16[n_off] = n_smoothed[0];
        next_head_pcm16[n_off + 1] = n_smoothed[1];
    }
}

enum Audio {
    Frame(Vec<u8>),
    SegmentEnd,
}

pub(super) struct Pipeline {
    segments: Option<SyncSender<String>>,
    finished: Receiver<Result<(), tts::Failure>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub(super) fn start(
        client: Arc<tts::Client>,
        voice: String,
        generation: u64,
        current: Arc<AtomicU64>,
    ) -> io::Result<Self> {
        let (complete, finished) = mpsc::channel();
        let (segments, incoming) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("bonsai-tts-prefetch".to_owned())
            .spawn(move || {
                let result = produce(&client, &incoming, &voice, generation, &current, &stop);
                let _sent = complete.send(result);
            })?;
        Ok(Self {
            segments: Some(segments),
            finished,
            cancelled,
            worker: Some(worker),
        })
    }

    pub(super) fn feed(&mut self, queued: &mut VecDeque<String>, finished: bool) {
        if let Some(sender) = &self.segments {
            while let Some(text) = queued.pop_front() {
                match sender.try_send(text) {
                    Ok(()) => {}
                    Err(TrySendError::Full(text)) => {
                        queued.push_front(text);
                        break;
                    }
                    Err(TrySendError::Disconnected(_)) => break,
                }
            }
        }
        if finished && queued.is_empty() {
            self.segments = None;
        }
    }

    pub(super) fn result(&self) -> Option<Result<(), tts::Failure>> {
        match self.finished.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(AUDIO_FAILURE)),
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

fn produce(
    client: &tts::Client,
    segments: &Receiver<String>,
    voice: &str,
    generation: u64,
    current: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
) -> Result<(), tts::Failure> {
    let sample_rate = client.sample_rate();
    let packet_bytes = packet_bytes_for_rate(sample_rate);
    let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
    let (recycle, recycled) = mpsc::sync_channel(RECYCLED_PACKETS);
    let output_stop = Arc::clone(stop);
    let output_current = Arc::clone(current);
    let playback = thread::Builder::new()
        .name("bonsai-tts-pcm".to_owned())
        .spawn(move || {
            let result = consume_at_rate(
                &incoming,
                generation,
                &output_current,
                &output_stop,
                recycle,
                sample_rate,
            );
            if result.is_err() {
                output_stop.store(true, Ordering::Release);
            }
            result
        })
        .map_err(|_| AUDIO_FAILURE)?;
    let synthesis = (|| {
        let mut pending = None;
        let mut frames = Frames::reusing_with_packet_bytes(recycled, packet_bytes);
        while let Some(text) = next_segment(segments, &mut pending, generation, current, stop)? {
            client.synthesize_pcm(&text, voice, generation, current, stop, |pcm| {
                frames.push(pcm, |packet| {
                    send(&sender, Audio::Frame(packet), generation, current, stop)
                })
            })?;
            frames.finish_segment(|packet| {
                send(&sender, Audio::Frame(packet), generation, current, stop)
            })?;
            send(&sender, Audio::SegmentEnd, generation, current, stop)?;
        }
        Ok(())
    })();
    if synthesis.is_err() {
        stop.store(true, Ordering::Release);
    }
    drop(sender);
    let played = playback.join().unwrap_or(Err(AUDIO_FAILURE));
    if let Err(failure) = played
        && failure != tts::Failure::Cancelled
    {
        return Err(failure);
    }
    synthesis?;
    played
}

fn next_segment(
    segments: &Receiver<String>,
    pending: &mut Option<String>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<Option<String>, tts::Failure> {
    let mut text = loop {
        check(generation, current, stop)?;
        if let Some(text) = pending.take() {
            break text;
        }
        match segments.recv_timeout(POLL) {
            Ok(text) => break text,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
        }
    };
    let mut characters = text.chars().count();
    while !super::segmentation::ends_sentence(&text)
        && let Ok(next) = segments.try_recv()
    {
        check(generation, current, stop)?;
        let count = next.chars().count();
        if characters + 1 + count > super::MAX_SEGMENT {
            *pending = Some(next);
            break;
        }
        text.push(' ');
        text.push_str(&next);
        characters += 1 + count;
    }
    Ok(Some(text))
}

struct Frames {
    pending: Vec<u8>,
    recycled: Option<Receiver<Vec<u8>>>,
    packet_bytes: usize,
    previous_tail_sample: Option<i16>,
    onset_remaining: usize,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            pending: Vec::with_capacity(PACKET_BYTES),
            recycled: None,
            packet_bytes: PACKET_BYTES,
            previous_tail_sample: None,
            onset_remaining: 0,
        }
    }
}

impl Frames {
    #[cfg(test)]
    fn reusing(recycled: Receiver<Vec<u8>>) -> Self {
        Self::reusing_with_packet_bytes(recycled, PACKET_BYTES)
    }

    fn reusing_with_packet_bytes(recycled: Receiver<Vec<u8>>, packet_bytes: usize) -> Self {
        Self {
            pending: Vec::with_capacity(packet_bytes),
            recycled: Some(recycled),
            packet_bytes,
            previous_tail_sample: None,
            onset_remaining: 0,
        }
    }

    fn replacement(&self) -> Vec<u8> {
        self.recycled
            .as_ref()
            .and_then(|recycled| recycled.try_recv().ok())
            .unwrap_or_else(|| Vec::with_capacity(self.packet_bytes))
    }

    fn apply_onset_smoothing(&mut self, start_offset: usize) {
        let Some(prev_last) = self.previous_tail_sample else {
            return;
        };
        if self.onset_remaining == 0 || self.pending.len() < start_offset + 2 {
            return;
        }
        let available_samples = (self.pending.len() - start_offset) / 2;
        let to_smooth = available_samples.min(self.onset_remaining);
        if to_smooth == 0 {
            return;
        }
        let first_sample = f32::from(i16::from_le_bytes([
            self.pending[start_offset],
            self.pending[start_offset + 1],
        ]));
        let midpoint = f32::midpoint(f32::from(prev_last), first_sample);
        let total = HANN_BOUNDARY_SAMPLES;
        let already_done = total.saturating_sub(self.onset_remaining);
        let denom = f32::from(u16::try_from(total + 1).unwrap_or(2));
        for idx in 0..to_smooth {
            let step = f32::from(u16::try_from(already_done + idx + 1).unwrap_or(1));
            let phase = core::f32::consts::PI * step / denom;
            let w_in = 0.5_f32 * (1.0_f32 - phase.cos());
            let w_out = 1.0_f32 - w_in;
            let off = start_offset + idx * 2;
            let sample = f32::from(i16::from_le_bytes([
                self.pending[off],
                self.pending[off + 1],
            ]));
            let smoothed = float_to_i16(sample.mul_add(w_in, midpoint * w_out)).to_le_bytes();
            self.pending[off] = smoothed[0];
            self.pending[off + 1] = smoothed[1];
        }
        self.onset_remaining -= to_smooth;
    }

    fn push(
        &mut self,
        mut pcm: &[u8],
        mut sink: impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        while !pcm.is_empty() {
            let start_offset = self.pending.len();
            let count = pcm.len().min(self.packet_bytes - start_offset);
            self.pending.extend_from_slice(&pcm[..count]);
            if self.onset_remaining > 0 {
                self.apply_onset_smoothing(start_offset - (start_offset % 2));
            }
            pcm = &pcm[count..];
            if self.pending.len() == self.packet_bytes {
                self.previous_tail_sample = Some(i16::from_le_bytes([
                    self.pending[self.packet_bytes - 2],
                    self.pending[self.packet_bytes - 1],
                ]));
                let replacement = self.replacement();
                sink(std::mem::replace(&mut self.pending, replacement))?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn finish(
        &mut self,
        sink: impl FnOnce(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if self.pending.is_empty() {
            Ok(())
        } else if self.pending.len() % 2 == 0 {
            let replacement = self.replacement();
            sink(std::mem::replace(&mut self.pending, replacement))
        } else {
            Err(AUDIO_FAILURE)
        }
    }

    fn finish_segment(
        &mut self,
        sink: impl FnOnce(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if self.pending.is_empty() {
            self.onset_remaining = HANN_BOUNDARY_SAMPLES;
            Ok(())
        } else if self.pending.len() % 2 == 0 {
            let len = self.pending.len();
            // Apply a gentle Hann tail taper toward 0 on the final HANN_BOUNDARY_SAMPLES
            // of the segment and record the boundary sample for the next segment's onset.
            let mut next_dummy = [0_u8; HANN_BOUNDARY_SAMPLES * 2];
            smooth_segment_boundary(&mut self.pending, &mut next_dummy);
            let tail = i16::from_le_bytes([self.pending[len - 2], self.pending[len - 1]]);
            self.previous_tail_sample = Some(tail);
            self.onset_remaining = HANN_BOUNDARY_SAMPLES;
            let replacement = self.replacement();
            sink(std::mem::replace(&mut self.pending, replacement))
        } else {
            Err(AUDIO_FAILURE)
        }
    }
}

fn recycle(sender: &SyncSender<Vec<u8>>, mut pcm: Vec<u8>) {
    if pcm.capacity() == PACKET_BYTES || pcm.capacity() == PACKET_BYTES_48K {
        pcm.clear();
        let _returned = sender.try_send(pcm);
    }
}

fn check(generation: u64, current: &AtomicU64, stop: &AtomicBool) -> Result<(), tts::Failure> {
    if stop.load(Ordering::Acquire) || current.load(Ordering::Acquire) != generation {
        Err(tts::Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn send(
    sender: &SyncSender<Audio>,
    mut pcm: Audio,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(), tts::Failure> {
    loop {
        check(generation, current, stop)?;
        match sender.try_send(pcm) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(packet)) => {
                pcm = packet;
                thread::sleep(POLL);
            }
            Err(TrySendError::Disconnected(_)) => return Err(AUDIO_FAILURE),
        }
    }
}

fn consume_at_rate(
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    recycle: SyncSender<Vec<u8>>,
    sample_rate: u32,
) -> Result<(), tts::Failure> {
    consume_with(
        incoming,
        generation,
        current,
        stop,
        &mut WaveOutput::open_with_rate(recycle, sample_rate)?,
    )
}

trait Output {
    fn queued(&self) -> usize;
    fn push(&mut self, pcm: Vec<u8>) -> Result<(), tts::Failure>;
    fn reap(&mut self) -> Result<(), tts::Failure>;
    fn pause(&mut self) -> Result<(), tts::Failure>;
    fn start(&mut self) -> Result<(), tts::Failure>;
}

fn consume_with(
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    output: &mut impl Output,
) -> Result<(), tts::Failure> {
    output.pause()?;
    let mut started = false;
    let mut buffered = Buffered::default();
    let mut pending = None;
    let mut finished = false;
    loop {
        check(generation, current, stop)?;
        output.reap()?;
        if started && output.queued() == 0 && buffered.ready.is_empty() {
            output.pause()?;
            if buffered.released {
                buffered.underrun();
            }
            started = false;
        }
        while output.queued() < PLAYBACK_PACKETS {
            if let Some(pcm) = buffered.pop() {
                output.push(pcm)?;
                let flushed = buffered.ready.is_empty() && buffered.flush;
                if flushed {
                    buffered.flush = false;
                }
                if !started && (output.queued() >= PREROLL_PACKETS || flushed) {
                    output.start()?;
                    started = true;
                }
            } else {
                break;
            }
        }
        if buffered.ready.is_empty() && buffered.flush {
            buffered.flush = false;
            if !started && output.queued() > 0 {
                output.start()?;
                started = true;
            }
        }
        if finished && buffered.ready.is_empty() && output.queued() == 0 {
            return Ok(());
        }
        let audio = if pending.is_some() {
            pending.take()
        } else if finished {
            None
        } else {
            match incoming.recv_timeout(POLL) {
                Ok(audio) => Some(audio),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if buffered.segment_bytes != 0 {
                        return Err(AUDIO_FAILURE);
                    }
                    finished = true;
                    None
                }
            }
        };
        check(generation, current, stop)?;
        match audio {
            Some(Audio::Frame(pcm)) => {
                if let Some(pcm) = buffered.frame(pcm, Instant::now())? {
                    pending = Some(Audio::Frame(pcm));
                }
            }
            Some(Audio::SegmentEnd) => buffered.end()?,
            None => {}
        }
        if pending.is_some() || finished {
            thread::sleep(POLL);
        }
    }
}

// At most one 120-second clip of application staging, plus the 16-second
// producer channel and 800 ms WinMM queue. Slow synthesis waits for a validated
// segment instead of repeatedly playing short bursts separated by silence.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Playback staging tracks four independent boolean lifecycle latches."
)]
struct Buffered {
    ready: VecDeque<Vec<u8>>,
    segment: VecDeque<Vec<u8>>,
    bytes: usize,
    segment_bytes: usize,
    packet_bytes: usize,
    first: Option<(Instant, usize)>,
    released: bool,
    slow: bool,
    underrun: bool,
    flush: bool,
}

impl Buffered {
    fn frame(&mut self, pcm: Vec<u8>, now: Instant) -> Result<Option<Vec<u8>>, tts::Failure> {
        let max_packet = if pcm.len() > PACKET_BYTES || self.packet_bytes == PACKET_BYTES_48K {
            PACKET_BYTES_48K
        } else {
            PACKET_BYTES
        };
        self.packet_bytes = max_packet;
        let max_buffer = if max_packet == PACKET_BYTES_48K {
            MAX_BUFFER_BYTES * 2
        } else {
            MAX_BUFFER_BYTES
        };
        if pcm.is_empty()
            || pcm.len() > max_packet
            || pcm.len() % 2 != 0
            || self.segment_bytes + pcm.len() > max_buffer
        {
            return Err(AUDIO_FAILURE);
        }
        if self.bytes + pcm.len() > max_buffer {
            return Ok(Some(pcm));
        }
        let (first, initial) = *self.first.get_or_insert((now, pcm.len()));
        self.segment_bytes += pcm.len();
        self.bytes += pcm.len();
        if self.released {
            self.ready.push_back(pcm);
        } else {
            let bytes_per_ms = if max_packet == PACKET_BYTES_48K {
                96
            } else {
                48
            };
            self.segment.push_back(pcm);
            if self.segment.len() >= PREROLL_PACKETS
                && self.segment_bytes >= PREROLL_PACKETS * max_packet
            {
                // Exclude cold first-audio latency. This compares later supply
                // time to the duration it produced, not to model load time.
                let duration = Duration::from_millis(
                    u64::try_from(self.segment_bytes - initial).map_err(|_| AUDIO_FAILURE)?
                        / bytes_per_ms,
                );
                // The native ramp groups 80/160/320/640 ms into separate callbacks,
                // requiring 12 frames (960 ms) and two vocoder decodes between the
                // first packet (Callback 2) and Packet 7 (Callback 4).
                self.slow |= now.saturating_duration_since(first)
                    > duration.saturating_mul(2) + CODEC_BURST.saturating_mul(2);
                if !self.slow {
                    self.ready.append(&mut self.segment);
                    self.released = true;
                }
            }
        }
        Ok(None)
    }

    fn end(&mut self) -> Result<(), tts::Failure> {
        if self.segment_bytes == 0 {
            return Err(AUDIO_FAILURE);
        }
        if self.segment.is_empty() {
            self.underrun = false;
        }
        self.ready.append(&mut self.segment);
        self.segment_bytes = 0;
        self.first = None;
        self.released = false;
        self.slow = self.underrun;
        self.flush = true;
        Ok(())
    }

    fn pop(&mut self) -> Option<Vec<u8>> {
        let pcm = self.ready.pop_front()?;
        self.bytes -= pcm.len();
        Some(pcm)
    }

    const fn underrun(&mut self) {
        self.underrun = true;
        self.slow = true;
        self.released = false;
    }
}

struct Buffer {
    // WinMM mutates this descriptor until unprepare releases it. No Rust reference
    // to the WAVEHDR itself exists while the driver owns it.
    header: UnsafeCell<WAVEHDR>,
    pcm: Vec<u8>,
}

struct PlaybackBuffers {
    // WinMM retains these addresses. Moving the boxes never moves the headers.
    queued: VecDeque<Box<Buffer>>,
    // Only successfully unprepared, empty descriptors enter this bounded pool.
    #[allow(
        clippy::vec_box,
        reason = "Reused WinMM descriptors must keep their original stable addresses."
    )]
    spare: Vec<Box<Buffer>>,
    max_packet_bytes: usize,
}

impl Default for PlaybackBuffers {
    fn default() -> Self {
        Self::with_max_packet_bytes(PACKET_BYTES)
    }
}

impl PlaybackBuffers {
    const fn with_max_packet_bytes(max_packet_bytes: usize) -> Self {
        Self {
            queued: VecDeque::new(),
            spare: Vec::new(),
            max_packet_bytes,
        }
    }

    fn take(&mut self, mut pcm: Vec<u8>) -> Result<Box<Buffer>, tts::Failure> {
        if self.queued.len() >= PLAYBACK_PACKETS
            || pcm.is_empty()
            || pcm.len() > self.max_packet_bytes
            || pcm.len() % 2 != 0
        {
            return Err(AUDIO_FAILURE);
        }
        let header = WAVEHDR {
            lpData: pcm.as_mut_ptr(),
            dwBufferLength: u32::try_from(pcm.len()).map_err(|_| AUDIO_FAILURE)?,
            ..WAVEHDR::default()
        };
        let mut buffer = self.spare.pop().unwrap_or_else(|| {
            Box::new(Buffer {
                header: UnsafeCell::new(WAVEHDR::default()),
                pcm: Vec::new(),
            })
        });
        // The driver has released this descriptor (or has never seen it).
        *buffer.header.get_mut() = header;
        buffer.pcm = pcm;
        Ok(buffer)
    }

    fn reap(
        &mut self,
        mut unprepare: impl FnMut(*mut WAVEHDR) -> u32,
        returned: &SyncSender<Vec<u8>>,
    ) -> Result<(), tts::Failure> {
        while let Some(buffer) = self.queued.front() {
            match unprepare(buffer.header.get()) {
                0 => {
                    if let Some(mut buffer) = self.queued.pop_front() {
                        // Successful unprepare releases both native addresses.
                        // Remove every stale driver field and PCM pointer before reuse.
                        *buffer.header.get_mut() = WAVEHDR::default();
                        recycle(returned, std::mem::take(&mut buffer.pcm));
                        if self.spare.len() < PLAYBACK_PACKETS {
                            self.spare.push(buffer);
                        }
                    }
                }
                33 => break, // WAVERR_STILLPLAYING: retain both allocations.
                _ => return Err(AUDIO_FAILURE),
            }
        }
        Ok(())
    }

    fn drain(
        &mut self,
        mut unprepare: impl FnMut(*mut WAVEHDR) -> u32,
        mut retain: impl FnMut(Box<Buffer>),
    ) -> bool {
        let mut released = true;
        while let Some(buffer) = self.queued.pop_front() {
            if unprepare(buffer.header.get()) != 0 {
                retain(buffer);
                released = false;
            }
        }
        released
    }
}

struct WaveOutput {
    handle: HWAVEOUT,
    buffers: PlaybackBuffers,
    header_bytes: u32,
    recycle: SyncSender<Vec<u8>>,
}

impl WaveOutput {
    fn open_with_rate(
        recycle: SyncSender<Vec<u8>>,
        sample_rate: u32,
    ) -> Result<Self, tts::Failure> {
        let rate = if sample_rate == SAMPLE_RATE_48K {
            SAMPLE_RATE_48K
        } else {
            SAMPLE_RATE
        };
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut handle = std::ptr::null_mut();
        // SAFETY: Writable handle output and complete fixed PCM descriptor live through the call;
        // CALLBACK_NULL registers no callback or user-data pointer.
        if unsafe { waveOutOpen(&raw mut handle, u32::MAX, &raw const format, 0, 0, 0) } != 0 {
            return Err(AUDIO_FAILURE);
        }
        Ok(Self {
            handle,
            buffers: PlaybackBuffers::with_max_packet_bytes(packet_bytes_for_rate(rate)),
            header_bytes: u32::try_from(size_of::<WAVEHDR>()).map_err(|_| AUDIO_FAILURE)?,
            recycle,
        })
    }
}

impl Output for WaveOutput {
    fn queued(&self) -> usize {
        self.buffers.queued.len()
    }

    fn pause(&mut self) -> Result<(), tts::Failure> {
        // SAFETY: This thread exclusively owns a live WinMM handle. Pausing
        // retains queued headers and PCM until restart/reset releases them.
        if unsafe { waveOutPause(self.handle) } == 0 {
            Ok(())
        } else {
            Err(AUDIO_FAILURE)
        }
    }

    fn start(&mut self) -> Result<(), tts::Failure> {
        // SAFETY: The live handle and queued buffers remain exclusively owned.
        if unsafe { waveOutRestart(self.handle) } == 0 {
            Ok(())
        } else {
            Err(AUDIO_FAILURE)
        }
    }

    fn push(&mut self, pcm: Vec<u8>) -> Result<(), tts::Failure> {
        let buffer = self.buffers.take(pcm)?;
        // SAFETY: This exclusive boxed header and its owned PCM allocation remain stable until
        // unprepare confirms release; the descriptor size matches the native ABI.
        if unsafe { waveOutPrepareHeader(self.handle, buffer.header.get(), self.header_bytes) } != 0
        {
            return Err(AUDIO_FAILURE);
        }
        self.buffers.queued.push_back(buffer);
        let buffer = self.buffers.queued.back().ok_or(AUDIO_FAILURE)?;
        // SAFETY: The prepared header and PCM remain owned by buffers until the driver releases them.
        if unsafe { waveOutWrite(self.handle, buffer.header.get(), self.header_bytes) } != 0 {
            return Err(AUDIO_FAILURE);
        }
        Ok(())
    }

    fn reap(&mut self) -> Result<(), tts::Failure> {
        self.buffers.reap(
            |header| {
                // SAFETY: The submitted header remains boxed and owned by buffers.
                // Unprepare tests release without reading driver-written flags in Rust.
                unsafe { waveOutUnprepareHeader(self.handle, header, self.header_bytes) }
            },
            &self.recycle,
        )
    }
}

impl Drop for WaveOutput {
    fn drop(&mut self) {
        // SAFETY: This thread exclusively owns the live handle; reset stops playback and returns
        // queued buffers before unprepare and release. No callbacks can outlive these buffers.
        let reset = unsafe { waveOutReset(self.handle) };
        let released = self.buffers.drain(
            |header| {
                // SAFETY: The header remains allocated while asking the driver to release it.
                unsafe { waveOutUnprepareHeader(self.handle, header, self.header_bytes) }
            },
            |buffer| {
                // An abnormal driver refusal cannot justify freeing memory still owned by WinMM.
                // This bounded allocation survives to process exit instead of becoming dangling.
                let _retained = Box::leak(buffer);
            },
        );
        if reset == 0 && released {
            // SAFETY: Reset succeeded and all prepared buffers have been released.
            let _closed = unsafe { waveOutClose(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arc, AtomicBool, AtomicU64, Audio, Buffered, Duration, Frames, HANN_BOUNDARY_SAMPLES,
        Instant, MAX_BUFFER_BYTES, Ordering, Output, PACKET_BYTES, PACKET_BYTES_48K,
        PLAYBACK_PACKETS, PREFETCH_PACKETS, PREROLL_PACKETS, SAMPLE_RATE, SAMPLE_RATE_48K,
        VecDeque, check, consume_with, mpsc, packet_bytes_for_rate, send, smooth_segment_boundary,
    };
    use crate::tts::Failure;

    #[test]
    fn hann_window_boundary_smoothing_preserves_unity_and_eliminates_step_clicks() {
        assert_eq!(packet_bytes_for_rate(SAMPLE_RATE), PACKET_BYTES);
        assert_eq!(packet_bytes_for_rate(SAMPLE_RATE_48K), PACKET_BYTES_48K);

        // A constant signal across the boundary remains unchanged.
        let constant_sample = 12_000_i16.to_le_bytes();
        let mut prev_const = constant_sample.repeat(HANN_BOUNDARY_SAMPLES * 2);
        let mut next_const = constant_sample.repeat(HANN_BOUNDARY_SAMPLES * 2);
        let expected_const = prev_const.clone();
        smooth_segment_boundary(&mut prev_const, &mut next_const);
        assert_eq!(prev_const, expected_const);
        assert_eq!(next_const, expected_const);

        // Opposite-polarity DC step (+10000 -> -10000) tapers smoothly toward 0 at the splice.
        let mut prev_step = 10_000_i16.to_le_bytes().repeat(HANN_BOUNDARY_SAMPLES);
        let mut next_step = (-10_000_i16).to_le_bytes().repeat(HANN_BOUNDARY_SAMPLES);
        smooth_segment_boundary(&mut prev_step, &mut next_step);
        let last_prev = i16::from_le_bytes([
            prev_step[prev_step.len() - 2],
            prev_step[prev_step.len() - 1],
        ]);
        let first_next = i16::from_le_bytes([next_step[0], next_step[1]]);
        assert!(last_prev.abs() < 200);
        assert!(first_next.abs() < 200);
        assert!((i32::from(last_prev) - i32::from(first_next)).abs() < 400);
    }

    #[test]
    fn streaming_segment_input_waits_through_gaps_then_finishes_or_cancels() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let current = AtomicU64::new(4);
        let stop = AtomicBool::new(false);
        assert!(sender.send("First".to_owned()).is_ok());
        assert_eq!(
            super::next_segment(&receiver, &mut None, 4, &current, &stop),
            Ok(Some("First".to_owned()))
        );
        current.store(5, Ordering::Release);
        assert_eq!(
            super::next_segment(&receiver, &mut None, 4, &current, &stop),
            Err(Failure::Cancelled)
        );
        drop(sender);
        assert_eq!(
            super::next_segment(&receiver, &mut None, 5, &current, &stop),
            Ok(None)
        );
        let (sender, receiver) = mpsc::sync_channel(2);
        let waiting = std::thread::spawn(move || {
            super::next_segment(
                &receiver,
                &mut None,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
            )
        });
        assert!(sender.send("Second".to_owned()).is_ok());
        assert_eq!(waiting.join().ok(), Some(Ok(Some("Second".to_owned()))));
    }

    #[test]
    fn bounded_prefetch_cancels_while_the_audio_queue_is_full() {
        let (sender, _incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        assert_eq!(PREFETCH_PACKETS * PACKET_BYTES, 48_000 * 16);
        for _ in 0..PREFETCH_PACKETS {
            assert!(sender.try_send(Audio::Frame(vec![0; PACKET_BYTES])).is_ok());
        }
        let current = Arc::new(AtomicU64::new(4));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_current = Arc::clone(&current);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            send(
                &sender,
                Audio::Frame(vec![0; 2]),
                4,
                &worker_current,
                &worker_stop,
            )
        });
        current.store(5, Ordering::Release);
        assert_eq!(worker.join().ok(), Some(Err(Failure::Cancelled)));
        stop.store(true, Ordering::Release);
        assert_eq!(check(5, &current, &stop), Err(Failure::Cancelled));
    }

    #[test]
    fn queued_short_text_coalesces_without_reordering_or_exceeding_the_request_bound() {
        let (sender, receiver) = mpsc::sync_channel(5);
        assert!(sender.send("Hello".to_owned()).is_ok());
        assert!(sender.send("another clause.".to_owned()).is_ok());
        assert!(sender.send("Separate sentence.".to_owned()).is_ok());
        assert!(sender.send("x".repeat(super::super::MAX_SEGMENT)).is_ok());
        assert!(sender.send("Last.".to_owned()).is_ok());
        drop(sender);
        let mut pending = None;
        let current = AtomicU64::new(4);
        let stop = AtomicBool::new(false);
        for expected in [
            "Hello another clause.".to_owned(),
            "Separate sentence.".to_owned(),
            "x".repeat(super::super::MAX_SEGMENT),
            "Last.".to_owned(),
        ] {
            assert_eq!(
                super::next_segment(&receiver, &mut pending, 4, &current, &stop),
                Ok(Some(expected))
            );
        }
        assert_eq!(
            super::next_segment(&receiver, &mut pending, 4, &current, &stop),
            Ok(None)
        );
    }

    #[test]
    fn tiny_pcm_callbacks_form_fixed_frames_and_flush_the_final_tail_exactly() -> Result<(), Failure>
    {
        let source: Vec<_> = (0_u8..=255).cycle().take(PACKET_BYTES * 3 + 42).collect();
        let mut frames = Frames::default();
        let mut packets = Vec::new();
        for tiny in source.chunks(2) {
            frames.push(tiny, |packet| {
                packets.push(packet);
                Ok(())
            })?;
            assert!(frames.pending.len() < PACKET_BYTES);
        }
        frames.finish(|packet| {
            packets.push(packet);
            Ok(())
        })?;
        assert_eq!(
            packets.iter().map(Vec::len).collect::<Vec<_>>(),
            [PACKET_BYTES, PACKET_BYTES, PACKET_BYTES, 42]
        );
        assert_eq!(packets.concat(), source);
        Ok(())
    }

    #[test]
    fn pcm_packet_allocations_reuse_only_returned_playback_storage() -> Result<(), Failure> {
        let (recycle, recycled) = mpsc::sync_channel(super::RECYCLED_PACKETS);
        let mut frames = Frames::reusing(recycled);
        let first_pointer = frames.pending.as_ptr();
        let mut packets = VecDeque::new();
        frames.push(&vec![1; PACKET_BYTES], |packet| {
            packets.push_back(packet);
            Ok(())
        })?;
        assert_ne!(frames.pending.as_ptr(), first_pointer);
        let first = packets.pop_front().ok_or(super::AUDIO_FAILURE)?;
        assert_eq!(first.as_ptr(), first_pointer);
        assert_eq!(first, vec![1; PACKET_BYTES]);
        super::recycle(&recycle, first);

        frames.push(&vec![2; PACKET_BYTES], |packet| {
            packets.push_back(packet);
            Ok(())
        })?;
        assert_eq!(frames.pending.as_ptr(), first_pointer);
        assert_eq!(frames.pending, Vec::<u8>::new());
        assert_eq!(frames.pending.capacity(), PACKET_BYTES);
        assert_eq!(packets.front(), Some(&vec![2; PACKET_BYTES]));

        frames.push(&[3; 42], |_| Err(super::AUDIO_FAILURE))?;
        frames.finish(|packet| {
            assert_eq!(packet.as_ptr(), first_pointer);
            assert_eq!(packet, [3; 42]);
            Ok(())
        })?;
        assert_eq!(frames.pending, Vec::<u8>::new());
        frames.finish(|_| Err(super::AUDIO_FAILURE))?;
        frames.push(&[4; 64], |_| Err(super::AUDIO_FAILURE))?;
        frames.finish(|packet| {
            assert_eq!(packet, [4; 64]);
            Ok(())
        })?;
        frames.push(&[5], |_| Err(super::AUDIO_FAILURE))?;
        assert_eq!(frames.finish(|_| Ok(())), Err(super::AUDIO_FAILURE));
        Ok(())
    }

    #[test]
    fn pcm_recycling_has_a_fixed_capacity_and_never_waits_for_the_producer() {
        let (sender, receiver) = mpsc::sync_channel(super::RECYCLED_PACKETS);
        for _ in 0..super::RECYCLED_PACKETS * 3 {
            super::recycle(&sender, vec![7; PACKET_BYTES]);
        }
        let returned: Vec<_> = receiver.try_iter().collect();
        assert_eq!(returned.len(), super::RECYCLED_PACKETS);
        assert!(returned.iter().all(Vec::is_empty));
        assert_eq!(
            returned.iter().map(Vec::capacity).sum::<usize>(),
            PLAYBACK_PACKETS * PACKET_BYTES
        );
        // A larger external allocation cannot expand retained spare capacity.
        super::recycle(&sender, vec![0; PACKET_BYTES * 3]);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        super::recycle(&sender, vec![0; PACKET_BYTES]);
    }

    #[test]
    fn playback_descriptors_reuse_released_allocations_and_clear_native_fields()
    -> Result<(), Failure> {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        let mut addresses = Vec::new();
        for _ in 0..PLAYBACK_PACKETS {
            let mut buffer = buffers.take(vec![7; PACKET_BYTES])?;
            addresses.push(buffer.header.get());
            // Simulate fields written during preparation/playback before putting
            // this descriptor in the fake driver's queue. No audio device is used.
            let header = buffer.header.get_mut();
            header.dwBytesRecorded = u32::MAX;
            header.dwFlags = u32::MAX;
            header.dwLoops = u32::MAX;
            header.dwUser = usize::MAX;
            header.lpNext = std::ptr::dangling_mut();
            header.reserved = usize::MAX;
            buffers.queued.push_back(buffer);
        }
        for _ in 0..16 {
            buffers.reap(
                |header| {
                    assert!(addresses.contains(&header));
                    0
                },
                &sender,
            )?;
            assert!(buffers.queued.is_empty());
            assert_eq!(buffers.spare.len(), PLAYBACK_PACKETS);
            assert_eq!(returned.try_iter().count(), PLAYBACK_PACKETS);
            for invalid in [Vec::new(), vec![0], vec![0; PACKET_BYTES + 2]] {
                assert!(buffers.take(invalid).is_err());
                assert_eq!(buffers.spare.len(), PLAYBACK_PACKETS);
            }
            for buffer in &mut buffers.spare {
                assert_eq!(buffer.pcm, b"");
                assert_eq!(buffer.pcm.capacity(), 0);
                let header = buffer.header.get_mut();
                assert!(header.lpData.is_null());
                assert_eq!({ header.dwBufferLength }, 0);
                assert_eq!({ header.dwBytesRecorded }, 0);
                assert_eq!({ header.dwFlags }, 0);
                assert_eq!({ header.dwLoops }, 0);
                assert_eq!({ header.dwUser }, 0);
                assert!(header.lpNext.is_null());
                assert_eq!({ header.reserved }, 0);
            }
            for index in 0..PLAYBACK_PACKETS {
                let bytes = if index == 0 { 42 } else { PACKET_BYTES };
                let mut pcm = Vec::with_capacity(PACKET_BYTES);
                pcm.resize(bytes, 3);
                let pcm_address = pcm.as_ptr();
                let mut buffer = buffers.take(pcm)?;
                assert!(addresses.contains(&buffer.header.get()));
                let header = buffer.header.get_mut();
                assert_eq!(header.lpData.cast_const(), pcm_address);
                assert_eq!(usize::try_from(header.dwBufferLength).ok(), Some(bytes));
                assert_eq!(buffer.pcm, vec![3; bytes]);
                buffers.queued.push_back(buffer);
            }
            assert!(buffers.spare.is_empty());
            // Rejection cannot consume a spare descriptor or exceed the native
            // queue bound, even when a caller bypasses the consume loop.
            assert!(buffers.take(vec![0; 2]).is_err());
            assert_eq!(buffers.queued.len(), PLAYBACK_PACKETS);
            assert_eq!(
                buffers.queued.front().map(|buffer| buffer.pcm.len()),
                Some(42)
            );
        }
        Ok(())
    }

    #[test]
    fn playback_descriptors_keep_driver_owned_storage_on_unprepare_refusal() -> Result<(), Failure>
    {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        let buffer = buffers.take(vec![9; PACKET_BYTES])?;
        let header = buffer.header.get();
        let pcm = buffer.pcm.as_ptr();
        buffers.queued.push_back(buffer);
        for result in [33, 1] {
            assert_eq!(
                buffers.reap(
                    |address| {
                        assert_eq!(address, header);
                        result
                    },
                    &sender
                ),
                if result == 33 {
                    Ok(())
                } else {
                    Err(super::AUDIO_FAILURE)
                }
            );
            assert_eq!(buffers.queued.len(), 1);
            assert!(buffers.spare.is_empty());
            assert!(returned.try_recv().is_err());
            let retained = buffers.queued.front().ok_or(super::AUDIO_FAILURE)?;
            assert_eq!(retained.header.get(), header);
            assert_eq!(retained.pcm.as_ptr(), pcm);
            assert_eq!(retained.pcm, vec![9; PACKET_BYTES]);
        }
        buffers.reap(|_| 0, &sender)?;
        assert_eq!(buffers.spare.len(), 1);
        assert_eq!(returned.try_recv().ok().map(|pcm| pcm.as_ptr()), Some(pcm));
        Ok(())
    }

    #[test]
    fn playback_descriptor_teardown_quarantines_only_refused_active_buffers() -> Result<(), Failure>
    {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        // Establish one spare descriptor. Teardown must never ask the driver to
        // unprepare it again or quarantine already released storage.
        let spare = buffers.take(vec![1; PACKET_BYTES])?;
        let spare_address = spare.header.get();
        buffers.queued.push_back(spare);
        buffers.reap(|_| 0, &sender)?;
        assert!(returned.try_recv().is_ok());
        let mut active = Vec::new();
        for _ in 0..3 {
            let buffer = super::PlaybackBuffers::default().take(vec![8; PACKET_BYTES])?;
            active.push((buffer.header.get(), buffer.pcm.as_ptr()));
            buffers.queued.push_back(buffer);
        }
        let mut calls = 0;
        let mut quarantined = Vec::new();
        assert!(!buffers.drain(
            |header| {
                assert_ne!(header, spare_address);
                assert_eq!(header, active[calls].0);
                calls += 1;
                u32::from(calls != 2)
            },
            |buffer| quarantined.push(buffer)
        ));
        assert_eq!(calls, 3);
        assert!(buffers.queued.is_empty());
        assert_eq!(buffers.spare.len(), 1);
        assert_eq!(quarantined.len(), 2);
        drop(buffers);
        for (buffer, index) in quarantined.iter().zip([0, 2]) {
            assert_eq!(buffer.header.get(), active[index].0);
            assert_eq!(buffer.pcm.as_ptr(), active[index].1);
            assert_eq!(buffer.pcm, vec![8; PACKET_BYTES]);
        }
        Ok(())
    }

    #[test]
    fn fast_supply_primes_after_cold_start_but_slow_supply_waits_for_validated_segment()
    -> Result<(), Failure> {
        let cold = Instant::now() + Duration::from_secs(90);
        let mut fast = Buffered::default();
        let mut slow = Buffered::default();
        for index in 0..PREROLL_PACKETS {
            let tick = u64::try_from(index).map_err(|_| super::AUDIO_FAILURE)?;
            assert!(
                fast.frame(
                    vec![1; PACKET_BYTES],
                    cold + Duration::from_millis(tick * 50)
                )?
                .is_none()
            );
            assert!(
                slow.frame(
                    vec![2; PACKET_BYTES],
                    cold + Duration::from_millis(tick * 800)
                )?
                .is_none()
            );
            if index + 1 < PREROLL_PACKETS {
                assert!(fast.ready.is_empty());
            }
            assert!(slow.ready.is_empty());
        }
        assert_eq!(fast.ready.len(), PREROLL_PACKETS);
        assert!(!fast.slow);
        assert!(slow.slow);
        slow.end()?;
        assert_eq!(slow.ready.len(), PREROLL_PACKETS);
        assert!(slow.flush);
        Ok(())
    }

    #[test]
    fn realtime_native_callback_ramp_keeps_streaming_despite_burst_jitter() -> Result<(), Failure> {
        let now = Instant::now() + Duration::from_secs(90);
        let mut buffered = Buffered::default();
        let mut frames = Frames::default();
        for (millis, bytes) in [(80, 3_840), (240, 7_680), (560, 15_360), (1_200, 30_720)] {
            frames.push(&vec![0; bytes], |frame| {
                assert!(
                    buffered
                        .frame(frame, now + Duration::from_millis(millis))?
                        .is_none()
                );
                Ok(())
            })?;
        }
        assert!(!buffered.slow);
        assert_eq!(buffered.ready.len(), 12);
        Ok(())
    }

    #[test]
    fn underrun_requires_complete_segments_and_staging_rejects_overlong_audio()
    -> Result<(), Failure> {
        let now = Instant::now();
        let mut buffered = Buffered::default();
        buffered.underrun();
        for _ in 0..MAX_BUFFER_BYTES / PACKET_BYTES {
            assert!(buffered.frame(vec![0; PACKET_BYTES], now)?.is_none());
        }
        assert!(buffered.ready.is_empty());
        assert_eq!(buffered.bytes, MAX_BUFFER_BYTES);
        assert!(buffered.frame(vec![0; 2], now).is_err());
        buffered.end()?;
        assert_eq!(buffered.frame(vec![0; 2], now)?, Some(vec![0; 2]));
        assert!(buffered.pop().is_some());
        assert!(buffered.frame(vec![0; 2], now)?.is_none());
        assert!(buffered.slow);
        Ok(())
    }

    #[derive(Default)]
    struct RecordingOutput {
        queued: VecDeque<Vec<u8>>,
        played: Vec<u8>,
        starts: Vec<usize>,
        paused: bool,
        maximum: usize,
    }

    impl Output for RecordingOutput {
        fn queued(&self) -> usize {
            self.queued.len()
        }

        fn push(&mut self, pcm: Vec<u8>) -> Result<(), Failure> {
            self.queued.push_back(pcm);
            self.maximum = self.maximum.max(self.queued.len());
            Ok(())
        }

        fn reap(&mut self) -> Result<(), Failure> {
            if !self.paused
                && let Some(pcm) = self.queued.pop_front()
            {
                self.played.extend(pcm);
            }
            Ok(())
        }

        fn pause(&mut self) -> Result<(), Failure> {
            self.paused = true;
            Ok(())
        }

        fn start(&mut self) -> Result<(), Failure> {
            self.starts.push(self.queued.len());
            self.paused = false;
            Ok(())
        }
    }

    #[test]
    fn synthetic_output_primes_and_drains_short_final_segments_without_an_audio_device()
    -> Result<(), Failure> {
        for frames in [1, PREROLL_PACKETS + 5] {
            let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
            for _ in 0..frames {
                assert!(sender.send(Audio::Frame(vec![7; PACKET_BYTES])).is_ok());
            }
            assert!(sender.send(Audio::SegmentEnd).is_ok());
            drop(sender);
            let mut output = RecordingOutput::default();
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
                &mut output,
            )?;
            assert_eq!(output.played, vec![7; PACKET_BYTES * frames]);
            assert_eq!(output.starts.first(), Some(&frames.min(PREROLL_PACKETS)));
            assert!(output.maximum <= PLAYBACK_PACKETS);
        }
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        for _ in 0..PREROLL_PACKETS {
            assert!(sender.send(Audio::Frame(vec![1; PACKET_BYTES])).is_ok());
        }
        assert!(sender.send(Audio::SegmentEnd).is_ok());
        for _ in 0..PREROLL_PACKETS {
            assert!(sender.send(Audio::Frame(vec![2; PACKET_BYTES])).is_ok());
        }
        assert!(sender.send(Audio::Frame(vec![2; PACKET_BYTES])).is_ok());
        assert!(sender.send(Audio::SegmentEnd).is_ok());
        drop(sender);
        let mut multi_segment_output = RecordingOutput::default();
        consume_with(
            &incoming,
            4,
            &AtomicU64::new(4),
            &AtomicBool::new(false),
            &mut multi_segment_output,
        )?;
        assert_eq!(
            multi_segment_output.starts,
            vec![PREROLL_PACKETS, PREROLL_PACKETS]
        );
        assert_eq!(
            multi_segment_output.played.len(),
            PACKET_BYTES * (PREROLL_PACKETS * 2 + 1)
        );
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        assert!(sender.send(Audio::Frame(vec![0; 2])).is_ok());
        drop(sender);
        assert!(
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
                &mut RecordingOutput::default()
            )
            .is_err()
        );
        assert_eq!(
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(5),
                &AtomicBool::new(false),
                &mut RecordingOutput::default()
            ),
            Err(Failure::Cancelled)
        );
        Ok(())
    }

    #[test]
    fn punctuation_boundaries_drain_48k_partial_tails_and_reset_false_underruns()
    -> Result<(), Failure> {
        // 1. 48 kHz short sentence (4 full 9,600-byte packets + 1 partial 1,920-byte tail
        // at a punctuation mark) must not prematurely release on the partial tail or hang
        // paused with queued buffers when SegmentEnd arrives.
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        let mut frames_48k = Frames {
            packet_bytes: PACKET_BYTES_48K,
            ..Frames::default()
        };
        let sentence_one = vec![100_u8; PACKET_BYTES_48K * 4 + 1_920];
        frames_48k.push(&sentence_one, |packet| {
            sender
                .send(Audio::Frame(packet))
                .map_err(|_| super::AUDIO_FAILURE)
        })?;
        frames_48k.finish_segment(|packet| {
            sender
                .send(Audio::Frame(packet))
                .map_err(|_| super::AUDIO_FAILURE)
        })?;
        assert!(sender.send(Audio::SegmentEnd).is_ok());
        let sentence_two = vec![50_u8; PACKET_BYTES_48K * PREROLL_PACKETS];
        frames_48k.push(&sentence_two, |packet| {
            sender
                .send(Audio::Frame(packet))
                .map_err(|_| super::AUDIO_FAILURE)
        })?;
        frames_48k.finish_segment(|packet| {
            sender
                .send(Audio::Frame(packet))
                .map_err(|_| super::AUDIO_FAILURE)
        })?;
        assert!(sender.send(Audio::SegmentEnd).is_ok());
        drop(sender);

        let mut output_48k = RecordingOutput::default();
        consume_with(
            &incoming,
            1,
            &AtomicU64::new(1),
            &AtomicBool::new(false),
            &mut output_48k,
        )?;
        assert_eq!(
            output_48k.played.len(),
            sentence_one.len() + sentence_two.len()
        );
        assert_eq!(output_48k.starts, vec![5, PREROLL_PACKETS]);

        // 2. A boundary underrun right after all released packets of a sentence have
        // drained (before SegmentEnd is popped) must reset when SegmentEnd arrives so
        // the next sentence after punctuation still streams after PREROLL_PACKETS.
        let now = Instant::now();
        let mut buffered = Buffered::default();
        for _ in 0..PREROLL_PACKETS {
            assert!(buffered.frame(vec![1; PACKET_BYTES], now)?.is_none());
        }
        assert!(buffered.released);
        while buffered.pop().is_some() {}
        buffered.underrun();
        assert!(buffered.slow);
        buffered.end()?;
        assert!(!buffered.slow);
        for _ in 0..PREROLL_PACKETS {
            assert!(buffered.frame(vec![2; PACKET_BYTES], now)?.is_none());
        }
        assert!(buffered.released);
        assert_eq!(buffered.ready.len(), PREROLL_PACKETS);
        Ok(())
    }
}
