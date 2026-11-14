//! One offline native decoder process per explicitly selected recording.
//!
//! Only bounded canonical WAV segments cross this boundary. The selected source
//! and its ancestors stay locked through confirmed process cleanup; neither the
//! ASR service nor this pipe protocol can request another pathname.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::{Component, Path},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bonsai_windows_credentials::PrivateDirectory;
use bonsai_windows_process::{OwnedProcess, ProcessSpec, SandboxNetwork, SandboxPolicy};

use super::{
    CANCELLED, CHUNK_TOO_LONG, CHUNK_WORKING_BYTES, CODEC_FAILURE, CONVERSION_LIMIT, Chunk,
    ImportResult, MAX_SAMPLES, MEMORY_FAILURE, OsString, POLL, Shared, await_advance, check_memory,
    convert_with, sample_duration,
};

const HEADER_BYTES: usize = 48;
const MAGIC: &[u8; 8] = b"BSAUD001";
const FLAG_LAST: u32 = 1;
const FLAG_ERROR: u32 = 2;
const NEXT: u8 = b'N';
const PROCESS_COMMIT_LIMIT: u64 = 256 * 1024 * 1024;
// Charge the complete helper allowance in addition to the existing host/audio
// pipeline buffer ledger and both requested 64 KiB kernel pipe buffers.
const ISOLATED_WORKING_BYTES: u64 = PROCESS_COMMIT_LIMIT + CHUNK_WORKING_BYTES + 128 * 1024;
const CLEANUP_LIMIT: Duration = Duration::from_secs(2);
const ISOLATION_FAILURE: &str = "Windows could not start the isolated audio decoder. Transcription stopped. Your draft was kept.";
const PROTOCOL_FAILURE: &str = "The isolated audio decoder returned invalid or incomplete audio. Transcription stopped. Your draft was kept.";
const TIMEOUT: &str = "Windows did not finish decoding the next audio segment. Try converting the recording to WAV. Your draft was kept.";
const CLEANUP_FAILURE: &str = "Windows could not confirm isolated audio decoder cleanup. Restart the app before importing another file. Your draft was kept.";
const SOURCE_FAILURE: &str = "Choose a nonempty regular audio file on a local drive. The file must stay unchanged until transcription finishes. Your draft was kept.";
static ADMISSION_CLOSED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static QUARANTINE: Mutex<Option<Quarantine>> = Mutex::new(None);

struct ActiveImport;

impl ActiveImport {
    fn acquire() -> Result<Self, &'static str> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(
                |_| "Another audio import is still active. Cancel it before selecting a new file.",
            )
    }
}

impl Drop for ActiveImport {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

// At most one import is admitted, and failed cleanup permanently closes new
// admission. Retain this single quarantine until process exit when native death
// is unknown; dropped Job/PSEC handles still request terminal OS enforcement.
struct Quarantine {
    _source: Selection,
    _executable: Selection,
    _scratch: Scratch,
    _reader: Option<JoinHandle<()>>,
}

fn quarantine(
    source: Selection,
    executable: Selection,
    scratch: Scratch,
    reader: Option<JoinHandle<()>>,
) {
    ADMISSION_CLOSED.store(true, Ordering::Release);
    *QUARANTINE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Quarantine {
        _source: source,
        _executable: executable,
        _scratch: scratch,
        _reader: reader,
    });
}

/// No file contents are read or hashed during authorization. Retained host
/// directory readers deny ancestor rename; the leaf reader also denies writes.
struct Selection {
    _ancestors: Vec<File>,
    _file: File,
}

impl Selection {
    fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute()
            || !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
            || path
                .components()
                .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        {
            return Err(io::Error::other(SOURCE_FAILURE));
        }
        bonsai_windows_process::require_local_data_drive(path)?;
        let mut ancestors = Vec::new();
        // Retain parents from the drive root down before opening the leaf, so
        // a later path grant cannot select a replacement through an ancestor.
        for ancestor in path
            .ancestors()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let directory = OpenOptions::new()
                .read(true)
                // FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES. Metadata-only
                // opens do not enforce denied-delete sharing for renames.
                .access_mode(0x81)
                .share_mode(3)
                .custom_flags(0x0220_0000)
                .open(ancestor)?;
            let metadata = directory.metadata()?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other(SOURCE_FAILURE));
            }
            ancestors.push(directory);
        }
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(SOURCE_FAILURE));
        }
        Ok(Self {
            _ancestors: ancestors,
            _file: file,
        })
    }
}

