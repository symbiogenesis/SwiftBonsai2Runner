//! Native file selection, clipboard paste and bounded audio conversion.
#![expect(
    clippy::as_conversions,
    reason = "Win32 HWND and callback cookies preserve their pointer-sized address bits."
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Only the window uses this native boundary."
)]

use std::{
    ffi::OsString,
    io,
    marker::PhantomData,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::Win32::System::WinRT::{
    RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        },
        Ole::CF_HDROP,
    },
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_DONTADDTORECENT, OFN_ENABLEHOOK,
            OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
        Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_INSERT, VK_SHIFT},
        Shell::{DefSubclassProc, DragQueryFileW, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GetParent, PostMessageW, WM_CLOSE, WM_INITDIALOG, WM_KEYDOWN, WM_NCDESTROY,
        },
    },
};

mod decoder;
mod isolated;
mod segments;
use decoder::Decoder;
use segments::{
    CHUNK_TOO_LONG, CODEC_FAILURE, MAX_SAMPLES, MEMORY_FAILURE, TICKS_PER_SECOND, pause_segment,
    sample_duration,
};

const CANCELLED: &str = "Audio import cancelled.";
const MAX_CONVERTED_BYTES: u64 = 16_000 * 2 * 30 + 65_536;
// Budget the callback sample/copy, one retained decoder frame, readback PCM,
// pause suffix and canonical WAV, plus the preceding WAV being transcribed.
// The isolated launcher additionally reserves the helper's complete Job commit
// allowance and pipe buffers; the parent receives directly into its final WAV.
// No complete-file audio allocation is made.
const CHUNK_WORKING_BYTES: u64 = 6 * MAX_CONVERTED_BYTES;
const CONVERSION_LIMIT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(10);
type ImportResult = Result<Option<Chunk>, &'static str>;

pub(crate) struct Chunk {
    pub(crate) wav: Vec<u8>,
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    pub(crate) total: Duration,
    pub(crate) last: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Picker { owner: isize },
    Clipboard,
}

struct PasteState {
    owner: AtomicIsize,
    pending: AtomicBool,
}
/// UI-thread hook: winit consumes file-paste keys without emitting an egui event.
pub(crate) struct PasteHook {
    state: Option<Box<PasteState>>,
    _thread: PhantomData<Rc<()>>,
}
impl PasteHook {
    pub(crate) fn new(owner: isize) -> io::Result<Self> {
        let state = Box::new(PasteState {
            owner: AtomicIsize::new(owner),
            pending: AtomicBool::new(false),
        });
        let cookie = std::ptr::from_ref(state.as_ref()) as usize;
        // SAFETY: eframe supplies its HWND on its owning UI thread. The boxed
        // callback state remains live at a stable address until subclass removal.
        let installed =
            unsafe { SetWindowSubclass(owner as HWND, Some(paste_window_proc), cookie, cookie) };
        if installed == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            state: Some(state),
            _thread: PhantomData,
        })
    }
    pub(crate) fn poll(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.pending.swap(false, Ordering::AcqRel))
    }
}
impl Drop for PasteHook {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        let owner = state.owner.load(Ordering::Acquire);
        if owner != 0 {
            let cookie = std::ptr::from_ref(state.as_ref()) as usize;
            // SAFETY: This !Send guard removes its subclass on the owning UI
            // thread. Destruction clears HWND before this removal is necessary.
            let removed =
                unsafe { RemoveWindowSubclass(owner as HWND, Some(paste_window_proc), cookie) };
            if removed == 0 {
                // A failed removal leaves callback ownership uncertain. Retain
                // this one small allocation so any later callback stays valid.
                let _retained = Box::leak(state);
            }
        }
    }
}
unsafe extern "system" fn paste_window_proc(
    hwnd: HWND,
    message: u32,
    key: WPARAM,
    detail: LPARAM,
    cookie: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: The stable boxed state outlives its subclass; callbacks and Drop
    // run on this same window thread, so removal precedes freeing this pointer.
    let state = unsafe { &*(data as *const PasteState) };
    if message == WM_KEYDOWN && detail & (1 << 30) == 0 {
        // SAFETY: GetKeyState only queries process keyboard state.
        let control = unsafe { GetKeyState(i32::from(VK_CONTROL)) } < 0;
        // SAFETY: GetKeyState borrows no resources or pointers.
        let shift = unsafe { GetKeyState(i32::from(VK_SHIFT)) } < 0;
        if ((control && key == usize::from(b'V')) || (shift && key == usize::from(VK_INSERT)))
            && clipboard_has_files()
        {
            state.pending.store(true, Ordering::Release);
        }
    } else if message == WM_NCDESTROY {
        state.owner.store(0, Ordering::Release);
        // SAFETY: Remove the current callback from its live destruction callback.
        unsafe { RemoveWindowSubclass(hwnd, Some(paste_window_proc), cookie) };
    }
    // SAFETY: Preserve winit's original window procedure and its resource lifetime.
    unsafe { DefSubclassProc(hwnd, message, key, detail) }
}
fn clipboard_has_files() -> bool {
    // SAFETY: Standard format availability query returns no borrowed handles.
    unsafe { IsClipboardFormatAvailable(u32::from(CF_HDROP)) != 0 }
}

