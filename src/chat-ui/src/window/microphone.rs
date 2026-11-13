//! UI-thread microphone permission, capture and cancellation.
//!
//! No microphone API is activated until an explicit start. `WinRT` operations are
//! polled by the egui event loop; one completed utterance remains in memory for
//! local transcription. Finishing and cancelling a recording are separate actions.

use std::{
    io,
    marker::PhantomData,
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Media::{
        Capture::{MediaCapture, MediaCaptureInitializationSettings, StreamingCaptureMode},
        MediaProperties::{AudioEncodingProperties, AudioEncodingQuality, MediaEncodingProfile},
    },
    Security::Authorization::AppCapabilityAccess::{AppCapability, AppCapabilityAccessStatus},
    Storage::Streams::{DataReader, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};
use windows_future::{AsyncStatus, IAsyncAction, IAsyncOperation};

// Leave headroom for polling and Windows finalization below the ASR's 30s bound.
const CLIP_LIMIT: Duration = Duration::from_secs(25);
const START_LIMIT: Duration = Duration::from_secs(15);
const OPERATION_LIMIT: Duration = Duration::from_secs(45);
const PERMISSION_LIMIT: Duration = Duration::from_mins(2);
const MAX_WAV: u64 = 1024 * 1024;

pub(super) enum Event {
    Status(&'static str),
    Recording { generation: u64 },
    Clip { generation: u64, wav: Vec<u8> },
    Failed(&'static str),
}

struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    fn initialize() -> io::Result<Self> {
        // SAFETY: This guard is created on the native UI thread before microphone
        // activation and cannot cross threads. winit also uses an STA on Windows;
        // this successful WinRT initialization has its own balanced guard.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.map_err(|_| {
            io::Error::other("The UI thread cannot initialize Windows microphone access.")
        })?;
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: All capture objects have been closed on this same thread before
        // this guard releases its successful RoInitialize call.
        unsafe { RoUninitialize() };
    }
}

#[derive(Default)]
struct Lifecycle {
    generation: u64,
    requested: bool,
    waiting: bool,
}
impl Lifecycle {
    fn begin(&mut self) -> io::Result<u64> {
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            io::Error::other("Microphone session limit reached. Restart the app.")
        })?;
        self.requested = true;
        self.waiting = false;
        Ok(self.generation)
    }

    const fn stop(&mut self) {
        self.requested = false;
        self.waiting = false;
    }

    const fn accept_clip(&mut self) {
        self.waiting = true;
    }
}

enum Pending {
    Permission(IAsyncOperation<AppCapabilityAccessStatus>),
    Initialize(IAsyncAction),
    Start(IAsyncAction),
    Stop(IAsyncAction),
    Read {
        operation: IAsyncOperation<u32>,
        reader: DataReader,
        size: u32,
    },
}
impl Pending {
    fn status(&self) -> windows::core::Result<AsyncStatus> {
        match self {
            Self::Permission(operation) => operation.Status(),
            Self::Initialize(operation) | Self::Start(operation) | Self::Stop(operation) => {
                operation.Status()
            }
            Self::Read { operation, .. } => operation.Status(),
        }
    }

    fn cancel(&self) {
        let _result = match self {
            Self::Permission(operation) => operation.Cancel(),
            Self::Initialize(operation) | Self::Start(operation) | Self::Stop(operation) => {
                operation.Cancel()
            }
            Self::Read { operation, .. } => operation.Cancel(),
        };
    }
}

trait AudioResource {
    fn close(self);
}

impl AudioResource for MediaCapture {
    fn close(self) {
        let _result = self.Close();
    }
}

impl AudioResource for InMemoryRandomAccessStream {
    fn close(self) {
        let _result = self.Close();
    }
}

/// Capture and stream resources belong to one explicitly enabled generation.
struct CaptureResources<C, S> {
    capture: Option<(u64, C)>,
    stream: Option<S>,
}

impl<C: AudioResource, S: AudioResource> CaptureResources<C, S> {
    const fn new() -> Self {
        Self {
            capture: None,
            stream: None,
        }
    }

