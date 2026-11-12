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
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Foundation::TimeSpan,
    Media::{
        MediaProperties::{AudioEncodingProperties, AudioEncodingQuality, MediaEncodingProfile},
        Transcoding::MediaTranscoder,
    },
    Storage::{
        StorageFile,
        Streams::{DataReader, InMemoryRandomAccessStream},
    },
    Win32::System::WinRT::{
        RO_INIT_MULTITHREADED, RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
    },
    core::{HSTRING, Interface},
};
use windows_future::{AsyncStatus, IAsyncInfo};
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

const CODEC_FAILURE: &str =
    "Could not read this audio file with the installed Windows codecs. Try a WAV or MP3 file.";
const CANCELLED: &str = "Audio import cancelled.";
const TOO_LONG: &str = "Audio files must contain at most 30 seconds of audio. Your draft was kept.";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SAMPLES: usize = 16_000 * 30;
const MAX_CONVERTED_BYTES: u64 = 16_000 * 2 * 31 + 65_536;
const CONVERSION_LIMIT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(10);
type ImportResult = Result<Option<Vec<u8>>, &'static str>;

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
    state: Box<PasteState>,
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
            state,
            _thread: PhantomData,
        })
    }
    pub(crate) fn poll(&self) -> bool {
        self.state.pending.swap(false, Ordering::AcqRel)
    }
}
impl Drop for PasteHook {
    fn drop(&mut self) {
        let owner = self.state.owner.load(Ordering::Acquire);
        if owner != 0 {
            let cookie = std::ptr::from_ref(self.state.as_ref()) as usize;
            // SAFETY: This !Send guard removes its subclass on the owning UI
            // thread before freeing the callback state. Destruction clears HWND.
            unsafe { RemoveWindowSubclass(owner as HWND, Some(paste_window_proc), cookie) };
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
    worker: Option<JoinHandle<()>>,
    complete: bool,
}
impl Session {
    pub(crate) fn start(source: Source) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker_state = Arc::clone(&shared);
        let (sender, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bonsai-audio-import".to_owned())
            .spawn(move || {
                let imported = import(source, &worker_state);
                let _sent = sender.send(imported);
            })?;
        Ok(Self {
            shared,
            result,
            worker: Some(worker),
            complete: false,
        })
    }
    pub(crate) fn poll(&mut self) -> Option<ImportResult> {
        if self.complete
            || self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            return None;
        }
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
        let result = match self.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Audio import worker stopped. Your draft was kept.")
            }
        };
        self.complete = true;
        Some(result)
    }
    pub(crate) fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
        close_dialog(self.shared.dialog.load(Ordering::Acquire));
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
fn import(source: Source, shared: &Shared) -> ImportResult {
    let path = match source {
        Source::Picker { owner } => pick_file(owner, shared)?,
        Source::Clipboard => clipboard_file()?,
    };
    let Some(path) = path else {
        return Ok(None);
    };
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(CANCELLED);
    }
    if !supported_path(&path) {
        return Err("Choose an MP3, WAV, M4A, AAC, FLAC or WMA audio file. Your draft was kept.");
    }
    let metadata = path
        .metadata()
        .map_err(|_| "The selected audio file could not be opened. Your draft was kept.")?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FILE_BYTES {
        return Err("Choose a nonempty audio file no larger than 64 MiB. Your draft was kept.");
    }
    convert(&path, shared).map(Some)
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
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>())
            .map_err(|_| CODEC_FAILURE)?,
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
fn wait(
    operation: &impl Interface,
    shared: &Shared,
    deadline: Instant,
) -> Result<(), &'static str> {
    let info: IAsyncInfo = operation.cast().map_err(|_| CODEC_FAILURE)?;
    loop {
        if shared.cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            let _cancelled = info.Cancel();
            return Err(if shared.cancelled.load(Ordering::Acquire) {
                CANCELLED
            } else {
                "Audio conversion timed out. Try a shorter WAV file. Your draft was kept."
            });
        }
        match info.Status().map_err(|_| CODEC_FAILURE)? {
            AsyncStatus::Started => thread::sleep(POLL),
            AsyncStatus::Completed => return Ok(()),
            _ => return Err(CODEC_FAILURE),
        }
    }
}
macro_rules! complete {
    ($operation:expr, $shared:expr, $deadline:expr) => {{
        let operation = $operation.map_err(|_| CODEC_FAILURE)?;
        wait(&operation, $shared, $deadline)?;
        operation.GetResults().map_err(|_| CODEC_FAILURE)?
    }};
}
fn convert(path: &Path, shared: &Shared) -> Result<Vec<u8>, &'static str> {
    let _apartment = Apartment::initialize(false)?;
    let deadline = Instant::now() + CONVERSION_LIMIT;
    let path = HSTRING::from(path.as_os_str());
    let file = complete!(StorageFile::GetFileFromPathAsync(&path), shared, deadline);
    let source = complete!(file.OpenReadAsync(), shared, deadline);
    if source.Size().map_err(|_| CODEC_FAILURE)? > MAX_FILE_BYTES {
        let _closed = source.Close();
        return Err("Choose an audio file no larger than 64 MiB. Your draft was kept.");
    }
    let destination = InMemoryRandomAccessStream::new().map_err(|_| CODEC_FAILURE)?;
    let result = transcode(&source, &destination, shared, deadline);
    let _closed_source = source.Close();
    let _closed_destination = destination.Close();
    result
}
fn transcode(
    source: &windows::Storage::Streams::IRandomAccessStreamWithContentType,
    destination: &InMemoryRandomAccessStream,
    shared: &Shared,
    deadline: Instant,
) -> Result<Vec<u8>, &'static str> {
    let profile =
        MediaEncodingProfile::CreateWav(AudioEncodingQuality::Low).map_err(|_| CODEC_FAILURE)?;
    profile
        .SetAudio(&AudioEncodingProperties::CreatePcm(16_000, 1, 16).map_err(|_| CODEC_FAILURE)?)
        .map_err(|_| CODEC_FAILURE)?;
    let transcoder = MediaTranscoder::new().map_err(|_| CODEC_FAILURE)?;
    transcoder
        .SetAlwaysReencode(true)
        .map_err(|_| CODEC_FAILURE)?;
    // Decode an extra second to detect overlength input while bounding output
    // storage. Any result longer than 30 seconds is rejected, never truncated.
    transcoder
        .SetTrimStopTime(TimeSpan {
            Duration: 31 * 10_000_000,
        })
        .map_err(|_| CODEC_FAILURE)?;
    let prepared = complete!(
        transcoder.PrepareStreamTranscodeAsync(source, destination, &profile),
        shared,
        deadline
    );
    if !prepared.CanTranscode().map_err(|_| CODEC_FAILURE)? {
        return Err(CODEC_FAILURE);
    }
    complete!(prepared.TranscodeAsync(), shared, deadline);
    let size = destination.Size().map_err(|_| CODEC_FAILURE)?;
    if !(44..=MAX_CONVERTED_BYTES).contains(&size) {
        return Err(TOO_LONG);
    }
    let size = u32::try_from(size).map_err(|_| TOO_LONG)?;
    let reader =
        DataReader::CreateDataReader(&destination.GetInputStreamAt(0).map_err(|_| CODEC_FAILURE)?)
            .map_err(|_| CODEC_FAILURE)?;
    let loaded = complete!(reader.LoadAsync(size), shared, deadline);
    if loaded != size {
        return Err(CODEC_FAILURE);
    }
    let mut wave = vec![0_u8; usize::try_from(size).map_err(|_| CODEC_FAILURE)?];
    reader.ReadBytes(&mut wave).map_err(|_| CODEC_FAILURE)?;
    normalize_wave(&wave)
}
fn normalize_wave(wave: &[u8]) -> Result<Vec<u8>, &'static str> {
    if wave.get(..4) != Some(b"RIFF") || wave.get(8..12) != Some(b"WAVE") {
        return Err(CODEC_FAILURE);
    }
    let declared = usize::try_from(u32::from_le_bytes(
        wave.get(4..8)
            .ok_or(CODEC_FAILURE)?
            .try_into()
            .map_err(|_| CODEC_FAILURE)?,
    ))
    .map_err(|_| CODEC_FAILURE)?;
    if declared.checked_add(8) != Some(wave.len()) {
        return Err(CODEC_FAILURE);
    }
    let mut offset = 12_usize;
    let mut format = false;
    let mut data = None;
    while let Some(header) = wave.get(offset..offset.saturating_add(8)) {
        let length = usize::try_from(u32::from_le_bytes(
            header[4..8].try_into().map_err(|_| CODEC_FAILURE)?,
        ))
        .map_err(|_| CODEC_FAILURE)?;
        offset += 8;
        let end = offset.checked_add(length).ok_or(CODEC_FAILURE)?;
        let payload = wave.get(offset..end).ok_or(CODEC_FAILURE)?;
        match &header[..4] {
            b"fmt " => {
                if format || payload.len() < 16 {
                    return Err(CODEC_FAILURE);
                }
                let expected = [1, 0, 1, 0, 128, 62, 0, 0, 0, 125, 0, 0, 2, 0, 16, 0];
                if payload[..16] != expected {
                    return Err(CODEC_FAILURE);
                }
                format = true;
            }
            b"data" => {
                if data.replace(payload).is_some() {
                    return Err(CODEC_FAILURE);
                }
            }
            _ => {}
        }
        offset = end.checked_add(length % 2).ok_or(CODEC_FAILURE)?;
    }
    if offset != wave.len() {
        return Err(CODEC_FAILURE);
    }
    let data = data.ok_or(CODEC_FAILURE)?;
    if !format || data.is_empty() || !data.len().is_multiple_of(2) {
        return Err(CODEC_FAILURE);
    }
    let samples = data.len() / 2;
    if samples > MAX_SAMPLES {
        return Err(TOO_LONG);
    }
    let header = bonsai_audio::mono_pcm16_header(16_000, samples).ok_or(CODEC_FAILURE)?;
    let mut result = Vec::with_capacity(data.len() + 44);
    result.extend_from_slice(&header);
    result.extend_from_slice(data);
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::{MAX_SAMPLES, TOO_LONG, normalize_wave, supported_path};
    use std::path::Path;
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
    fn normalized_wave_accepts_metadata_and_preserves_samples() {
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, 2)
            .unwrap_or([0; 44])
            .to_vec();
        wave.extend_from_slice(&[0, 128, 255, 127]);
        wave.extend_from_slice(b"JUNK\x02\0\0\0xx");
        let length = u32::try_from(wave.len() - 8).unwrap_or(0);
        wave[4..8].copy_from_slice(&length.to_le_bytes());
        let normalized = normalize_wave(&wave).unwrap_or_default();
        assert_eq!(
            bonsai_audio::mono_pcm16_data(&normalized, 16_000, 480_000),
            Some(&[0, 128, 255, 127][..])
        );
    }
    #[test]
    fn overlength_empty_and_non_pcm_wave_fail_without_truncation() {
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, MAX_SAMPLES + 1)
            .unwrap_or([0; 44])
            .to_vec();
        wave.resize(44 + (MAX_SAMPLES + 1) * 2, 0);
        assert_eq!(normalize_wave(&wave), Err(TOO_LONG));
        assert!(normalize_wave(&[]).is_err());
        wave[20] = 3;
        assert!(normalize_wave(&wave).is_err());
    }
}