#[derive(Default)]
struct Shared {
    cancelled: AtomicBool,
    dialog: AtomicIsize,
}
pub(crate) struct Session {
    shared: Arc<Shared>,
    result: Receiver<ImportResult>,
    advance: SyncSender<()>,
    worker: Option<JoinHandle<()>>,
    complete: bool,
    can_advance: bool,
}
impl Session {
    #[cfg(test)]
    pub(crate) fn fixture(chunks: Vec<ImportResult>) -> io::Result<Self> {
        Self::fixture_observed(chunks).map(|(session, _)| session)
    }

    #[cfg(test)]
    pub(crate) fn fixture_observed(chunks: Vec<ImportResult>) -> io::Result<(Self, Receiver<()>)> {
        let shared = Arc::new(Shared::default());
        let worker_state = Arc::clone(&shared);
        let (sender, result) = mpsc::sync_channel(1);
        let (advance, next) = mpsc::sync_channel(1);
        let (decoded, observations) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("bonsai-audio-import-fixture".to_owned())
            .spawn(move || {
                for chunk in chunks {
                    let _sent = decoded.send(());
                    let complete = terminal_result(&chunk);
                    if sender.send(chunk).is_err() || complete {
                        break;
                    }
                    if await_advance(&worker_state, &next).is_err() {
                        break;
                    }
                }
            })?;
        Ok((
            Self {
                shared,
                result,
                advance,
                worker: Some(worker),
                complete: false,
                can_advance: false,
            },
            observations,
        ))
    }

