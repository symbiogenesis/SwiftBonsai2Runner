//! One asynchronous Media Foundation reader per imported file.
//!
//! Only PCM bytes cross the callback boundary. The reader retains one bounded
//! frame suffix, and requests no new frame until the caller needs more PCM.

use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::Media::MediaFoundation::{
        IMFAttributes, IMFMediaBuffer, IMFMediaEvent, IMFMediaType, IMFSample, IMFSourceReader,
        IMFSourceReaderCallback, IMFSourceReaderCallback_Impl, MFAudioFormat_PCM,
        MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
        MFSTARTUP_FULL, MFShutdown, MFStartup, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
        MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
        MF_SOURCE_READER_ASYNC_CALLBACK, MF_SOURCE_READERF_ALLEFFECTSREMOVED,
        MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
        MF_SOURCE_READERF_ERROR, MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED,
        MF_SOURCE_READERF_NEWSTREAM, MF_SOURCE_READERF_STREAMTICK, MF_VERSION,
    },
    core::{HRESULT, PCWSTR, Ref, implement},
};

use super::{
    CANCELLED, CHUNK_TOO_LONG, CODEC_FAILURE, MAX_CONVERTED_BYTES, MAX_SAMPLES, MEMORY_FAILURE,
    POLL, Shared, check_memory, media_duration,
};

// These DWORD sentinels are the unsigned ABI representations of the SDK's
// negative MF_SOURCE_READER_* stream-index constants.
const AUDIO_STREAM: u32 = 0xffff_fffd;
const ALL_STREAMS: u32 = 0xffff_fffe;
const MEDIA_SOURCE: u32 = 0xffff_ffff;
const FLUSH_LIMIT: Duration = Duration::from_secs(2);
const DECODE_TIMEOUT: &str = "Windows did not finish decoding the next audio segment. Try converting the recording to WAV. Your draft was kept.";
const FORMAT_CHANGED: &str = "The recording changed audio format or contains a timeline gap. Try converting it to WAV. Your draft was kept.";
const DURATION_FAILURE: &str = "Windows could not determine this recording's duration. Try converting it to WAV. Your draft was kept.";
const CLEANUP_FAILURE: &str = "Windows could not confirm audio decoder shutdown. Restart the app before importing another audio file. Your draft was kept.";
static ADMISSION_CLOSED: AtomicBool = AtomicBool::new(false);
const REJECTED_FLAGS: u32 = MF_SOURCE_READERF_NEWSTREAM.0.unsigned_abs()
    | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0.unsigned_abs()
    | MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0.unsigned_abs()
    | MF_SOURCE_READERF_STREAMTICK.0.unsigned_abs()
    | MF_SOURCE_READERF_ALLEFFECTSREMOVED.0.unsigned_abs();

struct Frame {
    pcm: Vec<u8>,
    eof: bool,
}

#[derive(Default)]
struct CallbackState {
    accepting: AtomicBool,
    failed: AtomicBool,
    flushed: AtomicBool,
    active: AtomicUsize,
}

struct Activity<'a>(&'a AtomicUsize);

impl<'a> Activity<'a> {
    fn enter(active: &'a AtomicUsize) -> Self {
        active.fetch_add(1, Ordering::AcqRel);
        Self(active)
    }
}

impl Drop for Activity<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

#[implement(IMFSourceReaderCallback)]
struct Callback {
    state: Arc<CallbackState>,
    frames: SyncSender<Result<Frame, &'static str>>,
}

