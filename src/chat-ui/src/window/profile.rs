//! Encrypted device-wide preferences, independent of conversation saving.
use super::{State, VOICE_LIMIT};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Write as _},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 8_192;
const MAX_SEALED: u64 = 73_728;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    voice_profile: String,
}

pub(super) struct Opened {
    store: Store,
    saved: Option<String>,
    load_failed: bool,
}

pub(super) struct Store {
    latest: Option<String>,
    pending: Arc<Mutex<Option<String>>>,
    wake: Option<SyncSender<()>>,
    failed: Arc<AtomicBool>,
    retry: Arc<AtomicBool>,
    last_attempt: Instant,
    worker: Option<JoinHandle<()>>,
}

impl Store {
    pub(super) fn open(data: &Path) -> io::Result<Opened> {
        let path = data.join("chat-profile.dpapi");
        bonsai_windows_credentials::validate_external_path(&path)?;
        let (saved, load_failed) = match read(&path) {
            Ok(saved) => (saved, false),
            Err(_) => (None, true),
        };
        let pending = Arc::new(Mutex::new(None));
        let queued = Arc::clone(&pending);
        let failed = Arc::new(AtomicBool::new(false));
        let reported = Arc::clone(&failed);
        let retry = Arc::new(AtomicBool::new(false));
        let retry_requested = Arc::clone(&retry);
        let (wake, incoming) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("swift-chat-profile".to_owned())
            .spawn(move || {
                while incoming.recv().is_ok() {
                    let voice = queued.lock().ok().and_then(|mut pending| pending.take());
                    if let Some(voice) = voice
                        && save(&path, &voice).is_err()
                    {
                        reported.store(true, Ordering::Release);
                        retry_requested.store(true, Ordering::Release);
                    }
                }
            })?;
        Ok(Opened {
            store: Self {
                latest: saved.clone(),
                pending,
                wake: Some(wake),
                failed,
                retry,
                last_attempt: Instant::now(),
                worker: Some(worker),
            },
            saved,
            load_failed,
        })
    }

    pub(super) fn queue(&mut self, voice: &str) -> io::Result<()> {
        if voice.chars().count() > VOICE_LIMIT {
            return Err(io::Error::other("Voice profile exceeds its size limit."));
        }
        if self.latest.as_deref() == Some(voice)
            && (!self.retry.load(Ordering::Acquire)
                || self.last_attempt.elapsed() < Duration::from_secs(3))
        {
            return Ok(());
        }
        self.retry.store(false, Ordering::Release);
        *self
            .pending
            .lock()
            .map_err(|_| io::Error::other("Profile saving worker stopped."))? =
            Some(voice.to_owned());
        if let Some(wake) = &self.wake {
            match wake.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(())) => {}
                Err(mpsc::TrySendError::Disconnected(())) => {
                    return Err(io::Error::other("Profile saving worker stopped."));
                }
            }
        }
        self.latest = Some(voice.to_owned());
        self.last_attempt = Instant::now();
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.wake.take();
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

impl State {
    pub(super) fn attach_profile(&mut self, opened: io::Result<Opened>) {
        match opened {
            Ok(mut opened) => {
                if let Some(voice) = opened.saved {
                    self.voice_profile = voice;
                } else if opened.load_failed {
                    // Establish a baseline without replacing unreadable bytes.
                    // Only a subsequent explicit voice edit may replace them.
                    opened.store.latest = Some(self.voice_profile.clone());
                    self.record("Could not restore your voice profile. The existing file is preserved until you change the voice in Settings.");
                }
                // Missing profiles migrate the voice restored from older workspaces.
                self.profile_store = Some(opened.store);
                self.poll_profile();
            }
            Err(_) => self.record("Could not open profile preferences. Voice changes cannot be saved for the next launch."),
        }
    }

    pub(super) fn poll_profile(&mut self) {
        let Some(store) = &mut self.profile_store else {
            return;
        };
        let failed = store.failed.swap(false, Ordering::Acquire);
        if store.queue(&self.voice_profile).is_err() || failed {
            self.record(
                "Could not save your voice profile. Your conversations are saved separately.",
            );
        }
    }
}

fn read(path: &Path) -> io::Result<Option<String>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut sealed = Vec::new();
    file.take(MAX_SEALED + 1).read_to_end(&mut sealed)?;
    if u64::try_from(sealed.len()).map_err(io::Error::other)? > MAX_SEALED {
        return Err(io::Error::other("Saved profile exceeds its storage bound."));
    }
    let bytes = bonsai_windows_credentials::unprotect_private_data(&sealed)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("Saved profile exceeds its size limit."));
    }
    let saved: Saved = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if saved.version != 1 || saved.voice_profile.chars().count() > VOICE_LIMIT {
        return Err(io::Error::other("Invalid saved profile."));
    }
    Ok(Some(saved.voice_profile))
}