    pub(crate) fn start(source: Source) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker_state = Arc::clone(&shared);
        let (sender, result) = mpsc::sync_channel(1);
        let (advance, next) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bonsai-audio-import".to_owned())
            .spawn(move || {
                if let Err(message) = import(source, &worker_state, &sender, &next) {
                    // Cancellation can occur while the preceding chunk still
                    // occupies the slot. Never block joined shutdown on it.
                    let _sent = sender.try_send(Err(message));
                }
            })?;
        Ok(Self {
            shared,
            result,
            advance,
            worker: Some(worker),
            complete: false,
            can_advance: false,
        })
    }
    pub(crate) fn poll(&mut self) -> Option<ImportResult> {
        if self.complete {
            return None;
        }
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = self.worker.take()
        {
            let _joined = worker.join();
        }
        let result = match self.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Audio import worker stopped. Your draft was kept.")
            }
        };
        self.complete = terminal_result(&result);
        self.can_advance = matches!(&result, Ok(Some(chunk)) if !chunk.last);
        Some(result)
    }
    pub(crate) fn advance(&mut self) {
        // One consumed segment grants exactly one next decode. Duplicate or
        // premature calls cannot fill the delivery slot and start a third
        // segment whose blocking send would prevent joined cancellation.
        if self.can_advance {
            self.can_advance = false;
            let _sent = self.advance.try_send(());
        }
    }
    pub(crate) fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
        close_dialog(self.shared.dialog.load(Ordering::Acquire));
    }
}
const fn terminal_result(result: &ImportResult) -> bool {
    match result {
        Ok(Some(chunk)) => chunk.last,
        Ok(None) | Err(_) => true,
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}
fn close_dialog(dialog: isize) {
    if dialog != 0 {
        // SAFETY: The hook publishes its picker HWND. Posting lets that thread
        // close its dialog; no synchronous cross-thread message wait is used.
        unsafe { PostMessageW(dialog as HWND, WM_CLOSE, 0, 0) };
    }
}
fn import(
    source: Source,
    shared: &Shared,
    sender: &SyncSender<ImportResult>,
    next: &Receiver<()>,
) -> Result<(), &'static str> {
    let path = match source {
        Source::Picker { owner } => pick_file(owner, shared)?,
        Source::Clipboard => clipboard_file()?,
    };
    let Some(path) = path else {
        let _sent = sender.send(Ok(None));
        return Ok(());
    };
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(CANCELLED);
    }
    if !supported_path(&path) {
        return Err("Choose an MP3, WAV, M4A, AAC, FLAC or WMA audio file. Your draft was kept.");
    }
    // The isolated boundary validates and retains the exact local file before
    // any codec sees it; do not follow a pathname for a preliminary metadata read.
    isolated::convert(&path, shared, sender, next)
}
fn supported_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            ["mp3", "wav", "m4a", "aac", "flac", "wma"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}
fn pick_file(owner: isize, shared: &Shared) -> Result<Option<PathBuf>, &'static str> {
    // Shell dialogs use an STA. It is released before the codec worker's MTA.
    let _apartment = Apartment::initialize(true)?;
    let mut name = vec![0_u16; 32_768];
    let filter: Vec<u16> = "Audio files (*.mp3;*.wav;*.m4a;*.aac;*.flac;*.wma)\0*.mp3;*.wav;*.m4a;*.aac;*.flac;*.wma\0\0".encode_utf16().collect();
    let title: Vec<u16> = "Add audio prompt\0".encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>()).map_err(|_| CODEC_FAILURE)?,
        hwndOwner: owner as HWND,
        lpstrFile: name.as_mut_ptr(),
        nMaxFile: 32_768,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        Flags: OFN_DONTADDTORECENT
            | OFN_ENABLEHOOK
            | OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST,
        lpfnHook: Some(picker_hook),
        lCustData: std::ptr::from_ref(shared) as isize,
        ..OPENFILENAMEW::default()
    };
    // SAFETY: All pointers refer to owned buffers live for this synchronous call;
    // no multiselect is requested and Shared outlives every picker hook callback.
    let selected = unsafe { GetOpenFileNameW(&raw mut dialog) };
    shared.dialog.store(0, Ordering::Release);
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(CANCELLED);
    }
    if selected == 0 {
        // SAFETY: The immediately preceding common-dialog call ran on this thread.
        return if unsafe { CommDlgExtendedError() } == 0 {
            Ok(None)
        } else {
            Err("The audio file picker could not open. Try pasting a copied file instead.")
        };
    }
    let length = name
        .iter()
        .position(|&unit| unit == 0)
        .ok_or(CODEC_FAILURE)?;
    Ok(Some(PathBuf::from(OsString::from_wide(&name[..length]))))
}
unsafe extern "system" fn picker_hook(
    hwnd: HWND,
    message: u32,
    _: WPARAM,
    detail: LPARAM,
) -> usize {
    if message == WM_INITDIALOG {
        // SAFETY: The OS supplies our live OPENFILENAMEW with WM_INITDIALOG.
        let dialog = unsafe { &*(detail as *const OPENFILENAMEW) };
        // SAFETY: lCustData borrows Shared for the entire GetOpenFileNameW call.
        let shared = unsafe { &*(dialog.lCustData as *const Shared) };
        // SAFETY: Explorer-style hooks receive a child of the real picker dialog.
        let parent = unsafe { GetParent(hwnd) };
        shared.dialog.store(parent as isize, Ordering::Release);
        if shared.cancelled.load(Ordering::Acquire) {
            close_dialog(parent as isize);
        }
    }
    0
}
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: This guard is constructed only after successful OpenClipboard.
        unsafe { CloseClipboard() };
    }
}
fn clipboard_file() -> Result<Option<PathBuf>, &'static str> {
    // SAFETY: This worker only reads; no owner HWND is required for OpenClipboard.
    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return Err("The clipboard is busy. Copy the audio file and paste again.");
    }
    let _clipboard = Clipboard;
    // SAFETY: The clipboard stays open while its HDROP handle is borrowed below.
    let handle = unsafe { GetClipboardData(u32::from(CF_HDROP)) };
    if handle.is_null() {
        return Ok(None);
    }
    // SAFETY: CF_HDROP provides a live HDROP; the sentinel requests its file count.
    if unsafe { DragQueryFileW(handle, u32::MAX, std::ptr::null_mut(), 0) } != 1 {
        return Err("Add one audio file at a time. Your draft was kept.");
    }
    // SAFETY: The clipboard handle remains live while querying its first path.
    let length = unsafe { DragQueryFileW(handle, 0, std::ptr::null_mut(), 0) };
    if length == 0 || length >= 32_768 {
        return Err("The copied audio filename is invalid or too long.");
    }
    let mut name = vec![0_u16; usize::try_from(length + 1).map_err(|_| CODEC_FAILURE)?];
    // SAFETY: The name buffer has length+1 writable UTF-16 units; HDROP is live.
    let copied = unsafe { DragQueryFileW(handle, 0, name.as_mut_ptr(), length + 1) };
    if copied != length {
        return Err("The copied audio filename could not be read.");
    }
    Ok(Some(PathBuf::from(OsString::from_wide(
        &name[..name.len() - 1],
    ))))
}
struct Apartment;
impl Apartment {
    fn initialize(single_threaded: bool) -> Result<Self, &'static str> {
        // SAFETY: This fresh worker owns this apartment and all its WinRT objects.
        unsafe {
            RoInitialize(if single_threaded {
                RO_INIT_SINGLETHREADED
            } else {
                RO_INIT_MULTITHREADED
            })
        }
        .map_err(|_| CODEC_FAILURE)?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Balance successful initialization on this same worker thread.
        unsafe { RoUninitialize() };
    }
}
pub(crate) fn run_worker(arguments: &[OsString]) -> std::process::ExitCode {
    isolated::run_worker(arguments)
}

