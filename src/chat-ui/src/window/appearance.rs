//! Device-wide reading preferences, independent of conversation recovery.
use super::{egui, platform};
use egui::{Color32, FontId};
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

const MAX_BYTES: u64 = 4_096;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub(super) const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    const fn preference(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preferences {
    pub(super) theme: Theme,
    pub(super) text_size: u16,
    pub(super) reading_width: u16,
    pub(super) sidebar_width: f32,
    pub(super) sidebar_visible: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            text_size: 16,
            reading_width: 800,
            sidebar_width: 256.0,
            sidebar_visible: true,
        }
    }
}

impl Preferences {
    fn valid(self) -> bool {
        (14..=24).contains(&self.text_size)
            && (640..=1_040).contains(&self.reading_width)
            && self.sidebar_width.is_finite()
            && (216.0..=340.0).contains(&self.sidebar_width)
    }

    pub(super) fn column_width(self) -> f32 {
        f32::from(self.reading_width)
    }

    pub(super) fn apply(self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::unique("reading-text-size"),
                f32::from(self.text_size),
            );
        });
        ctx.set_theme(self.theme.preference());
        ctx.set_visuals(platform::high_contrast_visuals().unwrap_or_else(|| palette(ctx.theme())));
    }
}

pub(super) fn text_size(ctx: &egui::Context) -> f32 {
    ctx.data(|data| {
        data.get_temp::<f32>(egui::Id::unique("reading-text-size"))
            .unwrap_or(16.0)
    })
}

pub(super) fn configure(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let mut style = (*ctx.style_of(theme)).clone();
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(16.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 9.0);
        style.spacing.interact_size.y = 34.0;
        style.visuals = palette(theme);
        ctx.set_style_of(theme, style);
    }
    Preferences::default().apply(ctx);
}

fn palette(theme: egui::Theme) -> egui::Visuals {
    let mut visuals = theme.default_visuals();
    if theme == egui::Theme::Dark {
        visuals.panel_fill = Color32::from_rgb(20, 25, 24);
        visuals.window_fill = Color32::from_rgb(27, 33, 31);
        visuals.extreme_bg_color = Color32::from_rgb(24, 30, 28);
        visuals.faint_bg_color = Color32::from_rgb(35, 43, 39);
        visuals.selection.bg_fill = Color32::from_rgb(154, 182, 150);
        visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(17, 31, 21));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(42, 51, 46);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 71, 62);
        visuals.widgets.active.bg_fill = Color32::from_rgb(66, 84, 70);
        visuals.error_fg_color = Color32::from_rgb(246, 145, 145);
    } else {
        visuals.panel_fill = Color32::from_rgb(250, 248, 242);
        visuals.window_fill = Color32::from_rgb(242, 241, 232);
        visuals.extreme_bg_color = Color32::from_rgb(255, 254, 249);
        visuals.faint_bg_color = Color32::from_rgb(235, 239, 228);
        visuals.code_bg_color = Color32::from_rgb(228, 234, 222);
        visuals.weak_text_color = Some(Color32::from_rgb(94, 106, 92));
        visuals.selection.bg_fill = Color32::from_rgb(203, 222, 191);
        visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(30, 57, 33));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(230, 235, 222);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(217, 229, 207);
        visuals.widgets.active.bg_fill = Color32::from_rgb(199, 219, 187);
        visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1.0, Color32::from_rgb(213, 218, 204));
        visuals.warn_fg_color = Color32::from_rgb(130, 77, 15);
        visuals.error_fg_color = Color32::from_rgb(164, 48, 48);
    }
    // A global text override also overrides the contrasting selection ink on
    // selectable buttons, including dropdown entries. Theme ordinary widget ink
    // instead, so selected controls can use their own foreground/background pair.
    let ink = if theme == egui::Theme::Dark {
        Color32::from_rgb(226, 232, 226)
    } else {
        Color32::from_rgb(39, 49, 40)
    };
    visuals.widgets.noninteractive.fg_stroke.color = ink;
    visuals.widgets.inactive.fg_stroke.color = ink;
    visuals.widgets.hovered.fg_stroke.color = ink;
    visuals.widgets.active.fg_stroke.color = ink;
    visuals.widgets.open.fg_stroke.color = ink;
    visuals
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    preferences: Preferences,
}

pub(super) struct Store {
    latest: Preferences,
    pending: Arc<Mutex<Option<Preferences>>>,
    wake: Option<SyncSender<()>>,
    failed: Arc<AtomicBool>,
    retry: Arc<AtomicBool>,
    last_attempt: Instant,
    worker: Option<JoinHandle<()>>,
}