struct Scratch(Option<PrivateDirectory>);

impl Scratch {
    fn create() -> io::Result<Self> {
        PrivateDirectory::create(&std::env::temp_dir()).map(|directory| Self(Some(directory)))
    }

    fn path(&self) -> io::Result<&Path> {
        self.0
            .as_ref()
            .map(PrivateDirectory::path)
            .ok_or_else(|| io::Error::other(CLEANUP_FAILURE))
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let Some(directory) = self.0.take() else {
            return Ok(());
        };
        let path = directory.path().to_owned();
        // This worker owns only these fixed entries. Unknown files are retained
        // privately and reported, rather than traversed or silently discarded.
        let stderr = path.join("decoder.stderr");
        if stderr.try_exists()? {
            fs::remove_file(stderr)?;
        }
        let worker = path.join("worker");
        if worker.try_exists()? {
            fs::remove_dir(worker)?;
        }
        drop(directory);
        fs::remove_dir(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = self.cleanup();
    }
}

struct Prepared {
    source: Selection,
    executable_guard: Selection,
    scratch: Scratch,
    specification: ProcessSpec,
    policy: SandboxPolicy,
}

fn prepare(path: &Path) -> Result<Prepared, &'static str> {
    check_memory(ISOLATED_WORKING_BYTES)?;
    let source = Selection::open(path).map_err(|_| SOURCE_FAILURE)?;
    let executable = std::env::current_exe().map_err(|_| ISOLATION_FAILURE)?;
    let executable_guard = Selection::open(&executable).map_err(|_| ISOLATION_FAILURE)?;
    let scratch = Scratch::create().map_err(|_| ISOLATION_FAILURE)?;
    let directory = scratch.path().map_err(|_| ISOLATION_FAILURE)?;
    let working_directory = directory.join("worker");
    fs::create_dir(&working_directory).map_err(|_| ISOLATION_FAILURE)?;
    let specification = ProcessSpec {
        executable: executable.clone(),
        arguments: vec![
            OsString::from("--audio-import-worker"),
            path.as_os_str().to_owned(),
        ],
        working_directory: working_directory.clone(),
        stdout: directory.join("decoder.stdout"),
        stderr: directory.join("decoder.stderr"),
        memory_limit_bytes: usize::try_from(PROCESS_COMMIT_LIMIT).map_err(|_| ISOLATION_FAILURE)?,
        active_process_limit: 1,
    };
    let policy = SandboxPolicy {
        readonly_paths: vec![executable, path.to_owned()],
        writable_directory: working_directory,
    };
    Ok(Prepared {
        source,
        executable_guard,
        scratch,
        specification,
        policy,
    })
}

pub(super) fn convert(
    path: &Path,
    shared: &Shared,
    sender: &SyncSender<ImportResult>,
    next: &Receiver<()>,
) -> Result<(), &'static str> {
    let _active = ActiveImport::acquire()?;
    if ADMISSION_CLOSED.load(Ordering::Acquire) {
        return Err(CLEANUP_FAILURE);
    }
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(CANCELLED);
    }
    let Prepared {
        source,
        executable_guard,
        mut scratch,
        specification,
        policy,
    } = prepare(path)?;
    let deadline = Instant::now() + CONVERSION_LIMIT;
    let (process, input, output) =
        match OwnedProcess::start_sandboxed_piped(&specification, &policy, SandboxNetwork::Offline)
        {
            Ok(started) => started,
            Err(error) if !bonsai_windows_process::process_start_cleanup_unconfirmed(&error) => {
                return Err(ISOLATION_FAILURE);
            }
            Err(_) => {
                // A native creation/cleanup error can leave process death unknown.
                // Retain authority conservatively rather than reopen a source path.
                quarantine(source, executable_guard, scratch, None);
                return Err(CLEANUP_FAILURE);
            }
        };
    let (frames, received) = mpsc::sync_channel(1);
    let (advance, permits) = mpsc::sync_channel(1);
    let Ok(reader) = thread::Builder::new()
        .name("bonsai-audio-decoder-pipe".to_owned())
        .spawn(move || read_frames(input, output, &frames, &permits))
    else {
        if process.stop(CLEANUP_LIMIT).is_err() {
            quarantine(source, executable_guard, scratch, None);
            drop(process);
            return Err(CLEANUP_FAILURE);
        }
        drop(process);
        return Err(ISOLATION_FAILURE);
    };
    let result = relay(&advance, &received, shared, sender, next, deadline);
    // A compromised helper cannot keep a pipe reader blocked on channel delivery
    // during joined shutdown. Dropping permits also wakes its between-frame wait.
    drop(received);
    drop(advance);
    // Successful EOF still requires process exit and an empty Job. Every other
    // path terminates the exact owned helper, unblocking native codec/pipe calls.
    let clean = match &result {
        Ok(_) => process
            .wait_for_exit(CLEANUP_LIMIT)
            .and_then(|code| match code {
                Some(0) if process.active_processes()? == 0 => Ok(()),
                _ => Err(io::Error::other(CLEANUP_FAILURE)),
            }),
        Err(_) => process.stop(CLEANUP_LIMIT),
    };
    if clean.is_err() {
        quarantine(source, executable_guard, scratch, Some(reader));
        // Close both native owners for terminal OS enforcement, while retaining
        // path identities and IO ownership until application exit.
        drop(process);
        return Err(CLEANUP_FAILURE);
    }
    join_reader(reader)?;
    drop(process);
    if scratch.cleanup().is_err() {
        ADMISSION_CLOSED.store(true, Ordering::Release);
        return Err(CLEANUP_FAILURE);
    }
    let final_chunk = result?;
    // Final output is admitted only after verified native teardown and scratch
    // cleanup. This prevents a terminal audio result from hiding failed cleanup.
    if !shared.cancelled.load(Ordering::Acquire) {
        let _sent = sender.send(Ok(Some(final_chunk)));
    }
    Ok(())
}