fn convert_with(
    path: &Path,
    shared: &Shared,
    deliver: &mut impl FnMut(Chunk) -> Result<bool, &'static str>,
) -> Result<(), &'static str> {
    let _apartment = Apartment::initialize(false)?;
    check_memory(CHUNK_WORKING_BYTES)?;
    let mut decoder = Decoder::open(path, shared, Instant::now() + CONVERSION_LIMIT)?;
    let total = decoder.total();
    decode_with(
        &mut |capacity| decoder.read(capacity, shared, Instant::now() + CONVERSION_LIMIT),
        shared,
        deliver,
        total,
    )
}

#[cfg(test)]
fn decode_chunks(
    read: &mut impl FnMut(usize) -> Result<(Vec<u8>, bool), &'static str>,
    shared: &Shared,
    sender: &SyncSender<ImportResult>,
    next: &Receiver<()>,
    total: Duration,
) -> Result<(), &'static str> {
    decode_with(
        read,
        shared,
        &mut |chunk| {
            let last = chunk.last;
            if sender.send(Ok(Some(chunk))).is_err() || last {
                return Ok(false);
            }
            await_advance(shared, next)?;
            Ok(true)
        },
        total,
    )
}

fn decode_with(
    read: &mut impl FnMut(usize) -> Result<(Vec<u8>, bool), &'static str>,
    shared: &Shared,
    deliver: &mut impl FnMut(Chunk) -> Result<bool, &'static str>,
    total: Duration,
) -> Result<(), &'static str> {
    let mut pending = Vec::new();
    let mut emitted_samples = 0_u64;
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        check_memory(CHUNK_WORKING_BYTES)?;
        let capacity = MAX_SAMPLES
            .checked_sub(pending.len() / 2)
            .filter(|capacity| *capacity > 0)
            .ok_or(CHUNK_TOO_LONG)?;
        // One persistent reader advances only forward. Its bounded lookahead
        // identifies exact-boundary EOF without seeking or trusting duration.
        let (pcm, last) = read(capacity)?;
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        let wav = pause_segment(&mut pending, &pcm, last)?;
        let samples = u64::try_from((wav.len() - 44) / 2).map_err(|_| CHUNK_TOO_LONG)?;
        let next_sample = emitted_samples.checked_add(samples).ok_or(CHUNK_TOO_LONG)?;
        let end = sample_duration(next_sample)?;
        let chunk = Chunk {
            wav,
            start: sample_duration(emitted_samples)?,
            end,
            // Keep absent/underestimated metadata visible to presentation.
            // Only decoded EOF establishes the complete recording length.
            total: if last { end } else { total },
            last,
        };
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        if !deliver(chunk)? || last {
            return Ok(());
        }
        emitted_samples = next_sample;
    }
}

fn await_advance(shared: &Shared, next: &Receiver<()>) -> Result<(), &'static str> {
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        match next.recv_timeout(POLL) {
            Ok(()) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(CANCELLED),
        }
    }
}