#[expect(non_snake_case, reason = "The COM callback trait uses SDK method names.")]
impl IMFSourceReaderCallback_Impl for Callback_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        _timestamp: i64,
        sample: Ref<'_, IMFSample>,
    ) -> windows::core::Result<()> {
        let _active = Activity::enter(&self.state.active);
        if !self.state.accepting.load(Ordering::Acquire) {
            return Ok(());
        }
        let result = callback_frame(status, flags, sample.as_ref());
        // Never block a Media Foundation callback on the consumer or shutdown.
        // Only one ReadSample is outstanding, so a full slot is an error.
        if self.frames.try_send(result).is_err() {
            self.state.failed.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn OnFlush(&self, _stream: u32) -> windows::core::Result<()> {
        let _active = Activity::enter(&self.state.active);
        self.state.flushed.store(true, Ordering::Release);
        Ok(())
    }

    fn OnEvent(&self, _stream: u32, _event: Ref<'_, IMFMediaEvent>) -> windows::core::Result<()> {
        let _active = Activity::enter(&self.state.active);
        // Source events are informational here. Read errors arrive through the
        // outstanding OnReadSample result. Retain no event COM references.
        Ok(())
    }
}

fn callback_frame(
    status: HRESULT,
    flags: u32,
    sample: Option<&IMFSample>,
) -> Result<Frame, &'static str> {
    status.ok().map_err(|_| CODEC_FAILURE)?;
    if flags & MF_SOURCE_READERF_ERROR.0.unsigned_abs() != 0 {
        return Err(CODEC_FAILURE);
    }
    if flags & REJECTED_FLAGS != 0 {
        return Err(FORMAT_CHANGED);
    }
    Ok(Frame {
        pcm: sample.map_or_else(|| Ok(Vec::new()), copy_sample)?,
        eof: flags & MF_SOURCE_READERF_ENDOFSTREAM.0.unsigned_abs() != 0,
    })
}

struct LockedBuffer(IMFMediaBuffer);

impl Drop for LockedBuffer {
    fn drop(&mut self) {
        // SAFETY: This guard is created only after a successful Lock, and owns
        // the interface until its single matching Unlock.
        let _unlocked = unsafe { self.0.Unlock() };
    }
}

fn copy_sample(sample: &IMFSample) -> Result<Vec<u8>, &'static str> {
    // SAFETY: The callback lends a valid sample for this call; no COM pointer
    // leaves its callback thread or outlives that borrowed reference.
    let length = unsafe { sample.GetTotalLength() }.map_err(|_| CODEC_FAILURE)?;
    let length = usize::try_from(length).map_err(|_| CHUNK_TOO_LONG)?;
    if length == 0 || !length.is_multiple_of(2) {
        return Err(CODEC_FAILURE);
    }
    if u64::try_from(length).map_err(|_| CHUNK_TOO_LONG)? > MAX_CONVERTED_BYTES {
        return Err(CHUNK_TOO_LONG);
    }
    // Flattening can allocate another complete frame before the copy does.
    check_memory(u64::try_from(length).map_err(|_| CHUNK_TOO_LONG)? * 2)?;
    // SAFETY: The source sample stays live throughout bounded flattening.
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|_| CODEC_FAILURE)?;
    let mut data = ptr::null_mut();
    let mut actual = 0;
    // SAFETY: The live buffer receives valid stack output pointers. Unlock is
    // paired by the guard immediately after this successful call.
    unsafe { buffer.Lock(&raw mut data, None, Some(&raw mut actual)) }
        .map_err(|_| CODEC_FAILURE)?;
    let _locked = LockedBuffer(buffer);
    if data.is_null() || usize::try_from(actual).map_err(|_| CHUNK_TOO_LONG)? != length {
        return Err(CODEC_FAILURE);
    }
    let mut pcm = Vec::new();
    pcm.try_reserve_exact(length).map_err(|_| MEMORY_FAILURE)?;
    // SAFETY: Lock supplies actual initialized bytes, checked to equal the
    // bounded sample length. The buffer remains locked through this copy.
    pcm.extend_from_slice(unsafe { std::slice::from_raw_parts(data, length) });
    Ok(pcm)
}

struct Platform {
    shutdown: bool,
}