fn join_reader(reader: JoinHandle<()>) -> Result<(), &'static str> {
    reader.join().map_err(|_| PROTOCOL_FAILURE)
}

fn read_frames(
    mut input: File,
    mut output: File,
    frames: &SyncSender<Result<Chunk, &'static str>>,
    permits: &Receiver<()>,
) {
    let mut expected_start = Duration::ZERO;
    loop {
        let result = read_frame(&mut output, expected_start).and_then(|chunk| {
            if chunk.last {
                let mut surplus = [0];
                if output.read(&mut surplus).map_err(|_| PROTOCOL_FAILURE)? != 0 {
                    return Err(PROTOCOL_FAILURE);
                }
            }
            Ok(chunk)
        });
        let terminal = result.as_ref().map_or(true, |chunk| {
            expected_start = chunk.end;
            chunk.last
        });
        if frames.send(result).is_err() || terminal {
            break;
        }
        if permits.recv().is_err() || input.write_all(&[NEXT]).is_err() {
            break;
        }
    }
}

fn relay(
    advance: &SyncSender<()>,
    frames: &Receiver<Result<Chunk, &'static str>>,
    shared: &Shared,
    sender: &SyncSender<ImportResult>,
    next: &Receiver<()>,
    initial_deadline: Instant,
) -> Result<Chunk, &'static str> {
    let mut deadline = initial_deadline;
    loop {
        let chunk = receive_frame(frames, shared, deadline)?;
        if chunk.last {
            return Ok(chunk);
        }
        if sender.send(Ok(Some(chunk))).is_err() {
            return Err(CANCELLED);
        }
        await_advance(shared, next)?;
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        // Native writes stay on the supervised IO thread too: a compromised
        // helper may produce frames without consuming its stdin permits.
        advance.try_send(()).map_err(|_| PROTOCOL_FAILURE)?;
        deadline = Instant::now() + CONVERSION_LIMIT;
    }
}

fn receive_frame(
    frames: &Receiver<Result<Chunk, &'static str>>,
    shared: &Shared,
    deadline: Instant,
) -> Result<Chunk, &'static str> {
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(CANCELLED);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TIMEOUT);
        }
        match frames.recv_timeout(POLL.min(remaining)) {
            Ok(result) => {
                return if shared.cancelled.load(Ordering::Acquire) {
                    Err(CANCELLED)
                } else {
                    result
                };
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(PROTOCOL_FAILURE),
        }
    }
}