fn media_duration(ticks: i64) -> Result<Duration, &'static str> {
    let seconds = u64::try_from(ticks / TICKS_PER_SECOND).map_err(|_| CODEC_FAILURE)?;
    let nanos = u32::try_from((ticks % TICKS_PER_SECOND) * 100).map_err(|_| CODEC_FAILURE)?;
    Ok(Duration::new(seconds, nanos))
}

const fn admit_memory(physical: u64, commit: u64, required: u64) -> Result<(), &'static str> {
    if physical < required || commit < required {
        Err(MEMORY_FAILURE)
    } else {
        Ok(())
    }
}

fn check_memory(required: u64) -> Result<(), &'static str> {
    let available = bonsai_windows_process::memory_status().map_err(|_| {
        "Available memory could not be checked before decoding audio. Transcription stopped. Your draft was kept."
    })?;
    admit_memory(
        available.available_physical_bytes,
        available.available_commit_bytes,
        required,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CANCELLED, CHUNK_WORKING_BYTES, Chunk, ImportResult, MAX_SAMPLES, MEMORY_FAILURE, Ordering,
        Session, Shared, admit_memory, decode_chunks, media_duration, supported_path,
    };
    use std::{
        io,
        path::Path,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    #[test]
    fn formats_are_case_insensitive_and_audio_only() {
        for name in [
            "clip.mp3",
            "clip.WAV",
            "clip.M4A",
            "clip.aac",
            "clip.flac",
            "clip.WMA",
        ] {
            assert!(supported_path(Path::new(name)));
        }
        for name in ["clip.mp4", "clip.txt", "clip", "mp3"] {
            assert!(!supported_path(Path::new(name)));
        }
    }
    #[test]
    fn decoder_duration_keeps_subsecond_metadata_precision() {
        assert_eq!(
            media_duration(432_000_000_017),
            Ok(Duration::new(43_200, 1_700))
        );
    }

    #[test]
    fn sequential_stream_keeps_pause_suffix_and_eof_tail_past_duration_metadata() -> io::Result<()>
    {
        let mut original = 12_345_i16.to_le_bytes().repeat(MAX_SAMPLES * 2 + 17);
        original[16_000 * 27 * 2..16_000 * 28 * 2].fill(0);
        let shared = Shared::default();
        let (sender, chunks) = mpsc::sync_channel(4);
        let (advance, next) = mpsc::sync_channel(4);
        for _ in 0..3 {
            advance.send(()).map_err(io::Error::other)?;
        }
        let mut offset = 0;
        let mut capacities = Vec::new();
        decode_chunks(
            &mut |capacity| {
                capacities.push(capacity);
                let end = (offset + capacity * 2).min(original.len());
                let pcm = original[offset..end].to_vec();
                offset = end;
                Ok((pcm, end == original.len()))
            },
            &shared,
            &sender,
            &next,
            Duration::from_secs(1),
        )
        .map_err(io::Error::other)?;
        let mut emitted = Vec::new();
        let mut previous_end = Duration::ZERO;
        let mut last = false;
        for result in chunks.try_iter() {
            let chunk = result
                .map_err(io::Error::other)?
                .ok_or_else(|| io::Error::other("Missing stream chunk"))?;
            assert!(!last);
            assert_eq!(chunk.start, previous_end);
            assert_eq!(
                chunk.total,
                if chunk.last {
                    chunk.end
                } else {
                    Duration::from_secs(1)
                }
            );
            let data = bonsai_audio::mono_pcm16_data(&chunk.wav, 16_000, 480_000)
                .ok_or_else(|| io::Error::other("Invalid stream PCM"))?;
            emitted.extend_from_slice(data);
            previous_end = chunk.end;
            last = chunk.last;
        }
        assert_eq!(capacities[0], MAX_SAMPLES);
        assert!(capacities[1] < MAX_SAMPLES);
        assert_eq!(emitted, original);
        assert_eq!(previous_end, Duration::new(60, 1_062_500));
        assert!(last);
        Ok(())
    }

    #[test]
    fn sequential_stream_discards_pcm_when_cancelled_during_read() {
        let shared = Shared::default();
        let (sender, chunks) = mpsc::sync_channel(1);
        let (_advance, next) = mpsc::sync_channel(1);
        let result = decode_chunks(
            &mut |_| {
                shared.cancelled.store(true, Ordering::Release);
                Ok((vec![1, 2], true))
            },
            &shared,
            &sender,
            &next,
            Duration::from_secs(1),
        );
        assert_eq!(result, Err(CANCELLED));
        assert!(chunks.try_recv().is_err());
    }

    #[test]
    fn sequential_stream_failure_retains_completed_chunk_without_finishing() -> io::Result<()> {
        let shared = Shared::default();
        let (sender, chunks) = mpsc::sync_channel(2);
        let (advance, next) = mpsc::sync_channel(1);
        advance.send(()).map_err(io::Error::other)?;
        let mut reads = 0;
        let result = decode_chunks(
            &mut |_| {
                reads += 1;
                if reads == 1 {
                    Ok((vec![1, 2], false))
                } else {
                    Err("decoder failed")
                }
            },
            &shared,
            &sender,
            &next,
            Duration::from_secs(1),
        );
        assert_eq!(result, Err("decoder failed"));
        assert!(matches!(chunks.try_recv(), Ok(Ok(Some(chunk))) if !chunk.last));
        assert!(chunks.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn segment_memory_admission_requires_available_ram_and_commit_for_live_copies() {
        assert_eq!(
            admit_memory(
                CHUNK_WORKING_BYTES,
                CHUNK_WORKING_BYTES,
                CHUNK_WORKING_BYTES
            ),
            Ok(())
        );
        for (physical, commit) in [
            (CHUNK_WORKING_BYTES - 1, u64::MAX),
            (u64::MAX, CHUNK_WORKING_BYTES - 1),
        ] {
            assert_eq!(
                admit_memory(physical, commit, CHUNK_WORKING_BYTES),
                Err(MEMORY_FAILURE)
            );
        }
    }

    fn next_chunk(session: &mut Session) -> io::Result<ImportResult> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(chunk) = session.poll() {
                return Ok(chunk);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Fixture chunk was not delivered",
                ));
            }
            thread::yield_now();
        }
    }

    fn test_chunk(last: bool) -> Chunk {
        Chunk {
            wav: vec![1, 2],
            start: Duration::ZERO,
            end: Duration::from_secs(30),
            total: Duration::from_secs(60),
            last,
        }
    }

    #[test]
    fn segment_delivery_waits_for_advance_and_preserves_final_and_failure_results() -> io::Result<()>
    {
        let mut session = Session::fixture(vec![
            Ok(Some(test_chunk(false))),
            Ok(Some(test_chunk(true))),
        ])?;
        assert!(matches!(next_chunk(&mut session)?, Ok(Some(chunk)) if !chunk.last));
        assert!(session.poll().is_none());
        session.advance();
        assert!(matches!(next_chunk(&mut session)?, Ok(Some(chunk)) if chunk.last));
        assert!(session.poll().is_none());
        let mut failed = Session::fixture(vec![Err(MEMORY_FAILURE)])?;
        assert!(matches!(next_chunk(&mut failed)?, Err(MEMORY_FAILURE)));
        assert!(failed.poll().is_none());
        Ok(())
    }

    #[test]
    fn segment_prefetch_ignores_duplicate_and_premature_advance() -> io::Result<()> {
        let (mut session, decoded) = Session::fixture_observed(vec![
            Ok(Some(test_chunk(false))),
            Ok(Some(test_chunk(false))),
            Ok(Some(test_chunk(true))),
        ])?;
        decoded
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)?;
        // No result has been consumed, so this grants no decode permit.
        session.advance();
        assert!(matches!(next_chunk(&mut session)?, Ok(Some(chunk)) if !chunk.last));
        session.advance();
        decoded
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)?;
        // The worker has consumed the legitimate permit, so its permit channel
        // is empty: backpressure alone cannot hide an accepted duplicate.
        session.advance();
        // Even with an extra advance, the queued second segment must remain the
        // only lookahead. In particular Drop must not join a blocked third send.
        let extra_decode = decoded.recv_timeout(Duration::from_millis(50));
        if extra_decode.is_ok() {
            // If the invariant regresses, free the queued slot before failing
            // the assertion so this negative control cannot hang its test run.
            let _discarded = session.poll();
        }
        session.cancel();
        drop(session);
        assert_eq!(extra_decode, Err(mpsc::RecvTimeoutError::Timeout));
        assert!(decoded.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn cancelling_a_backpressured_segment_worker_does_not_require_advance() -> io::Result<()> {
        let mut session = Session::fixture(vec![
            Ok(Some(test_chunk(false))),
            Ok(Some(test_chunk(true))),
        ])?;
        assert!(matches!(next_chunk(&mut session)?, Ok(Some(_))));
        session.cancel();
        drop(session);
        Ok(())
    }
}