impl Platform {
    fn start() -> Result<Self, &'static str> {
        // SAFETY: The caller initialized COM on this import worker. This guard
        // balances the platform reference after reader/callback teardown.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|_| CODEC_FAILURE)?;
        Ok(Self { shutdown: true })
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        if self.shutdown {
            // SAFETY: All reader interfaces were released and any pending read
            // callback completed before this matching platform shutdown.
            let _shutdown = unsafe { MFShutdown() };
        }
    }
}

pub(super) struct Decoder {
    reader: Option<IMFSourceReader>,
    receiver: Receiver<Result<Frame, &'static str>>,
    state: Arc<CallbackState>,
    buffered: PcmBuffer,
    total: Duration,
    // Last field: shutdown must follow reader and callback-owned data release.
    platform: Platform,
}

impl Decoder {
    pub(super) fn open(
        path: &Path,
        shared: &Shared,
        deadline: Instant,
    ) -> Result<Self, &'static str> {
        check(shared, deadline)?;
        if ADMISSION_CLOSED.load(Ordering::Acquire) {
            return Err(CLEANUP_FAILURE);
        }
        if path.as_os_str().encode_wide().any(|unit| unit == 0) {
            return Err(CODEC_FAILURE);
        }
        let platform = Platform::start()?;
        let (frames, receiver) = mpsc::sync_channel(1);
        let state = Arc::new(CallbackState::default());
        state.accepting.store(true, Ordering::Release);
        let callback: IMFSourceReaderCallback = Callback {
            state: Arc::clone(&state),
            frames,
        }
        .into();
        let mut attributes = None;
        // SAFETY: Valid stack output storage receives an owned attribute store.
        unsafe { MFCreateAttributes(&raw mut attributes, 1) }.map_err(|_| CODEC_FAILURE)?;
        let attributes: IMFAttributes = attributes.ok_or(CODEC_FAILURE)?;
        // SAFETY: The attribute store AddRefs the live callback; its fields are
        // thread-safe and callbacks never lend COM interfaces across threads.
        unsafe { attributes.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback) }
            .map_err(|_| CODEC_FAILURE)?;
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        check(shared, deadline)?;
        // SAFETY: name is NUL terminated and live for this synchronous local-file
        // open. OS open/configuration/teardown calls cannot be preempted; check
        // the same deadline immediately after they return.
        let reader = unsafe { MFCreateSourceReaderFromURL(PCWSTR(name.as_ptr()), &attributes) }
            .map_err(|_| CODEC_FAILURE)?;
        check(shared, deadline)?;
        configure(&reader)?;
        check(shared, deadline)?;
        // SAFETY: This reader owns its presentation metadata; the returned
        // PROPVARIANT owns and automatically clears its native storage.
        let duration = unsafe { reader.GetPresentationAttribute(MEDIA_SOURCE, &MF_PD_DURATION) }
            .map_err(|_| DURATION_FAILURE)?;
        let duration = i64::try_from(&duration).map_err(|_| DURATION_FAILURE)?;
        if duration <= 0 {
            return Err(DURATION_FAILURE);
        }
        let total = media_duration(duration)?;
        check(shared, deadline)?;
        Ok(Self {
            reader: Some(reader),
            receiver,
            state,
            buffered: PcmBuffer::default(),
            total,
            platform,
        })
    }

    pub(super) const fn total(&self) -> Duration {
        self.total
    }

    pub(super) fn read(
        &mut self,
        max_samples: usize,
        shared: &Shared,
        deadline: Instant,
    ) -> Result<(Vec<u8>, bool), &'static str> {
        if max_samples == 0 || max_samples > MAX_SAMPLES {
            return Err(CHUNK_TOO_LONG);
        }
        let capacity = max_samples * 2;
        check(shared, deadline)?;
        check_memory(u64::try_from(capacity).map_err(|_| CHUNK_TOO_LONG)?)?;
        let reader = self.reader.as_ref().ok_or(CODEC_FAILURE)?;
        let (pcm, eof) = self.buffered.read(capacity, shared, deadline, &mut || {
            next_frame(reader, &self.receiver, &self.state, shared, deadline)
        })?;
        if eof {
            self.close()?;
            check(shared, deadline)?;
        }
        Ok((pcm, eof))
    }

    fn close(&mut self) -> Result<(), &'static str> {
        self.state.accepting.store(false, Ordering::Release);
        let Some(reader) = self.reader.take() else {
            return Ok(());
        };
        self.state.flushed.store(false, Ordering::Release);
        // SAFETY: The reader is live and no later read will be submitted.
        // Async Flush invalidates pending reads and acknowledges OnFlush.
        let flushing = unsafe { reader.Flush(ALL_STREAMS) }.is_ok();
        let deadline = Instant::now() + FLUSH_LIMIT;
        while flushing
            && (!self.state.flushed.load(Ordering::Acquire)
                || self.state.active.load(Ordering::Acquire) != 0)
            && Instant::now() < deadline
        {
            thread::sleep(POLL);
        }
        let quiet = flushing
            && self.state.flushed.load(Ordering::Acquire)
            && self.state.active.load(Ordering::Acquire) == 0;
        drop(reader);
        // A failed/timed-out flush cannot establish native quiescence. Retain
        // this one platform reference instead of shutting MF down under native
        // work. Close admission until restart so failures cannot accumulate refs.
        self.platform.shutdown = quiet;
        if quiet {
            Ok(())
        } else {
            ADMISSION_CLOSED.store(true, Ordering::Release);
            Err(CLEANUP_FAILURE)
        }
    }
}

