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
// Five milliseconds at the largest supported rate. Hold only this tail so exact
// packet-aligned segment endings can also reach silence without losing samples.
const MAX_EDGE_BYTES: usize = PACKET_BYTES_48K / 20;
const PREFETCH_PACKETS: usize = 160; // 16 seconds / 768,000 bytes at 24 kHz, outside WinMM.
const PLAYBACK_PACKETS: usize = 8;
// Only released packets enter this pool. Full queues discard excess buffers,
// keeping retained spare PCM capacity to one WinMM queue.
const RECYCLED_PACKETS: usize = PLAYBACK_PACKETS;
// The Rust codec emits one 80 ms frame per callback. Observe three packet
// intervals before starting, retaining four packets to absorb callback jitter.
const PREROLL_PACKETS: usize = 4;
const CODEC_BURST: Duration = Duration::from_millis(80);
const MAX_BUFFER_BYTES: usize = 48_000 * 120;

pub(super) const fn packet_bytes_for_rate(sample_rate: u32) -> usize {
    if sample_rate == SAMPLE_RATE_48K {
        PACKET_BYTES_48K
    } else {
        PACKET_BYTES
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Clamped i16 PCM range [-32768, 32767] fits in i16 after rounding."
)]
fn float_to_i16(value: f32) -> i16 {
    value
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
        .round() as i16
}

