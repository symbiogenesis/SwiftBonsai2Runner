//! Native window geometry, saved independently of encrypted conversation content.

use super::State;
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{
        GetWindowPlacement, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWNORMAL, SetWindowPlacement,
        WINDOWPLACEMENT,
    },
};

const MAX_BYTES: u64 = 2_048;
const MAX_COORDINATE: i32 = 1_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    normal: [i32; 4],
    maximized: bool,
}

impl Saved {
    fn valid(self) -> bool {
        let [left, top, right, bottom] = self.normal;
        self.version == 1
            && self
                .normal
                .iter()
                .all(|coordinate| (-MAX_COORDINATE..=MAX_COORDINATE).contains(coordinate))
            && (256..=32_768).contains(&(i64::from(right) - i64::from(left)))
            && (256..=32_768).contains(&(i64::from(bottom) - i64::from(top)))
    }

    fn from_native(native: &WINDOWPLACEMENT) -> Option<Self> {
        // Minimize/close must not replace the last ordinary geometry or turn a
        // maximized window into a restored window on its next launch.
        if native.showCmd == SW_SHOWMINIMIZED.unsigned_abs() {
            return None;
        }
        let rect = native.rcNormalPosition;
        let saved = Self {
            version: 1,
            normal: [rect.left, rect.top, rect.right, rect.bottom],
            maximized: native.showCmd == SW_SHOWMAXIMIZED.unsigned_abs(),
        };
        saved.valid().then_some(saved)
    }

    fn native(self) -> io::Result<WINDOWPLACEMENT> {
        let [left, top, right, bottom] = self.normal;
        Ok(WINDOWPLACEMENT {
            length: u32::try_from(size_of::<WINDOWPLACEMENT>()).map_err(io::Error::other)?,
            showCmd: if self.maximized {
                SW_SHOWMAXIMIZED
            } else {
                SW_SHOWNORMAL
            }
            .unsigned_abs(),
            rcNormalPosition: RECT {
                left,
                top,
                right,
                bottom,
            },
            ..WINDOWPLACEMENT::default()
        })
    }
}

pub(super) struct Placement {
    restored: Option<Saved>,
    latest: Option<Saved>,
    pending: Arc<Mutex<Option<Saved>>>,
    wake: Option<SyncSender<()>>,
    failed: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    last_observed: Option<Instant>,
}

impl Placement {
    pub(super) fn open(data: &Path) -> io::Result<Self> {
        let path = data.join("chat-window.json");
        bonsai_windows_credentials::validate_external_path(&path)?;
        let restored = read(&path).unwrap_or_default();
        let pending = Arc::new(Mutex::new(None));
        let queued = Arc::clone(&pending);
        let failed = Arc::new(AtomicBool::new(false));
        let reported = Arc::clone(&failed);
        let (wake, incoming) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("swift-chat-window".to_owned())
            .spawn(move || {
                while incoming.recv().is_ok() {
                    let saved = queued.lock().ok().and_then(|mut pending| pending.take());
                    if let Some(saved) = saved {
                        reported.store(save(&path, saved).is_err(), Ordering::Release);
                    }
                }
            })?;
        Ok(Self {
            restored,
            latest: restored,
            pending,
            wake: Some(wake),
            failed,
            worker: Some(worker),
            last_observed: None,
        })
    }