#[derive(Default)]
struct PcmBuffer {
    pending: Vec<u8>,
    offset: usize,
    eof: bool,
}

impl PcmBuffer {
    fn read(
        &mut self,
        capacity: usize,
        shared: &Shared,
        deadline: Instant,
        next: &mut impl FnMut() -> Result<Frame, &'static str>,
    ) -> Result<(Vec<u8>, bool), &'static str> {
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(capacity).map_err(|_| MEMORY_FAILURE)?;
        loop {
            check(shared, deadline)?;
            let available = self.pending.len() - self.offset;
            let count = available.min(capacity - pcm.len());
            pcm.extend_from_slice(&self.pending[self.offset..self.offset + count]);
            self.offset += count;
            if self.offset != self.pending.len() {
                return Ok((pcm, false));
            }
            self.pending.clear();
            self.offset = 0;
            if self.eof {
                return Ok((pcm, true));
            }
            // Read one bounded frame ahead when PCM exactly fills capacity.
            // This distinguishes a true exact-boundary EOF from another frame
            // without emitting an empty final recognizer request.
            let frame = next()?;
            self.pending = frame.pcm;
            self.eof = frame.eof;
            if pcm.len() == capacity && (!self.pending.is_empty() || self.eof) {
                return Ok((pcm, self.eof && self.pending.is_empty()));
            }
        }
    }
}

fn next_frame(
    reader: &IMFSourceReader,
    receiver: &Receiver<Result<Frame, &'static str>>,
    state: &CallbackState,
    shared: &Shared,
    deadline: Instant,
) -> Result<Frame, &'static str> {
    check(shared, deadline)?;
    check_memory(MAX_CONVERTED_BYTES * 2)?;
    // SAFETY: This reader was created with an async callback; all four
    // optional synchronous output parameters are NULL. Only one read is
    // submitted before consuming its corresponding callback message.
    unsafe { reader.ReadSample(AUDIO_STREAM, 0, None, None, None, None) }
        .map_err(|_| CODEC_FAILURE)?;
    loop {
        check(shared, deadline)?;
        if state.failed.load(Ordering::Acquire) {
            return Err(CODEC_FAILURE);
        }
        match receiver.recv_timeout(POLL.min(deadline.saturating_duration_since(Instant::now()))) {
            Ok(frame) => {
                check(shared, deadline)?;
                return frame;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(CODEC_FAILURE),
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _closed = self.close();
    }
}

fn check(shared: &Shared, deadline: Instant) -> Result<(), &'static str> {
    if shared.cancelled.load(Ordering::Acquire) {
        Err(CANCELLED)
    } else if Instant::now() >= deadline {
        Err(DECODE_TIMEOUT)
    } else {
        Ok(())
    }
}