// Explicit endpoints also cover a one-sample clip and avoid rounding a residual
// step into silence. The half cosine has zero slope at both ends of the ramp.
fn edge_gain(index: usize, count: usize) -> f32 {
    if index == 0 || count <= 1 {
        0.0
    } else if index + 1 >= count {
        1.0
    } else {
        let step = f32::from(u16::try_from(index).unwrap_or(0));
        let denominator = f32::from(u16::try_from(count - 1).unwrap_or(1));
        0.5 * (1.0 - (core::f32::consts::PI * step / denominator).cos())
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
    playing: Arc<AtomicBool>,
    notified_playing: bool,
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
        let playing = Arc::new(AtomicBool::new(false));
        let output_playing = Arc::clone(&playing);
        let worker = thread::Builder::new()
            .name("bonsai-tts-prefetch".to_owned())
            .spawn(move || {
                let result = produce(
                    &client,
                    &incoming,
                    &voice,
                    generation,
                    &current,
                    &stop,
                    &output_playing,
                );
                let _sent = complete.send(result);
            })?;
        Ok(Self {
            segments: Some(segments),
            finished,
            cancelled,
            playing,
            notified_playing: false,
            worker: Some(worker),
        })
    }

    pub(super) fn feed(&mut self, queued: &mut VecDeque<String>, finished: bool) {
        if let Some(sender) = &self.segments {
            while let Some(mut text) = queued.pop_front() {
                // Batch before the two-entry channel: punctuation need not
                // restart synthesis when more of the answer is already ready.
                let mut characters = text.chars().count();
                while let Some(next) = queued.front() {
                    if !append_segment(&mut text, &mut characters, next) {
                        break;
                    }
                    let _merged = queued.pop_front();
                }
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

    pub(super) fn playback_changed(&mut self) -> Option<bool> {
        let playing = self.playing.load(Ordering::Acquire);
        if playing == self.notified_playing {
            return None;
        }
        self.notified_playing = playing;
        Some(playing)
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
    playing: &Arc<AtomicBool>,
) -> Result<(), tts::Failure> {
    let sample_rate = client.sample_rate();
    let packet_bytes = packet_bytes_for_rate(sample_rate);
    let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
    let (recycle, recycled) = mpsc::sync_channel(RECYCLED_PACKETS);
    let output_stop = Arc::clone(stop);
    let output_current = Arc::clone(current);
    let output_playing = Arc::clone(playing);
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
                &output_playing,
            );
            if result.is_err() {
                output_stop.store(true, Ordering::Release);
            }
            result
        })
        .map_err(|_| AUDIO_FAILURE)?;
    let synthesis = (|| {
        let mut pending = None;
        let mut frames = SegmentFrames::reusing_with_packet_bytes(recycled, packet_bytes);
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
    while let Ok(next) = segments.try_recv() {
        check(generation, current, stop)?;
        if !append_segment(&mut text, &mut characters, &next) {
            *pending = Some(next);
            break;
        }
    }
    Ok(Some(text))
}

fn append_segment(text: &mut String, characters: &mut usize, next: &str) -> bool {
    // Preserve paragraph cues supplied by segmentation and charge only the
    // separator actually needed. An extra space can otherwise force a new
    // model request even when the original two pieces fit exactly.
    let needs_space = !text.ends_with(char::is_whitespace) && !next.starts_with(char::is_whitespace);
    let added = usize::from(needs_space) + next.chars().count();
    if *characters + added > super::MAX_SEGMENT {
        return false;
    }
    if needs_space {
        text.push(' ');
    }
    text.push_str(next);
    *characters += added;
    true
}

struct Frames {
    pending: Vec<u8>,
    recycled: Option<Receiver<Vec<u8>>>,
    packet_bytes: usize,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            pending: Vec::with_capacity(PACKET_BYTES),
            recycled: None,
            packet_bytes: PACKET_BYTES,
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
        }
    }

    fn replacement(&self) -> Vec<u8> {
        self.recycled
            .as_ref()
            .and_then(|recycled| recycled.try_recv().ok())
            .unwrap_or_else(|| Vec::with_capacity(self.packet_bytes))
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
            pcm = &pcm[count..];
            if self.pending.len() == self.packet_bytes {
                let replacement = self.replacement();
                sink(std::mem::replace(&mut self.pending, replacement))?;
            }
        }
        Ok(())
    }

    fn finish(
        &mut self,
        sink: impl FnOnce(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if self.pending.is_empty() {
            Ok(())
        } else if self.pending.len().is_multiple_of(2) {
            let replacement = self.replacement();
            sink(std::mem::replace(&mut self.pending, replacement))
        } else {
            Err(AUDIO_FAILURE)
        }
    }
}

// Smoothing belongs to synthesized segments, rather than codec/HTTP callbacks.
// PCM framing remains a separate byte-preserving operation with recycled packets.
struct SegmentFrames {
    frames: Frames,
    tail: [u8; MAX_EDGE_BYTES],
    tail_len: usize,
    split_byte: Option<u8>,
    onset_samples: usize,
}

impl Default for SegmentFrames {
    fn default() -> Self {
        Self::with_frames(Frames::default())
    }
}

impl SegmentFrames {
    fn with_frames(frames: Frames) -> Self {
        Self {
            frames,
            tail: [0; MAX_EDGE_BYTES],
            tail_len: 0,
            split_byte: None,
            onset_samples: 0,
        }
    }

    fn reusing_with_packet_bytes(recycled: Receiver<Vec<u8>>, packet_bytes: usize) -> Self {
        Self::with_frames(Frames::reusing_with_packet_bytes(recycled, packet_bytes))
    }

    fn edge_bytes(&self) -> usize {
        self.frames.packet_bytes / 20
    }

    fn retain_tail(
        &mut self,
        pcm: &[u8],
        sink: &mut impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        let release = (self.tail_len + pcm.len()).saturating_sub(self.edge_bytes());
        let old = release.min(self.tail_len);
        self.frames.push(&self.tail[..old], &mut *sink)?;
        self.tail.copy_within(old..self.tail_len, 0);
        self.tail_len -= old;
        let incoming = release - old;
        self.frames.push(&pcm[..incoming], sink)?;
        let remaining = &pcm[incoming..];
        self.tail[self.tail_len..self.tail_len + remaining.len()].copy_from_slice(remaining);
        self.tail_len += remaining.len();
        Ok(())
    }

    fn push_samples(
        &mut self,
        pcm: &[u8],
        sink: &mut impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        let edge_samples = self.edge_bytes() / 2;
        let onset_bytes = pcm.len().min((edge_samples - self.onset_samples) * 2);
        if onset_bytes > 0 {
            let mut onset = [0; MAX_EDGE_BYTES];
            for (input, output) in pcm[..onset_bytes]
                .chunks_exact(2)
                .zip(onset[..onset_bytes].chunks_exact_mut(2))
            {
                let sample = f32::from(i16::from_le_bytes([input[0], input[1]]));
                output.copy_from_slice(
                    &float_to_i16(sample * edge_gain(self.onset_samples, edge_samples))
                        .to_le_bytes(),
                );
                self.onset_samples += 1;
            }
            self.retain_tail(&onset[..onset_bytes], sink)?;
        }
        self.retain_tail(&pcm[onset_bytes..], sink)
    }

    fn push(
        &mut self,
        mut pcm: &[u8],
        mut sink: impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if pcm.is_empty() {
            return Ok(());
        }
        if let Some(first) = self.split_byte.take() {
            self.push_samples(&[first, pcm[0]], &mut sink)?;
            pcm = &pcm[1..];
        }
        let aligned = pcm.len() - pcm.len() % 2;
        self.push_samples(&pcm[..aligned], &mut sink)?;
        self.split_byte = pcm.get(aligned).copied();
        Ok(())
    }

    fn finish_segment(
        &mut self,
        mut sink: impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if self.split_byte.is_some() {
            return Err(AUDIO_FAILURE);
        }
        let count = self.tail_len / 2;
        for (index, sample) in self.tail[..self.tail_len].chunks_exact_mut(2).enumerate() {
            let value = f32::from(i16::from_le_bytes([sample[0], sample[1]]));
            sample.copy_from_slice(
                &float_to_i16(value * edge_gain(count - index - 1, count)).to_le_bytes(),
            );
        }
        self.frames.push(&self.tail[..self.tail_len], &mut sink)?;
        self.frames.finish(sink)?;
        self.tail_len = 0;
        self.onset_samples = 0;
        Ok(())
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
    playing: &AtomicBool,
) -> Result<(), tts::Failure> {
    consume_with_progress(
        incoming,
        generation,
        current,
        stop,
        &mut WaveOutput::open_with_rate(recycle, sample_rate)?,
        playing,
    )
}

trait Output {
    fn queued(&self) -> usize;
    fn push(&mut self, pcm: Vec<u8>) -> Result<(), tts::Failure>;
    fn reap(&mut self) -> Result<(), tts::Failure>;
    fn pause(&mut self) -> Result<(), tts::Failure>;
    fn start(&mut self) -> Result<(), tts::Failure>;
}

#[cfg(test)]
fn consume_with(
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    output: &mut impl Output,
) -> Result<(), tts::Failure> {
    consume_with_progress(
        incoming,
        generation,
        current,
        stop,
        output,
        &AtomicBool::new(false),
    )
}

fn consume_with_progress(
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    output: &mut impl Output,
    playing: &AtomicBool,
) -> Result<(), tts::Failure> {
    output.pause()?;
    playing.store(false, Ordering::Release);
    let mut started = false;
    let mut buffered = Buffered::default();
    let mut pending = None;
    let mut finished = false;
    loop {
        check(generation, current, stop)?;
        output.reap()?;
        if started
            && output.queued() == 0
            && buffered.ready.is_empty()
            && pending.is_none()
            && !finished
        {
            // A late consumer wake can find WinMM empty while the producer has
            // already queued more audio. Refill before latching slow synthesis;
            // otherwise scheduler jitter buffers the rest of a healthy segment.
            (pending, finished) =
                refill_queued(incoming, &mut buffered, generation, current, stop)?;
        }
        if started && output.queued() == 0 && buffered.ready.is_empty() {
            output.pause()?;
            if buffered.released {
                buffered.underrun();
            }
            started = false;
            playing.store(false, Ordering::Release);
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
                    playing.store(true, Ordering::Release);
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
                playing.store(true, Ordering::Release);
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

fn refill_queued(
    incoming: &Receiver<Audio>,
    buffered: &mut Buffered,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(Option<Audio>, bool), tts::Failure> {
    for _ in 0..PREFETCH_PACKETS {
        check(generation, current, stop)?;
        match incoming.try_recv() {
            Ok(Audio::Frame(pcm)) => {
                if let Some(pcm) = buffered.frame(pcm, Instant::now())? {
                    return Ok((Some(Audio::Frame(pcm)), false));
                }
            }
            Ok(Audio::SegmentEnd) => buffered.end()?,
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                if buffered.segment_bytes != 0 {
                    return Err(AUDIO_FAILURE);
                }
                return Ok((None, true));
            }
        }
        if !buffered.ready.is_empty() {
            break;
        }
    }
    Ok((None, false))
}

// At most one 120-second clip of application staging, plus the 16-second
// producer channel and 800 ms WinMM queue. Slow synthesis waits for a validated
// segment instead of repeatedly playing short bursts separated by silence.
#[derive(Default)]
#[expect(
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
            || !pcm.len().is_multiple_of(2)
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
                // Allow half a codec frame of observed jitter while rejecting sustained
                // slower-than-playback supply before releasing the first audio.
                self.slow |= now.saturating_duration_since(first) > duration + CODEC_BURST / 2;
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
    #[expect(
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
            || !pcm.len().is_multiple_of(2)
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
        Arc, AtomicBool, AtomicU64, Audio, Buffered, Duration, Frames, Instant, MAX_BUFFER_BYTES,
        Ordering, Output, PACKET_BYTES, PACKET_BYTES_48K, PLAYBACK_PACKETS, PREFETCH_PACKETS,
        PREROLL_PACKETS, SAMPLE_RATE, SAMPLE_RATE_48K, SegmentFrames, VecDeque, check,
        consume_with, consume_with_progress, mpsc, packet_bytes_for_rate, send,
    };
    use crate::tts::Failure;

    #[test]
    fn segment_edges_reach_silence_at_both_rates_without_changing_duration() -> Result<(), Failure>
    {
        assert_eq!(packet_bytes_for_rate(SAMPLE_RATE), PACKET_BYTES);
        assert_eq!(packet_bytes_for_rate(SAMPLE_RATE_48K), PACKET_BYTES_48K);
        for packet_bytes in [PACKET_BYTES, PACKET_BYTES_48K] {
            let edge_bytes = packet_bytes / 20;
            for bytes in [
                2,
                4,
                edge_bytes - 2,
                edge_bytes,
                edge_bytes + 2,
                packet_bytes - 2,
                packet_bytes,
                packet_bytes + 2,
                packet_bytes * 2 + 42,
            ] {
                let mut frames = SegmentFrames {
                    frames: Frames {
                        pending: Vec::with_capacity(packet_bytes),
                        packet_bytes,
                        ..Frames::default()
                    },
                    ..SegmentFrames::default()
                };
                // A subsequent opposite-polarity segment must start from its own
                // silence edge, including when the previous ending was packet aligned.
                for amplitude in [12_000_i16, -12_000] {
                    let source = amplitude.to_le_bytes().repeat(bytes / 2);
                    let mut packets = Vec::new();
                    frames.push(&source, |packet| {
                        packets.push(packet);
                        Ok(())
                    })?;
                    assert!(frames.tail_len <= edge_bytes);
                    frames.finish_segment(|packet| {
                        packets.push(packet);
                        Ok(())
                    })?;
                    assert!(packets.iter().all(|packet| packet.len() <= packet_bytes));
                    assert!(packets.iter().all(|packet| packet.len().is_multiple_of(2)));
                    let output = packets.concat();
                    assert_eq!(output.len(), source.len());
                    assert_eq!(&output[..2], &[0, 0]);
                    assert_eq!(&output[output.len() - 2..], &[0, 0]);
                    if bytes >= edge_bytes * 2 {
                        assert_eq!(
                            &output[edge_bytes..bytes - edge_bytes],
                            &source[edge_bytes..bytes - edge_bytes]
                        );
                        assert_eq!(
                            &output[edge_bytes - 2..edge_bytes],
                            &amplitude.to_le_bytes()
                        );
                        assert_eq!(
                            &output[bytes - edge_bytes..bytes - edge_bytes + 2],
                            &amplitude.to_le_bytes()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn segment_onset_smoothing_is_identical_across_fragmented_pcm_callbacks() -> Result<(), Failure>
    {
        fn render(
            packet_bytes: usize,
            samples: usize,
            fragment: usize,
        ) -> Result<Vec<u8>, Failure> {
            let mut frames = SegmentFrames {
                frames: Frames {
                    packet_bytes,
                    ..Frames::default()
                },
                ..SegmentFrames::default()
            };
            let mut output = Vec::new();
            // The first utterance edge and both following transitions share the
            // same policy. Odd fragments exercise samples split across callbacks.
            for polarity in [1_i16, -1, 1] {
                let pcm: Vec<u8> = (0_i16..180)
                    .cycle()
                    .take(samples)
                    .flat_map(|index| (polarity * (12_000 - index * 100)).to_le_bytes())
                    .collect();
                for bytes in pcm.chunks(fragment) {
                    frames.push(bytes, |packet| {
                        output.extend(packet);
                        Ok(())
                    })?;
                }
                frames.finish_segment(|packet| {
                    output.extend(packet);
                    Ok(())
                })?;
            }
            Ok(output)
        }

        for packet_bytes in [PACKET_BYTES, PACKET_BYTES_48K] {
            for samples in [180, packet_bytes / 2, packet_bytes / 2 + 1] {
                let contiguous = render(packet_bytes, samples, usize::MAX)?;
                for fragment in [1, 2, 3, 17, 128, packet_bytes - 1, packet_bytes] {
                    assert_eq!(render(packet_bytes, samples, fragment)?, contiguous);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn segment_edges_hold_only_five_milliseconds_and_reject_a_split_final_sample()
    -> Result<(), Failure> {
        let mut frames = SegmentFrames::default();
        let mut output = Vec::new();
        frames.push(&vec![0; PACKET_BYTES + PACKET_BYTES / 20], |packet| {
            output.extend(packet);
            Ok(())
        })?;
        assert_eq!(output.len(), PACKET_BYTES);
        assert!(frames.frames.pending.is_empty());
        assert_eq!(frames.tail_len, PACKET_BYTES / 20);
        frames.push(&[7], |_| Err(super::AUDIO_FAILURE))?;
        assert_eq!(frames.finish_segment(|_| Ok(())), Err(super::AUDIO_FAILURE));
        assert_eq!(output.len(), PACKET_BYTES);
        Ok(())
    }

    #[test]
    fn segment_edges_allow_realtime_codec_supply_to_prime_at_both_rates() -> Result<(), Failure> {
        let now = Instant::now();
        for packet_bytes in [PACKET_BYTES, PACKET_BYTES_48K] {
            let mut frames = SegmentFrames::with_frames(Frames {
                packet_bytes,
                ..Frames::default()
            });
            let mut buffered = Buffered::default();
            // Tail holdback can defer a full packet until the next 80ms codec
            // callback. It must still satisfy the existing supply jitter bound.
            let codec_frame = vec![0; packet_bytes * 4 / 5];
            for tick in 1..=10 {
                frames.push(&codec_frame, |packet| {
                    assert!(
                        buffered
                            .frame(packet, now + Duration::from_millis(tick * 80))?
                            .is_none()
                    );
                    Ok(())
                })?;
                assert!(!buffered.slow);
                if tick == 6 {
                    assert_eq!(buffered.ready.len(), PREROLL_PACKETS);
                }
            }
            frames.finish_segment(|packet| {
                assert!(
                    buffered
                        .frame(packet, now + Duration::from_millis(800))?
                        .is_none()
                );
                Ok(())
            })?;
            buffered.end()?;
            assert_eq!(buffered.ready.len(), 8);
        }
        Ok(())
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
            "Hello another clause. Separate sentence.".to_owned(),
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
    fn paragraph_cues_survive_both_coalescing_stages_at_the_character_limit()
    -> Result<(), &'static str> {
        let now = Instant::now();
        let input = format!("{}.\n\nSecond. tail", "a".repeat(246));
        let expected = format!("{}.\n\nSecond.", "a".repeat(246));
        assert_eq!(expected.chars().count(), super::super::MAX_SEGMENT);
        let mut text = super::super::stream::Text::default();
        text.append(&input, now)?;
        let mut queued = text.ready(now);
        assert_eq!(queued.len(), 2);

        // Request intake must keep the same paragraph and fit the original cap.
        let (sender, receiver) = mpsc::sync_channel(2);
        for piece in &queued {
            sender.send(piece.clone()).map_err(|_| "Closed queue")?;
        }
        assert_eq!(
            super::next_segment(
                &receiver,
                &mut None,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
            ),
            Ok(Some(expected.clone()))
        );

        // The producer's earlier coalescing stage follows the same rule.
        let (_complete, finished) = mpsc::channel();
        let mut pipeline = super::Pipeline {
            segments: Some(sender),
            finished,
            cancelled: Arc::new(AtomicBool::new(false)),
            playing: Arc::new(AtomicBool::new(false)),
            notified_playing: false,
            worker: None,
        };
        pipeline.feed(&mut queued, true);
        assert_eq!(receiver.try_recv(), Ok(expected));
        assert!(queued.is_empty());
        Ok(())
    }

    #[test]
    fn punctuation_batches_before_the_bounded_channel_and_preserves_backpressure() {
        let (sender, incoming) = mpsc::sync_channel(2);
        let (_complete, finished) = mpsc::channel();
        let mut pipeline = super::Pipeline {
            segments: Some(sender),
            finished,
            cancelled: Arc::new(AtomicBool::new(false)),
            playing: Arc::new(AtomicBool::new(false)),
            notified_playing: false,
            worker: None,
        };
        let sentence = "One short sentence.";
        let per_request = (super::super::MAX_SEGMENT + 1) / (sentence.len() + 1);
        let expected = vec![sentence; per_request].join(" ");
        let mut queued = VecDeque::from(vec![sentence.to_owned(); per_request * 4]);
        pipeline.feed(&mut queued, true);
        // Two full requests enter the channel; all remaining text is retained.
        assert_eq!(incoming.try_recv(), Ok(expected.clone()));
        assert_eq!(incoming.try_recv(), Ok(expected.clone()));
        assert!(incoming.try_recv().is_err());
        assert!(pipeline.segments.is_some());
        pipeline.feed(&mut queued, true);
        assert_eq!(incoming.try_recv(), Ok(expected.clone()));
        assert_eq!(incoming.try_recv(), Ok(expected));
        assert!(queued.is_empty());
        assert!(pipeline.segments.is_none());
        assert!(incoming.try_recv().is_err());
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
    fn realtime_codec_frames_prime_with_jitter_and_reject_slow_supply() -> Result<(), Failure> {
        let now = Instant::now() + Duration::from_secs(90);
        let mut buffered = Buffered::default();
        let mut frames = Frames::default();
        for millis in [80, 160, 240, 360, 400, 480, 560, 640, 720, 800] {
            frames.push(&[0; 3_840], |frame| {
                assert!(
                    buffered
                        .frame(frame, now + Duration::from_millis(millis))?
                        .is_none()
                );
                Ok(())
            })?;
            if millis == 400 {
                assert_eq!(buffered.ready.len(), PREROLL_PACKETS);
            }
        }
        assert!(!buffered.slow);
        assert_eq!(buffered.ready.len(), 8);
        for interval in [120, 160] {
            let mut slow = Buffered::default();
            let mut frames = Frames::default();
            for index in 1..=5 {
                frames.push(&[0; 3_840], |frame| {
                    assert!(
                        slow.frame(frame, now + Duration::from_millis(index * interval))?
                            .is_none()
                    );
                    Ok(())
                })?;
            }
            assert!(slow.slow);
            assert!(slow.ready.is_empty());
            slow.end()?;
            assert_eq!(slow.ready.len(), PREROLL_PACKETS);
        }
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
        progress: Option<Arc<AtomicBool>>,
        playing_observations: usize,
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
                if let Some(progress) = &self.progress {
                    assert!(progress.load(Ordering::Acquire));
                    self.playing_observations += 1;
                }
                self.played.extend(pcm);
            }
            Ok(())
        }

        fn pause(&mut self) -> Result<(), Failure> {
            self.paused = true;
            Ok(())
        }

        fn start(&mut self) -> Result<(), Failure> {
            if let Some(progress) = &self.progress {
                assert!(!progress.load(Ordering::Acquire));
            }
            self.starts.push(self.queued.len());
            self.paused = false;
            Ok(())
        }
    }

    #[test]
    fn queued_producer_audio_refills_before_a_late_wake_can_latch_an_underrun()
    -> Result<(), Failure> {
        struct DelayedOutput {
            output: RecordingOutput,
            producer: Option<mpsc::SyncSender<Audio>>,
        }

        impl Output for DelayedOutput {
            fn queued(&self) -> usize {
                self.output.queued()
            }

            fn push(&mut self, pcm: Vec<u8>) -> Result<(), Failure> {
                self.output.push(pcm)
            }

            fn reap(&mut self) -> Result<(), Failure> {
                if !self.output.paused
                    && let Some(producer) = self.producer.take()
                {
                    // Simulate a late consumer wake after the native queue has
                    // drained, with subsequent audio already in the channel.
                    while let Some(pcm) = self.output.queued.pop_front() {
                        self.output.played.extend(pcm);
                    }
                    for _ in 0..PREROLL_PACKETS {
                        producer
                            .send(Audio::Frame(vec![9; PACKET_BYTES]))
                            .map_err(|_| super::AUDIO_FAILURE)?;
                    }
                    producer
                        .send(Audio::SegmentEnd)
                        .map_err(|_| super::AUDIO_FAILURE)?;
                    Ok(())
                } else {
                    self.output.reap()
                }
            }

            fn pause(&mut self) -> Result<(), Failure> {
                self.output.pause()
            }

            fn start(&mut self) -> Result<(), Failure> {
                self.output.start()
            }
        }

        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        for _ in 0..PREROLL_PACKETS {
            sender
                .send(Audio::Frame(vec![7; PACKET_BYTES]))
                .map_err(|_| super::AUDIO_FAILURE)?;
        }
        let mut output = DelayedOutput {
            output: RecordingOutput::default(),
            producer: Some(sender),
        };
        consume_with(
            &incoming,
            4,
            &AtomicU64::new(4),
            &AtomicBool::new(false),
            &mut output,
        )?;
        assert_eq!(output.output.starts, vec![PREROLL_PACKETS]);
        let mut expected = vec![7; PACKET_BYTES * PREROLL_PACKETS];
        expected.extend(vec![9; PACKET_BYTES * PREROLL_PACKETS]);
        assert_eq!(output.output.played, expected);
        assert!(output.output.maximum <= PLAYBACK_PACKETS);
        Ok(())
    }

    #[test]
    fn playback_progress_tracks_output_start_and_buffering_without_an_audio_device()
    -> Result<(), Failure> {
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        assert!(sender.send(Audio::Frame(vec![7; PACKET_BYTES])).is_ok());
        assert!(sender.send(Audio::SegmentEnd).is_ok());
        drop(sender);
        let progress = Arc::new(AtomicBool::new(true));
        let mut output = RecordingOutput {
            progress: Some(Arc::clone(&progress)),
            ..RecordingOutput::default()
        };
        consume_with_progress(
            &incoming,
            4,
            &AtomicU64::new(4),
            &AtomicBool::new(false),
            &mut output,
            &progress,
        )?;
        assert_eq!(output.playing_observations, 1);
        assert!(!progress.load(Ordering::Acquire));
        Ok(())
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
        assert_eq!(multi_segment_output.starts, vec![PREROLL_PACKETS]);
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
        // 1. A 48 kHz sentence's partial tail must drain without hanging paused;
        // the next queued sentence continues through its punctuation boundary.
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        let mut frames_48k = SegmentFrames {
            frames: Frames {
                packet_bytes: PACKET_BYTES_48K,
                ..Frames::default()
            },
            ..SegmentFrames::default()
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
        assert_eq!(output_48k.starts, vec![PREROLL_PACKETS]);

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