fn read_frame(read: &mut impl Read, expected_start: Duration) -> Result<Chunk, &'static str> {
    let mut header = [0; HEADER_BYTES];
    read.read_exact(&mut header).map_err(|_| PROTOCOL_FAILURE)?;
    if &header[..8] != MAGIC || header[44..] != [0; 4] {
        return Err(PROTOCOL_FAILURE);
    }
    let length = usize::try_from(u32_at(&header, 8)?).map_err(|_| PROTOCOL_FAILURE)?;
    let flags = u32_at(&header, 12)?;
    if flags == FLAG_ERROR && length == 0 {
        let code = u32_at(&header, 16)?;
        if code > 1 || header[20..].iter().any(|byte| *byte != 0) {
            return Err(PROTOCOL_FAILURE);
        }
        return Err(if code == 1 {
            MEMORY_FAILURE
        } else {
            CODEC_FAILURE
        });
    }
    if flags > FLAG_LAST
        || !(46..=MAX_SAMPLES * 2 + 44).contains(&length)
        || !length.is_multiple_of(2)
    {
        return Err(PROTOCOL_FAILURE);
    }
    check_memory(CHUNK_WORKING_BYTES)?;
    let start = sample_duration(u64_at(&header, 16)?)?;
    let end = sample_duration(u64_at(&header, 24)?)?;
    let nanos = u32_at(&header, 40)?;
    if nanos >= 1_000_000_000 {
        return Err(PROTOCOL_FAILURE);
    }
    let total = Duration::new(u64_at(&header, 32)?, nanos);
    let samples = u64::try_from((length - 44) / 2).map_err(|_| PROTOCOL_FAILURE)?;
    if start != expected_start
        || end <= start
        || end.checked_sub(start) != Some(sample_duration(samples)?)
        || (flags == FLAG_LAST && total != end)
    {
        return Err(PROTOCOL_FAILURE);
    }
    let mut wav = Vec::new();
    wav.try_reserve_exact(length).map_err(|_| MEMORY_FAILURE)?;
    wav.resize(length, 0);
    read.read_exact(&mut wav).map_err(|_| PROTOCOL_FAILURE)?;
    if bonsai_audio::mono_pcm16_data(
        &wav,
        16_000,
        u32::try_from(MAX_SAMPLES).map_err(|_| PROTOCOL_FAILURE)?,
    )
    .is_none()
    {
        return Err(PROTOCOL_FAILURE);
    }
    Ok(Chunk {
        wav,
        start,
        end,
        total,
        last: flags == FLAG_LAST,
    })
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(PROTOCOL_FAILURE)
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, &'static str> {
    bytes
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(PROTOCOL_FAILURE)
}

fn samples(duration: Duration) -> Result<u64, &'static str> {
    duration
        .as_secs()
        .checked_mul(16_000)
        .and_then(|whole| whole.checked_add(u64::from(duration.subsec_nanos() / 62_500)))
        .ok_or(CHUNK_TOO_LONG)
}

fn write_frame(write: &mut impl Write, chunk: &Chunk) -> Result<(), &'static str> {
    let mut header = [0; HEADER_BYTES];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(
        &u32::try_from(chunk.wav.len())
            .map_err(|_| CHUNK_TOO_LONG)?
            .to_le_bytes(),
    );
    header[12..16].copy_from_slice(&u32::from(chunk.last).to_le_bytes());
    header[16..24].copy_from_slice(&samples(chunk.start)?.to_le_bytes());
    header[24..32].copy_from_slice(&samples(chunk.end)?.to_le_bytes());
    header[32..40].copy_from_slice(&chunk.total.as_secs().to_le_bytes());
    header[40..44].copy_from_slice(&chunk.total.subsec_nanos().to_le_bytes());
    write.write_all(&header).map_err(|_| CANCELLED)?;
    write.write_all(&chunk.wav).map_err(|_| CANCELLED)?;
    // StdoutLock can buffer a small PCM tail. Flush one complete frame before
    // waiting for the next permit, including recordings with no newline bytes.
    write.flush().map_err(|_| CANCELLED)
}