fn configure(reader: &IMFSourceReader) -> Result<(), &'static str> {
    // SAFETY: Select only the audio stream on this exclusively owned reader.
    unsafe { reader.SetStreamSelection(ALL_STREAMS, false) }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: The SDK sentinel chooses this file's first audio stream.
    unsafe { reader.SetStreamSelection(AUDIO_STREAM, true) }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: Receives a new owned empty media-type attribute store.
    let format = unsafe { MFCreateMediaType() }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: Attribute keys and GUID pointers are static SDK values.
    unsafe { format.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio) }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: This static subtype requests signed integer PCM audio.
    unsafe { format.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM) }.map_err(|_| CODEC_FAILURE)?;
    for (key, value) in [
        (&MF_MT_AUDIO_NUM_CHANNELS, 1),
        (&MF_MT_AUDIO_SAMPLES_PER_SECOND, 16_000),
        (&MF_MT_AUDIO_BITS_PER_SAMPLE, 16),
        (&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2),
        (&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 32_000),
    ] {
        // SAFETY: The owned attribute store borrows a valid static key.
        unsafe { format.SetUINT32(key, value) }.map_err(|_| CODEC_FAILURE)?;
    }
    // SAFETY: No sample request exists during this one-time format negotiation.
    unsafe { reader.SetCurrentMediaType(AUDIO_STREAM, None, &format) }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: Returns the owned negotiated output type of the selected stream.
    let actual = unsafe { reader.GetCurrentMediaType(AUDIO_STREAM) }.map_err(|_| CODEC_FAILURE)?;
    validate_format(&actual)
}