    fn capture_for(&self, generation: u64) -> Option<&C> {
        self.capture
            .as_ref()
            .and_then(|(owner, capture)| (*owner == generation).then_some(capture))
    }

    fn finish_clip(&mut self) {
        if let Some(stream) = self.stream.take() {
            stream.close();
        }
    }

    fn close(&mut self) {
        if let Some((_, capture)) = self.capture.take() {
            capture.close();
        }
        self.finish_clip();
    }
}

pub(super) struct Microphone {
    lifecycle: Lifecycle,
    capability: Option<AppCapability>,
    resources: CaptureResources<MediaCapture, InMemoryRandomAccessStream>,
    pending: Option<Pending>,
    pending_since: Instant,
    recording_since: Option<Instant>,
    checked_access: Instant,
    _apartment: Apartment,
}

impl Microphone {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            lifecycle: Lifecycle::default(),
            capability: None,
            resources: CaptureResources::new(),
            pending: None,
            pending_since: Instant::now(),
            recording_since: None,
            checked_access: Instant::now(),
            _apartment: Apartment::initialize()?,
        })
    }

    pub(super) fn start(&mut self) -> io::Result<u64> {
        self.stop();
        let generation = self.lifecycle.begin()?;
        let result = self.request_access();
        if result.is_err() {
            self.stop();
            return Err(io::Error::other(
                "Windows microphone permission could not be requested. Check microphone privacy settings and app capabilities.",
            ));
        }
        Ok(generation)
    }

    fn request_access(&mut self) -> windows::core::Result<()> {
        let capability = AppCapability::Create(&HSTRING::from("microphone"))?;
        let request = capability.RequestAccessAsync()?;
        self.capability = Some(capability);
        self.set_pending(Pending::Permission(request));
        Ok(())
    }

    pub(super) fn stop(&mut self) {
        self.lifecycle.stop();
        self.close_capture();
        self.capability = None;
    }

    fn close_capture(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
        self.recording_since = None;
        self.resources.close();
    }

    /// Finish captured audio; a second click during permission/setup cancels.
    pub(super) fn finish(&mut self) -> io::Result<bool> {
        if self.recording_since.is_some() {
            self.finalize_clip().map_err(io::Error::other)?;
            return Ok(true);
        }
        if self.lifecycle.waiting
            || matches!(self.pending, Some(Pending::Stop(_) | Pending::Read { .. }))
        {
            return Ok(true);
        }
        self.stop();
        Ok(false)
    }

    fn finalize_clip(&mut self) -> Result<(), &'static str> {
        self.check_access()
            .map_err(|_| "Microphone permission is unavailable.")?;
        let capture = self
            .resources
            .capture_for(self.lifecycle.generation)
            .ok_or("The microphone session ended.")?;
        let operation = capture
            .StopRecordAsync()
            .map_err(|_| "Could not finalize the microphone recording.")?;
        self.recording_since = None;
        self.set_pending(Pending::Stop(operation));
        Ok(())
    }

    fn set_pending(&mut self, pending: Pending) {
        self.pending = Some(pending);
        self.pending_since = Instant::now();
    }

    fn check_access(&self) -> windows::core::Result<()> {
        let capability = self
            .capability
            .as_ref()
            .ok_or_else(windows::core::Error::from_thread)?;
        if capability.CheckAccess()? != AppCapabilityAccessStatus::Allowed {
            return Err(windows::core::Error::from_thread());
        }
        Ok(())
    }

    fn initialize_capture(&mut self) -> windows::core::Result<()> {
        self.check_access()?;
        let settings = MediaCaptureInitializationSettings::new()?;
        settings.SetStreamingCaptureMode(StreamingCaptureMode::Audio)?;
        let capture = MediaCapture::new()?;
        let operation = capture.InitializeWithSettingsAsync(&settings)?;
        self.resources.capture = Some((self.lifecycle.generation, capture));
        self.set_pending(Pending::Initialize(operation));
        self.checked_access = Instant::now();
        Ok(())
    }

    fn begin_clip(&mut self) -> windows::core::Result<()> {
        self.check_access()?;
        let profile = MediaEncodingProfile::CreateWav(AudioEncodingQuality::Low)?;
        profile.SetAudio(&AudioEncodingProperties::CreatePcm(16_000, 1, 16)?)?;
        let stream = InMemoryRandomAccessStream::new()?;
        let capture = self
            .resources
            .capture_for(self.lifecycle.generation)
            .ok_or_else(windows::core::Error::from_thread)?;
        let operation = capture.StartRecordToStreamAsync(&profile, &stream)?;
        self.resources.stream = Some(stream);
        self.recording_since = None;
        self.set_pending(Pending::Start(operation));
        Ok(())
    }

    pub(super) fn poll(&mut self) -> Vec<Event> {
        if !self.lifecycle.requested {
            return Vec::new();
        }
        match self.advance() {
            Ok(Some(event)) => vec![event],
            Ok(None) => Vec::new(),
            Err(message) => {
                self.stop();
                vec![Event::Failed(message)]
            }
        }
    }

    fn advance(&mut self) -> Result<Option<Event>, &'static str> {
        let permission_pending = matches!(self.pending, Some(Pending::Permission(_)));
        if !permission_pending && self.checked_access.elapsed() >= Duration::from_secs(1) {
            self.check_access().map_err(
                |_| "Microphone permission was revoked. Check Windows privacy settings.",
            )?;
            self.checked_access = Instant::now();
        }
        if self
            .resources
            .stream
            .as_ref()
            .is_some_and(|stream| !stream.Size().is_ok_and(|size| size <= MAX_WAV))
        {
            return Err(
                "The microphone clip exceeded its memory limit. Enable the microphone again to retry.",
            );
        }
        if matches!(self.pending, Some(Pending::Start(_)))
            && self.pending_since.elapsed() >= START_LIMIT
        {
            return Err("Windows could not start the microphone within its startup deadline.");
        }
        if let Some(pending) = self.pending.take() {
            let limit = if permission_pending {
                PERMISSION_LIMIT
            } else {
                OPERATION_LIMIT
            };
            if self.pending_since.elapsed() >= limit {
                pending.cancel();
                return Err(
                    "Windows microphone access timed out. Enable the microphone again to retry.",
                );
            }
            if pending
                .status()
                .map_err(|_| "Windows microphone operation failed.")?
                == AsyncStatus::Started
            {
                self.pending = Some(pending);
                return Ok(None);
            }
            return self.completed(pending);
        }
        if self
            .recording_since
            .is_some_and(|started| started.elapsed() >= CLIP_LIMIT)
        {
            self.finalize_clip()?;
            return Ok(Some(Event::Status("Finalizing microphone recording...")));
        }
        Ok(None)
    }

    fn completed(&mut self, pending: Pending) -> Result<Option<Event>, &'static str> {
        match pending {
            Pending::Permission(operation) => {
                let status = operation
                    .GetResults()
                    .map_err(|_| "Windows microphone permission was cancelled or unavailable.")?;
                if status != AppCapabilityAccessStatus::Allowed {
                    return Err(
                        "Windows microphone permission was denied. Check microphone privacy settings and app capabilities.",
                    );
                }
                self.initialize_capture()
                    .map_err(|_| "Windows could not initialize audio-only microphone capture.")?;
                Ok(Some(Event::Status("Initializing microphone...")))
            }
            Pending::Initialize(operation) => {
                operation.GetResults().map_err(|_| "Windows could not open the microphone. Check privacy settings and the audio device.")?;
                self.begin_clip()
                    .map_err(|_| "Windows could not start PCM microphone recording.")?;
                Ok(Some(Event::Status("Starting microphone recording...")))
            }
            Pending::Start(operation) => {
                operation
                    .GetResults()
                    .map_err(|_| "Windows could not start microphone recording.")?;
                self.recording_since = Some(Instant::now());
                // This is the first point where permission and recording have
                // both succeeded. The worker may overlap model loading with
                // capture; it still receives no audio until finalization.
                Ok(Some(Event::Recording {
                    generation: self.lifecycle.generation,
                }))
            }
            Pending::Stop(operation) => {
                operation
                    .GetResults()
                    .map_err(|_| "Windows could not finalize the microphone clip.")?;
                self.load_clip()
                    .map_err(|_| "The microphone clip could not be read within its limit.")?;
                Ok(Some(Event::Status(
                    "Preparing microphone audio for local transcription...",
                )))
            }
            Pending::Read {
                operation,
                reader,
                size,
            } => {
                let loaded = operation
                    .GetResults()
                    .map_err(|_| "The microphone clip could not be read.")?;
                if loaded != size {
                    return Err("The microphone clip was incomplete.");
                }
                let mut wav = vec![
                    0;
                    usize::try_from(size)
                        .map_err(|_| "The microphone clip length was invalid.")?
                ];
                reader
                    .ReadBytes(&mut wav)
                    .map_err(|_| "The microphone clip could not be copied.")?;
                let _result = reader.DetachStream();
                let _result = reader.Close();
                // StopRecordAsync completed; release the device before inference.
                self.resources.close();
                self.lifecycle.accept_clip();
                Ok(Some(Event::Clip {
                    generation: self.lifecycle.generation,
                    wav,
                }))
            }
        }
    }

    fn load_clip(&mut self) -> windows::core::Result<()> {
        let stream = self
            .resources
            .stream
            .as_ref()
            .ok_or_else(windows::core::Error::from_thread)?;
        let size = stream.Size()?;
        if !(44..=MAX_WAV).contains(&size) {
            return Err(windows::core::Error::from_thread());
        }
        let size = u32::try_from(size).map_err(|_| windows::core::Error::from_thread())?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        let operation = reader.LoadAsync(size)?;
        self.set_pending(Pending::Read {
            operation,
            reader,
            size,
        });
        Ok(())
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioResource, CaptureResources, Lifecycle};
    use std::{cell::Cell, io, rc::Rc};

    struct Resource(Rc<Cell<bool>>);
    impl AudioResource for Resource {
        fn close(self) {
            self.0.set(true);
        }
    }

    #[test]
    fn completed_utterance_releases_capture_and_waits_without_restarting() -> io::Result<()> {
        let capture_closed = Rc::new(Cell::new(false));
        let stream_closed = Rc::new(Cell::new(false));
        let mut lifecycle = Lifecycle::default();
        let generation = lifecycle.begin()?;
        let mut resources = CaptureResources::new();
        resources.capture = Some((generation, Resource(Rc::clone(&capture_closed))));
        resources.stream = Some(Resource(Rc::clone(&stream_closed)));
        resources.close();
        lifecycle.accept_clip();
        assert!(stream_closed.get());
        assert!(capture_closed.get());
        assert!(resources.stream.is_none());
        assert!(resources.capture_for(generation).is_none());
        assert!(lifecycle.requested && lifecycle.waiting);

        // Revocation and explicit disabling both take this full-close path.
        lifecycle.accept_clip();
        lifecycle.stop();
        resources.close();
        assert!(capture_closed.get());
        assert!(resources.capture_for(generation).is_none());
        assert!(!lifecycle.requested && !lifecycle.waiting);
        let next = lifecycle.begin()?;
        assert!(resources.capture_for(next).is_none());
        assert_ne!(next, generation);
        Ok(())
    }

    #[test]
    fn cancellation_invalidates_the_utterance_before_a_new_generation() -> io::Result<()> {
        let mut lifecycle = Lifecycle::default();
        let old = lifecycle.begin()?;
        lifecycle.accept_clip();
        lifecycle.stop();
        assert!(!lifecycle.requested && !lifecycle.waiting);
        let current = lifecycle.begin()?;
        assert_ne!(current, old);
        lifecycle.accept_clip();
        assert_eq!(lifecycle.generation, current);
        assert!(lifecycle.requested && lifecycle.waiting);
        Ok(())
    }

    #[test]
    fn exhausted_generations_cannot_reuse_an_old_session() {
        let mut lifecycle = Lifecycle {
            generation: u64::MAX,
            requested: false,
            waiting: false,
        };
        assert!(lifecycle.begin().is_err());
        assert!(!lifecycle.requested);
    }
}