pub(super) fn run_worker(arguments: &[OsString]) -> std::process::ExitCode {
    // A direct command-line invocation never activates codecs without actual
    // kernel containment. This entry point exposes no picker/device operation.
    if arguments.len() != 2
        || arguments[0] != "--audio-import-worker"
        || !bonsai_windows_credentials::is_app_container().unwrap_or(false)
    {
        return std::process::ExitCode::FAILURE;
    }
    let source = Path::new(&arguments[1]);
    let shared = Shared::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let result = convert_with(source, &shared, &mut |chunk| {
        write_frame(&mut output, &chunk)?;
        if chunk.last {
            return Ok(false);
        }
        let mut command = [0];
        input.read_exact(&mut command).map_err(|_| CANCELLED)?;
        if command[0] != NEXT {
            return Err(CANCELLED);
        }
        Ok(true)
    });
    if let Err(message) = result {
        let mut header = [0; HEADER_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[12..16].copy_from_slice(&FLAG_ERROR.to_le_bytes());
        header[16..20].copy_from_slice(&u32::from(message == MEMORY_FAILURE).to_le_bytes());
        let _reported = output.write_all(&header);
        let _flushed = output.flush();
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{
        FLAG_LAST, HEADER_BYTES, MAGIC, PROTOCOL_FAILURE, Selection, read_frame, write_frame,
    };
    use crate::audio_import::{Chunk, MAX_SAMPLES, sample_duration};
    use std::{
        fs, io,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn chunk(last: bool) -> io::Result<Chunk> {
        let mut wav = bonsai_audio::mono_pcm16_header(16_000, 2)
            .ok_or_else(|| io::Error::other("header"))?
            .to_vec();
        wav.extend_from_slice(&[1, 2, 3, 4]);
        let end = sample_duration(2).map_err(io::Error::other)?;
        Ok(Chunk {
            wav,
            start: Duration::ZERO,
            end,
            total: if last { end } else { Duration::from_secs(60) },
            last,
        })
    }

    #[test]
    fn isolated_frames_preserve_pcm_and_reject_changed_identity_and_truncation() -> io::Result<()> {
        let expected = chunk(true)?;
        let mut wire = Vec::new();
        write_frame(&mut wire, &expected).map_err(io::Error::other)?;
        let actual = read_frame(&mut wire.as_slice(), Duration::ZERO).map_err(io::Error::other)?;
        assert_eq!(actual.wav, expected.wav);
        assert_eq!(actual.end, expected.end);
        assert!(actual.last);
        for offset in [0, 8, 12, 16, 24, 32, 40, 44, HEADER_BYTES] {
            let mut changed = wire.clone();
            changed[offset] ^= 0x80;
            assert!(read_frame(&mut changed.as_slice(), Duration::ZERO).is_err());
        }
        for length in [0, 7, HEADER_BYTES - 1, HEADER_BYTES, wire.len() - 1] {
            assert!(read_frame(&mut &wire[..length], Duration::ZERO).is_err());
        }
        assert!(read_frame(&mut wire.as_slice(), Duration::from_secs(1)).is_err());
        Ok(())
    }

    #[test]
    fn isolated_frames_reject_excess_bytes_before_allocating_the_payload() -> io::Result<()> {
        let mut header = [0; HEADER_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(
            &u32::try_from(MAX_SAMPLES * 2 + 46)
                .map_err(io::Error::other)?
                .to_le_bytes(),
        );
        header[12..16].copy_from_slice(&FLAG_LAST.to_le_bytes());
        assert!(matches!(
            read_frame(&mut header.as_slice(), Duration::ZERO),
            Err(PROTOCOL_FAILURE)
        ));
        Ok(())
    }

    #[test]
    fn isolated_frames_flush_small_recordings_before_waiting_for_permit() -> io::Result<()> {
        // The stdio boundary can buffer an entire short clip without a newline.
        // Its frame must be visible before the decoder blocks for its next permit.
        let mut output = io::BufWriter::with_capacity(4_096, Vec::new());
        let expected = chunk(false)?;
        write_frame(&mut output, &expected).map_err(io::Error::other)?;
        assert_eq!(output.buffer(), b"");
        assert_eq!(output.get_ref().len(), HEADER_BYTES + expected.wav.len());
        Ok(())
    }

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            for name in ["input.wav", "sibling"] {
                let _removed = fs::remove_file(self.0.join(name));
            }
            let _removed = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn isolated_selection_retains_file_and_ancestors_without_blocking_sibling_writes()
    -> io::Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "Bonsai-SelectedAudio-{}-{}",
            std::process::id(),
            stamp.as_nanos()
        ));
        fs::create_dir(&path)?;
        let mut fixture = Fixture(path.clone());
        let input = path.join("input.wav");
        fs::write(&input, [1, 2, 3, 4])?;
        let selected = Selection::open(&input)?;
        assert!(fs::write(&input, [9, 9]).is_err());
        let moved = path.with_extension("moved");
        assert!(fs::rename(&path, &moved).is_err());
        fs::write(path.join("sibling"), [5, 6])?;
        assert_eq!(fs::read(&input)?, [1, 2, 3, 4]);
        drop(selected);
        fs::rename(&path, &moved)?;
        fixture.0 = moved;
        Ok(())
    }
}