    fn restore(&self, cc: &eframe::CreationContext<'_>) -> io::Result<()> {
        let Some(saved) = self.restored else {
            return Ok(());
        };
        let handle = cc.window_handle().map_err(io::Error::other)?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(io::Error::other(
                "Native Windows window handle is unavailable.",
            ));
        };
        let native = saved.native()?;
        // SAFETY: eframe lends this live root HWND only on the UI thread; native
        // is a correctly sized local structure. Windows adjusts wholly offscreen
        // placements for current monitors. No handle or pointer escapes the call.
        if unsafe { SetWindowPlacement(hwnd(handle.hwnd.get()), &raw const native) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn observe(&mut self, frame: &eframe::Frame, closing: bool) -> io::Result<()> {
        if !closing
            && self
                .last_observed
                .is_some_and(|observed| observed.elapsed() < Duration::from_millis(500))
        {
            return Ok(());
        }
        self.last_observed = Some(Instant::now());
        let handle = frame.window_handle().map_err(io::Error::other)?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Ok(());
        };
        let mut native = WINDOWPLACEMENT {
            length: u32::try_from(size_of::<WINDOWPLACEMENT>()).map_err(io::Error::other)?,
            ..WINDOWPLACEMENT::default()
        };
        // SAFETY: frame lends a live root HWND on this UI thread; native is an
        // initialized, correctly sized writable structure, with no escaping pointer.
        if unsafe { GetWindowPlacement(hwnd(handle.hwnd.get()), &raw mut native) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(saved) = Saved::from_native(&native) {
            self.queue(saved)?;
        }
        Ok(())
    }

    fn queue(&mut self, saved: Saved) -> io::Result<()> {
        if self.latest == Some(saved) {
            return Ok(());
        }
        *self
            .pending
            .lock()
            .map_err(|_| io::Error::other("Window saving worker stopped."))? = Some(saved);
        self.latest = Some(saved);
        if let Some(wake) = &self.wake {
            let _queued = wake.try_send(());
        }
        Ok(())
    }
}

// Integer-to-pointer conversion is confined to the audited native call sites.
const fn hwnd(value: isize) -> HWND {
    std::ptr::without_provenance_mut(value.cast_unsigned())
}

impl Drop for Placement {
    fn drop(&mut self) {
        self.wake.take();
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

impl State {
    pub(super) fn attach_placement(
        &mut self,
        placement: io::Result<Placement>,
        cc: &eframe::CreationContext<'_>,
    ) {
        match placement {
            Ok(placement) => {
                if placement.restore(cc).is_err() {
                    self.record("Could not restore the previous window placement; using the default window.");
                }
                self.placement = Some(placement);
            }
            Err(_) => self.record(
                "Could not open window settings. Window size and position will not be saved.",
            ),
        }
    }

    pub(super) fn poll_placement(&mut self, frame: &eframe::Frame, closing: bool) {
        if let Some(placement) = &mut self.placement {
            let write_failed = placement.failed.swap(false, Ordering::Acquire);
            if write_failed {
                placement.latest = None;
            }
            let failed = placement.observe(frame, closing).is_err() || write_failed;
            if failed {
                self.record(
                    "Could not save window size and position. Conversation saving is separate.",
                );
            }
        }
    }
}

fn read(path: &Path) -> io::Result<Option<Saved>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let Some(bytes) = crate::bounded_file::read(file, MAX_BYTES)? else {
        return Ok(None);
    };
    Ok(serde_json::from_slice::<Saved>(&bytes)
        .ok()
        .filter(|saved| saved.valid()))
}

fn save(path: &Path, saved: Saved) -> io::Result<()> {
    bonsai_windows_credentials::validate_external_path(path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{stamp}.pending", std::process::id()));
    bonsai_windows_credentials::validate_external_path(&temporary)?;
    let bytes = serde_json::to_vec(&saved).map_err(io::Error::other)?;
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(&bytes)?;
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

    const fn ordinary() -> Saved {
        Saved {
            version: 1,
            normal: [-900, 100, 400, 1000],
            maximized: false,
        }
    }

    #[test]
    fn window_placement_round_trips_normal_and_maximized_without_minimized() -> io::Result<()> {
        for maximized in [false, true] {
            let saved = Saved {
                maximized,
                ..ordinary()
            };
            let mut native = saved.native()?;
            assert_eq!(Saved::from_native(&native), Some(saved));
            native.showCmd = SW_SHOWMINIMIZED.unsigned_abs();
            assert!(Saved::from_native(&native).is_none());
        }
        Ok(())
    }

    #[test]
    fn window_placement_rejects_invalid_bounds_and_oversized_files() -> io::Result<()> {
        for normal in [
            [i32::MIN, 0, i32::MAX, 1000],
            [0, 0, 10, 10],
            [10, 0, 0, 1000],
            [0, 0, 100_000, 1000],
        ] {
            assert!(
                !Saved {
                    normal,
                    ..ordinary()
                }
                .valid()
            );
        }
        let directory = directory("invalid")?;
        let path = directory.join("chat-window.json");
        for bytes in [
            b"{}".to_vec(),
            vec![b' '; usize::try_from(MAX_BYTES + 1).map_err(io::Error::other)?],
        ] {
            fs::write(&path, bytes)?;
            assert!(read(&path)?.is_none());
        }
        fs::remove_file(path)?;
        fs::remove_dir(directory)
    }

    #[test]
    fn window_placement_worker_flushes_latest_bounds_independently_of_conversations()
    -> io::Result<()> {
        let directory = directory("worker")?;
        let mut placement = Placement::open(&directory)?;
        assert!(placement.restored.is_none());
        placement.queue(ordinary())?;
        let latest = Saved {
            normal: [500, 100, 1700, 950],
            maximized: true,
            ..ordinary()
        };
        placement.queue(latest)?;
        drop(placement);
        assert_eq!(read(&directory.join("chat-window.json"))?, Some(latest));
        assert!(!directory.join("chat-recovery.dpapi").exists());
        let reopened = Placement::open(&directory)?;
        assert_eq!(reopened.restored, Some(latest));
        drop(reopened);
        fs::remove_file(directory.join("chat-window.json"))?;
        fs::remove_dir(directory)
    }

    fn directory(label: &str) -> io::Result<std::path::PathBuf> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-window-{label}-{stamp}"));
        fs::create_dir(&directory)?;
        Ok(directory)
    }
}