fn save(path: &Path, voice: &str) -> io::Result<()> {
    bonsai_windows_credentials::validate_external_path(path)?;
    let bytes = serde_json::to_vec(&Saved {
        version: 1,
        voice_profile: voice.to_owned(),
    })
    .map_err(io::Error::other)?;
    if voice.chars().count() > VOICE_LIMIT || bytes.len() > MAX_BYTES {
        return Err(io::Error::other("Voice profile exceeds its size limit."));
    }
    let sealed = bonsai_windows_credentials::protect_private_data(&bytes)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{stamp}.pending", std::process::id()));
    bonsai_windows_credentials::validate_external_path(&temporary)?;
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(&sealed)?;
        file.sync_all()?;
        drop(file);
        bonsai_windows_credentials::validate_external_path(path)?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() && created {
        let _removed = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> io::Result<std::path::PathBuf> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-profile-{stamp}"));
        fs::create_dir(&directory)?;
        Ok(directory)
    }

    fn state() -> State {
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming) = crate::client::event_channel();
        let mut state = State::create(commands, incoming, None, None);
        state.preview = false;
        state
    }

    #[test]
    fn profile_voice_survives_restart_with_conversation_saving_off() -> io::Result<()> {
        let directory = fixture()?;
        let mut original = state();
        original.attach_recovery(crate::recovery::Recovery::open(&directory));
        original.attach_profile(Store::open(&directory));
        original.voice_profile = "A quiet, clear voice 雨 🌱".to_owned();
        let chosen = original.voice_profile.clone();
        original.input = "Do not save this conversation".to_owned();
        original.save_conversations = false;
        original.recovery_erase_pending = true;
        assert!(!original.save_conversations);
        drop(original);
        let path = directory.join("chat-profile.dpapi");
        assert!(
            !fs::read(&path)?
                .windows(chosen.len())
                .any(|bytes| bytes == chosen.as_bytes())
        );
        let mut restored = state();
        restored.attach_recovery(crate::recovery::Recovery::open(&directory));
        restored.attach_profile(Store::open(&directory));
        assert_eq!(restored.voice_profile, chosen);
        assert!(restored.input.is_empty());
        assert!(!restored.save_conversations && !restored.speaking && !restored.listening);
        assert!(!directory.join("chat-recovery.dpapi").exists());
        drop(restored);
        fs::remove_file(path)?;
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)
    }

    #[test]
    fn profile_migrates_legacy_voice_and_takes_precedence_over_recovery() -> io::Result<()> {
        let directory = fixture()?;
        let mut legacy = state();
        legacy.attach_recovery(crate::recovery::Recovery::open(&directory));
        legacy.voice_profile = "Voice restored from the legacy workspace".to_owned();
        drop(legacy);
        let mut migrated = state();
        migrated.attach_recovery(crate::recovery::Recovery::open(&directory));
        assert_eq!(
            migrated.voice_profile,
            "Voice restored from the legacy workspace"
        );
        migrated.attach_profile(Store::open(&directory));
        drop(migrated);
        // An older application may still write its historical workspace field.
        let mut stale = state();
        stale.attach_recovery(crate::recovery::Recovery::open(&directory));
        stale.voice_profile = "Older recovered workspace voice".to_owned();
        drop(stale);
        let mut restored = state();
        restored.attach_recovery(crate::recovery::Recovery::open(&directory));
        assert_eq!(restored.voice_profile, "Older recovered workspace voice");
        restored.attach_profile(Store::open(&directory));
        assert_eq!(
            restored.voice_profile,
            "Voice restored from the legacy workspace"
        );
        drop(restored);
        fs::remove_file(directory.join("chat-profile.dpapi"))?;
        fs::remove_file(directory.join("chat-recovery.dpapi"))?;
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)
    }

    #[test]
    fn profile_preserves_corrupt_bytes_until_a_voice_edit() -> io::Result<()> {
        let directory = fixture()?;
        let path = directory.join("chat-profile.dpapi");
        fs::write(&path, b"unreadable profile")?;
        let mut unchanged = state();
        unchanged.attach_profile(Store::open(&directory));
        unchanged.poll_profile();
        drop(unchanged);
        assert_eq!(fs::read(&path)?, b"unreadable profile");
        let mut changed = state();
        changed.attach_profile(Store::open(&directory));
        changed.voice_profile = "An explicitly changed voice".to_owned();
        drop(changed);
        assert_eq!(read(&path)?.as_deref(), Some("An explicitly changed voice"));
        fs::remove_file(path)?;
        fs::remove_dir(directory)
    }
}