fn validate_format(format: &IMFMediaType) -> Result<(), &'static str> {
    // SAFETY: The live negotiated type receives no mutations during validation.
    let major = unsafe { format.GetGUID(&MF_MT_MAJOR_TYPE) }.map_err(|_| CODEC_FAILURE)?;
    // SAFETY: GetGUID returns an owned value from this live type.
    let subtype = unsafe { format.GetGUID(&MF_MT_SUBTYPE) }.map_err(|_| CODEC_FAILURE)?;
    if major != MFMediaType_Audio || subtype != MFAudioFormat_PCM {
        return Err(CODEC_FAILURE);
    }
    for (key, expected) in [
        (&MF_MT_AUDIO_NUM_CHANNELS, 1),
        (&MF_MT_AUDIO_SAMPLES_PER_SECOND, 16_000),
        (&MF_MT_AUDIO_BITS_PER_SAMPLE, 16),
        (&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2),
        (&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 32_000),
    ] {
        // SAFETY: The static key names a scalar in the live negotiated type.
        if unsafe { format.GetUINT32(key) }.map_err(|_| CODEC_FAILURE)? != expected {
            return Err(CODEC_FAILURE);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CANCELLED, CODEC_FAILURE, DECODE_TIMEOUT, Decoder, FORMAT_CHANGED, Frame,
        HRESULT, MAX_SAMPLES, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_STREAMTICK, PcmBuffer,
        Shared, callback_frame,
    };
    use crate::audio_import::Apartment;
    use std::{
        fs,
        io::{self, Write},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct WaveFixture(PathBuf);

    impl Drop for WaveFixture {
        fn drop(&mut self) {
            let _removed = fs::remove_file(&self.0);
        }
    }

    fn decode_wave(samples: usize, request_samples: usize) -> io::Result<()> {
        let _apartment = Apartment::initialize(false).map_err(io::Error::other)?;
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bonsai-sequential-audio-{}-{id}.wav",
            std::process::id()
        ));
        let mut original = Vec::with_capacity(samples * 2);
        for index in 0..samples {
            let sample = i16::try_from(index % 32_000).map_err(io::Error::other)? - 16_000;
            original.extend_from_slice(&sample.to_le_bytes());
        }
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, samples)
            .ok_or_else(|| io::Error::other("Invalid decoder WAV fixture"))?
            .to_vec();
        wave.extend_from_slice(&original);
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
        let fixture = WaveFixture(path);
        file.write_all(&wave)?;
        drop(file);
        let shared = Shared::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut decoder = Decoder::open(&fixture.0, &shared, deadline).map_err(io::Error::other)?;
        let mut received = Vec::new();
        let mut requests = 0;
        loop {
            let (pcm, eof) = decoder
                .read(request_samples, &shared, deadline)
                .map_err(io::Error::other)?;
            assert!(!pcm.is_empty());
            assert!(pcm.len() <= request_samples * 2);
            received.extend_from_slice(&pcm);
            requests += 1;
            assert_eq!(eof, received.len() == original.len());
            if eof {
                break;
            }
        }
        assert_eq!(requests, samples.div_ceil(request_samples));
        assert_eq!(received, original);
        // Drop the native reader/platform before removing its source fixture.
        drop(decoder);
        Ok(())
    }

    #[test]
    fn sequential_decoder_native_pcm_preserves_frame_suffix_and_exact_eof() -> io::Result<()> {
        decode_wave(64, 32)?;
        decode_wave(65, 32)
    }

    #[test]
    fn sequential_decoder_native_pcm_preserves_request_boundary_and_final_tail() -> io::Result<()> {
        decode_wave(MAX_SAMPLES, MAX_SAMPLES)?;
        decode_wave(MAX_SAMPLES + 17, MAX_SAMPLES)
    }

    #[test]
    fn sequential_decoder_cancellation_and_deadline_precede_reading_more_frames() {
        let shared = Shared::default();
        let mut buffered = PcmBuffer::default();
        let mut source = || Err("The source must not be read.");
        shared.cancelled.store(true, Ordering::Release);
        assert_eq!(
            buffered.read(32, &shared, Instant::now() + Duration::from_secs(1), &mut source),
            Err(CANCELLED)
        );
        shared.cancelled.store(false, Ordering::Release);
        assert_eq!(
            buffered.read(32, &shared, Instant::now(), &mut source),
            Err(DECODE_TIMEOUT)
        );
    }

    #[test]
    fn sequential_decoder_eof_payload_is_preserved_and_gaps_fail_visibly() -> io::Result<()> {
        let shared = Shared::default();
        let mut buffered = PcmBuffer::default();
        let mut frames = [Frame {
            pcm: vec![1, 2, 3, 4, 5, 6],
            eof: true,
        }]
        .into_iter();
        let mut source = || frames.next().ok_or("Unexpected additional decoder read.");
        let deadline = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            buffered.read(4, &shared, deadline, &mut source),
            Ok((vec![1, 2, 3, 4], false))
        );
        assert_eq!(
            buffered.read(4, &shared, deadline, &mut source),
            Ok((vec![5, 6], true))
        );
        let eof = callback_frame(
            HRESULT(0),
            MF_SOURCE_READERF_ENDOFSTREAM.0.unsigned_abs(),
            None,
        )
        .map_err(io::Error::other)?;
        assert!(eof.eof && eof.pcm.is_empty());
        assert!(matches!(
            callback_frame(HRESULT(0), MF_SOURCE_READERF_STREAMTICK.0.unsigned_abs(), None),
            Err(FORMAT_CHANGED)
        ));
        assert!(matches!(
            callback_frame(HRESULT(-1), 0, None),
            Err(CODEC_FAILURE)
        ));
        Ok(())
    }
}