impl Store {
    pub(super) fn open(data: &Path) -> io::Result<(Self, Preferences)> {
        let path = data.join("chat-appearance.json");
        bonsai_windows_credentials::validate_external_path(&path)?;
        let preferences = read(&path).unwrap_or_default();
        let pending = Arc::new(Mutex::new(None));
        let queued = Arc::clone(&pending);
        let failed = Arc::new(AtomicBool::new(false));
        let reported = Arc::clone(&failed);
        let retry = Arc::new(AtomicBool::new(false));
        let retry_requested = Arc::clone(&retry);
        let (wake, incoming) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("swift-chat-appearance".to_owned())
            .spawn(move || {
                while incoming.recv().is_ok() {
                    let preferences = queued.lock().ok().and_then(|mut pending| pending.take());
                    if let Some(preferences) = preferences
                        && save(&path, preferences).is_err()
                    {
                        reported.store(true, Ordering::Release);
                        retry_requested.store(true, Ordering::Release);
                    }
                }
            })?;
        Ok((
            Self {
                latest: preferences,
                pending,
                wake: Some(wake),
                failed,
                retry,
                last_attempt: Instant::now(),
                worker: Some(worker),
            },
            preferences,
        ))
    }

    pub(super) fn queue(&mut self, preferences: Preferences) -> io::Result<()> {
        if !preferences.valid() {
            return Err(io::Error::other("Invalid appearance preferences."));
        }
        if preferences == self.latest
            && (!self.retry.load(Ordering::Acquire)
                || self.last_attempt.elapsed() < Duration::from_secs(3))
        {
            return Ok(());
        }
        self.retry.store(false, Ordering::Release);
        *self
            .pending
            .lock()
            .map_err(|_| io::Error::other("Appearance saving worker stopped."))? =
            Some(preferences);
        if let Some(wake) = &self.wake {
            match wake.try_send(()) {
                Ok(()) | Err(mpsc::TrySendError::Full(())) => {}
                Err(mpsc::TrySendError::Disconnected(())) => {
                    return Err(io::Error::other("Appearance saving worker stopped."));
                }
            }
        }
        self.latest = preferences;
        self.last_attempt = Instant::now();
        Ok(())
    }

    pub(super) fn take_error(&self) -> bool {
        self.failed.swap(false, Ordering::Acquire)
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

fn read(path: &Path) -> io::Result<Preferences> {
    let bytes = crate::bounded_file::read(File::open(path)?, MAX_BYTES)?
        .ok_or_else(|| io::Error::other("Appearance settings exceed their size limit."))?;
    let saved: Saved = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if saved.version != 1 || !saved.preferences.valid() {
        return Err(io::Error::other("Invalid appearance settings."));
    }
    Ok(saved.preferences)
}

fn save(path: &Path, preferences: Preferences) -> io::Result<()> {
    bonsai_windows_credentials::validate_external_path(path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{stamp}.pending", std::process::id()));
    bonsai_windows_credentials::validate_external_path(&temporary)?;
    let bytes = serde_json::to_vec(&Saved {
        version: 1,
        preferences,
    })
    .map_err(io::Error::other)?;
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

    #[test]
    fn appearance_preferences_reject_unbounded_or_nonfinite_geometry() {
        assert!(Preferences::default().valid());
        for sidebar_width in [f32::NAN, f32::INFINITY, 215.0, 341.0] {
            assert!(
                !Preferences {
                    sidebar_width,
                    ..Preferences::default()
                }
                .valid()
            );
        }
        assert!(
            !Preferences {
                text_size: 25,
                ..Preferences::default()
            }
            .valid()
        );
        assert!(
            !Preferences {
                reading_width: 639,
                ..Preferences::default()
            }
            .valid()
        );
    }

    #[test]
    fn appearance_preferences_update_reader_size_and_selected_theme() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let preferences = Preferences {
            theme: Theme::Light,
            text_size: 22,
            ..Preferences::default()
        };
        preferences.apply(&ctx);
        assert_eq!(text_size(&ctx).to_bits(), 22.0_f32.to_bits());
        assert_eq!(ctx.theme(), egui::Theme::Light);
        Preferences {
            theme: Theme::Dark,
            ..preferences
        }
        .apply(&ctx);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
    }

    #[test]
    fn appearance_store_flushes_latest_and_defaults_invalid_files() -> io::Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-appearance-{stamp}"));
        fs::create_dir(&directory)?;
        let (mut store, initial) = Store::open(&directory)?;
        assert_eq!(initial, Preferences::default());
        store.queue(Preferences {
            text_size: 20,
            ..initial
        })?;
        let latest = Preferences {
            theme: Theme::Light,
            text_size: 24,
            reading_width: 1_040,
            sidebar_width: 216.0,
            sidebar_visible: false,
        };
        store.queue(latest)?;
        drop(store);
        let path = directory.join("chat-appearance.json");
        assert_eq!(read(&path)?, latest);
        let (store, reopened) = Store::open(&directory)?;
        assert_eq!(reopened, latest);
        drop(store);
        for bytes in [b"{}".to_vec(), vec![b' '; 4_097]] {
            fs::write(&path, bytes)?;
            let (store, preferences) = Store::open(&directory)?;
            assert_eq!(preferences, Preferences::default());
            drop(store);
        }
        fs::remove_file(path)?;
        fs::remove_dir(directory)
    }
}
