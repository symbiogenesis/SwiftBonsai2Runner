//! Independent wire and channel controls; no codec, device or model is activated.

use std::{
    io::{self, Cursor, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::{
    ActiveImport, CANCELLED, CODEC_FAILURE, FLAG_ERROR, HEADER_BYTES, MEMORY_FAILURE, NEXT,
    PROTOCOL_FAILURE, Shared, TIMEOUT, read_frame, read_frames, receive_frame, relay,
};

// Independently assembled canonical PCM16: 16 kHz, mono, two samples [0x0201,
// 0x0403]. This deliberately does not call either production frame/WAV writer.
const WAVE: &[u8] = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\x3e\0\0\0\x7d\0\0\x02\0\x10\0data\x04\0\0\0\x01\x02\x03\x04";

fn wire(start: u64, last: bool) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_BYTES];
    bytes[..16].copy_from_slice(b"BSAUD001\x30\0\0\0\0\0\0\0");
    bytes[12] = u8::from(last);
    bytes[16..24].copy_from_slice(&start.to_le_bytes());
    bytes[24..32].copy_from_slice(&(start + 2).to_le_bytes());
    if last {
        // Test positions are zero and two samples, hence at most 250 microseconds.
        let nanos = if start == 0 { 125_000_u32 } else { 250_000_u32 };
        bytes[40..44].copy_from_slice(&nanos.to_le_bytes());
    } else {
        bytes[32..40].copy_from_slice(&1_u64.to_le_bytes());
    }
    bytes.extend_from_slice(WAVE);
    bytes
}

struct Fragmented {
    source: Cursor<Vec<u8>>,
    limit: usize,
    interrupted: bool,
}

impl Read for Fragmented {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let count = bytes.len().min(self.limit);
        self.source.read(&mut bytes[..count])
    }
}

fn decoded(start: u64, last: bool) -> io::Result<super::Chunk> {
    read_frame(
        &mut wire(start, last).as_slice(),
        Duration::from_nanos(start * 62_500),
    )
    .map_err(io::Error::other)
}

#[test]
fn isolated_wire_fragmentation_matches_independent_pcm_and_timestamps() -> io::Result<()> {
    for limit in 1..=17 {
        let mut source = Fragmented {
            source: Cursor::new(wire(0, true)),
            limit,
            interrupted: false,
        };
        let frame = read_frame(&mut source, Duration::ZERO).map_err(io::Error::other)?;
        assert_eq!(frame.wav, WAVE);
        assert_eq!(frame.start, Duration::ZERO);
        assert_eq!(frame.end, Duration::from_micros(125));
        assert_eq!(frame.total, Duration::from_micros(125));
        assert!(frame.last);
    }
    let bytes = wire(0, true);
    for end in 0..bytes.len() {
        assert!(matches!(
            read_frame(&mut &bytes[..end], Duration::ZERO),
            Err(PROTOCOL_FAILURE)
        ));
    }
    Ok(())
}

#[test]
fn isolated_wire_admits_exact_sample_limits_and_rejects_noncanonical_pcm() -> io::Result<()> {
    for samples in [1_u32, 480_000] {
        let payload = samples * 2;
        let length = usize::try_from(payload + 44).map_err(io::Error::other)?;
        let mut bytes = wire(0, true);
        bytes.resize(HEADER_BYTES + length, 0);
        bytes[8..12].copy_from_slice(&(payload + 44).to_le_bytes());
        bytes[24..32].copy_from_slice(&u64::from(samples).to_le_bytes());
        bytes[32..40].copy_from_slice(&u64::from(samples / 16_000).to_le_bytes());
        bytes[40..44].copy_from_slice(&((samples % 16_000) * 62_500).to_le_bytes());
        bytes[HEADER_BYTES + 4..HEADER_BYTES + 8].copy_from_slice(&(payload + 36).to_le_bytes());
        bytes[HEADER_BYTES + 40..HEADER_BYTES + 44].copy_from_slice(&payload.to_le_bytes());
        let frame = read_frame(&mut bytes.as_slice(), Duration::ZERO).map_err(io::Error::other)?;
        assert_eq!(frame.wav.len(), length);
        assert_eq!(frame.wav, bytes[HEADER_BYTES..]);
    }
    for offset in [0, 4, 8, 12, 16, 20, 22, 24, 28, 32, 34, 36, 40] {
        let mut bytes = wire(0, true);
        bytes[HEADER_BYTES + offset] ^= 1;
        assert!(matches!(
            read_frame(&mut bytes.as_slice(), Duration::ZERO),
            Err(PROTOCOL_FAILURE)
        ));
    }
    let mut changed_samples = wire(0, true);
    changed_samples[HEADER_BYTES + 44..].fill(0xff);
    let frame =
        read_frame(&mut changed_samples.as_slice(), Duration::ZERO).map_err(io::Error::other)?;
    assert_eq!(&frame.wav[44..], &[0xff; 4]);
    Ok(())
}

struct HeaderOnly {
    bytes: [u8; HEADER_BYTES],
    position: usize,
    payload_requests: usize,
}

impl HeaderOnly {
    fn new() -> io::Result<Self> {
        Ok(Self {
            bytes: wire(0, true)[..HEADER_BYTES]
                .try_into()
                .map_err(io::Error::other)?,
            position: 0,
            payload_requests: 0,
        })
    }

    fn rejects_without_payload(&mut self) {
        assert!(matches!(
            read_frame(self, Duration::ZERO),
            Err(PROTOCOL_FAILURE)
        ));
        assert_eq!(self.payload_requests, 0);
    }
}

impl Read for HeaderOnly {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.position == HEADER_BYTES {
            self.payload_requests += 1;
            return Err(io::Error::other("Rejected header requested a payload"));
        }
        let count = destination.len().min(HEADER_BYTES - self.position);
        destination[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

#[test]
fn isolated_wire_rejects_header_authority_and_geometry_before_payload() -> io::Result<()> {
    for length in [0_u32, 44, 45, 47, 960_046, u32::MAX] {
        let mut source = HeaderOnly::new()?;
        source.bytes[8..12].copy_from_slice(&length.to_le_bytes());
        source.rejects_without_payload();
    }
    for (offset, bytes) in [
        (12, 3_u32.to_le_bytes().to_vec()),
        (16, 1_u64.to_le_bytes().to_vec()),
        (24, 0_u64.to_le_bytes().to_vec()),
        (24, 3_u64.to_le_bytes().to_vec()),
        (32, 1_u64.to_le_bytes().to_vec()),
        (40, 1_000_000_000_u32.to_le_bytes().to_vec()),
    ] {
        let mut source = HeaderOnly::new()?;
        source.bytes[offset..offset + bytes.len()].copy_from_slice(&bytes);
        source.rejects_without_payload();
    }
    for offset in 44..HEADER_BYTES {
        let mut source = HeaderOnly::new()?;
        source.bytes[offset] = 1;
        source.rejects_without_payload();
    }
    Ok(())
}

#[test]
fn isolated_wire_accepts_only_fixed_error_codes_and_zero_reserved_fields() {
    for (code, expected) in [
        (0_u32, CODEC_FAILURE),
        (1, MEMORY_FAILURE),
        (2, PROTOCOL_FAILURE),
    ] {
        let mut bytes = [0; HEADER_BYTES];
        bytes[..8].copy_from_slice(b"BSAUD001");
        bytes[12..16].copy_from_slice(&FLAG_ERROR.to_le_bytes());
        bytes[16..20].copy_from_slice(&code.to_le_bytes());
        assert!(
            matches!(read_frame(&mut bytes.as_slice(), Duration::ZERO), Err(message) if message == expected)
        );
        for offset in 20..HEADER_BYTES {
            let mut changed = bytes;
            changed[offset] = 1;
            assert!(matches!(
                read_frame(&mut changed.as_slice(), Duration::ZERO),
                Err(PROTOCOL_FAILURE)
            ));
        }
    }
}

#[test]
fn isolated_receive_cancellation_deadline_and_disconnect_preserve_queued_input() -> io::Result<()> {
    let shared = Shared::default();
    let (send, frames) = mpsc::sync_channel(1);
    send.send(Ok(decoded(0, true)?))
        .map_err(|_| io::Error::other("Frame receiver closed"))?;
    shared.cancelled.store(true, Ordering::Release);
    assert!(matches!(
        receive_frame(&frames, &shared, Instant::now() + Duration::from_secs(1)),
        Err(CANCELLED)
    ));
    shared.cancelled.store(false, Ordering::Release);
    assert!(matches!(
        receive_frame(&frames, &shared, Instant::now()),
        Err(TIMEOUT)
    ));
    assert_eq!(
        frames
            .try_recv()
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?
            .wav,
        WAVE
    );
    // A live but silent producer reaches the existing absolute deadline; this
    // asserts an outcome, not a machine-specific latency threshold.
    assert!(matches!(
        receive_frame(&frames, &shared, Instant::now() + Duration::from_millis(1)),
        Err(TIMEOUT)
    ));
    drop(send);
    assert!(matches!(
        receive_frame(&frames, &shared, Instant::now() + Duration::from_secs(1)),
        Err(PROTOCOL_FAILURE)
    ));
    Ok(())
}

#[test]
fn isolated_pipe_reader_consumes_one_permit_and_leaves_flooded_tail_unread() -> io::Result<()> {
    let first = wire(0, false);
    let second = wire(2, false);
    let mut source = first.clone();
    source.extend_from_slice(&second);
    source.extend_from_slice(&wire(2, true));
    let mut reader = Cursor::new(source);
    let mut commands = Vec::new();
    let (send, frames) = mpsc::sync_channel(3);
    let (advance, permits) = mpsc::sync_channel(1);
    advance.send(()).map_err(io::Error::other)?;
    drop(advance);
    read_frames(&mut commands, &mut reader, &send, &permits);
    assert_eq!(commands, [NEXT]);
    assert_eq!(
        reader.position(),
        u64::try_from(first.len() + second.len()).map_err(io::Error::other)?
    );
    for start in [0, 2] {
        let frame = frames
            .try_recv()
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?;
        assert_eq!(frame.start, Duration::from_nanos(start * 62_500));
        assert!(!frame.last);
    }
    assert!(matches!(frames.try_recv(), Err(mpsc::TryRecvError::Empty)));
    Ok(())
}

#[test]
fn isolated_pipe_reader_rejects_terminal_surplus_and_stops_when_receiver_closes() -> io::Result<()>
{
    for surplus in [false, true] {
        let mut source = wire(0, true);
        if surplus {
            source.push(9);
        }
        let (send, frames) = mpsc::sync_channel(1);
        let (_advance, permits) = mpsc::sync_channel(1);
        let mut commands = Vec::new();
        read_frames(&mut commands, source.as_slice(), &send, &permits);
        let result = frames.try_recv().map_err(io::Error::other)?;
        if surplus {
            assert!(matches!(result, Err(PROTOCOL_FAILURE)));
        } else {
            assert!(result.is_ok());
        }
        assert_eq!(commands, b"");
    }
    let (send, frames) = mpsc::sync_channel(1);
    drop(frames);
    let (advance, permits) = mpsc::sync_channel(1);
    advance.send(()).map_err(io::Error::other)?;
    let mut source = wire(0, false);
    source.extend_from_slice(&wire(2, true));
    let mut reader = Cursor::new(source);
    let mut commands = Vec::new();
    read_frames(&mut commands, &mut reader, &send, &permits);
    assert_eq!(
        reader.position(),
        u64::try_from(HEADER_BYTES + WAVE.len()).map_err(io::Error::other)?
    );
    assert_eq!(permits.try_recv(), Ok(()));
    assert_eq!(commands, b"");
    Ok(())
}

#[test]
fn isolated_relay_grants_once_and_keeps_final_audio_out_of_ui_delivery() -> io::Result<()> {
    let shared = Shared::default();
    let (send, frames) = mpsc::sync_channel(2);
    send.send(Ok(decoded(0, false)?))
        .map_err(|_| io::Error::other("Frame receiver closed"))?;
    send.send(Ok(decoded(2, true)?))
        .map_err(|_| io::Error::other("Frame receiver closed"))?;
    let (advance, permits) = mpsc::sync_channel(2);
    let (ui, delivered) = mpsc::sync_channel(2);
    let (next, consumed) = mpsc::sync_channel(1);
    next.send(()).map_err(io::Error::other)?;
    let final_frame = relay(
        &advance,
        &frames,
        &shared,
        &ui,
        &consumed,
        Instant::now() + Duration::from_secs(1),
    )
    .map_err(io::Error::other)?;
    assert!(final_frame.last);
    assert_eq!(final_frame.end, Duration::from_micros(250));
    assert!(
        delivered
            .try_recv()
            .map_err(io::Error::other)?
            .map_err(io::Error::other)?
            .is_some_and(|frame| !frame.last && frame.wav == WAVE)
    );
    assert!(matches!(
        delivered.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(permits.try_recv(), Ok(()));
    assert!(matches!(permits.try_recv(), Err(mpsc::TryRecvError::Empty)));
    Ok(())
}

#[test]
fn isolated_relay_cancellation_during_backpressure_grants_no_further_decode() -> io::Result<()> {
    let shared = Arc::new(Shared::default());
    let worker_state = Arc::clone(&shared);
    let (send, frames) = mpsc::sync_channel(1);
    send.send(Ok(decoded(0, false)?))
        .map_err(|_| io::Error::other("Frame receiver closed"))?;
    let (advance, permits) = mpsc::sync_channel(1);
    let (ui, delivered) = mpsc::sync_channel(1);
    let (_next, consumed) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        relay(
            &advance,
            &frames,
            &worker_state,
            &ui,
            &consumed,
            Instant::now() + Duration::from_secs(1),
        )
    });
    let result = (|| {
        let first = delivered
            .recv_timeout(Duration::from_secs(1))
            .map_err(io::Error::other)?;
        if first
            .map_err(io::Error::other)?
            .is_none_or(|frame| frame.last)
        {
            return Err(io::Error::other("Relay did not deliver its nonfinal frame"));
        }
        Ok::<(), io::Error>(())
    })();
    // Always release the worker, including failure of the bounded observation.
    shared.cancelled.store(true, Ordering::Release);
    let stopped = worker
        .join()
        .map_err(|_| io::Error::other("Relay worker panicked"))?;
    result?;
    assert!(matches!(stopped, Err(CANCELLED)));
    assert!(matches!(
        permits.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(matches!(
        delivered.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    Ok(())
}

#[test]
fn isolated_active_admission_is_exclusive_and_recovers_only_on_owner_drop() -> io::Result<()> {
    let slot = AtomicBool::new(false);
    let owner = ActiveImport::acquire(&slot).map_err(io::Error::other)?;
    assert!(slot.load(Ordering::Acquire));
    assert!(ActiveImport::acquire(&slot).is_err());
    assert!(slot.load(Ordering::Acquire));
    drop(owner);
    let fresh = ActiveImport::acquire(&slot).map_err(io::Error::other)?;
    assert!(ActiveImport::acquire(&slot).is_err());
    drop(fresh);
    assert!(!slot.load(Ordering::Acquire));
    Ok(())
}

#[test]
fn isolated_worker_rejects_direct_invocation_before_codec_activation() -> io::Result<()> {
    use std::{ffi::OsString, process::ExitCode};

    for arguments in [
        Vec::new(),
        vec![OsString::from("--audio-import-worker")],
        vec![OsString::from("--preview"), OsString::from("input.wav")],
        vec![
            OsString::from("--audio-import-worker"),
            OsString::from("input.wav"),
            OsString::from("extra"),
        ],
    ] {
        assert_eq!(super::run_worker(&arguments), ExitCode::FAILURE);
    }
    // This acceptance control must run in the ordinary host test process. A
    // valid argument shape still cannot activate codecs without kernel isolation.
    if bonsai_windows_credentials::is_app_container()? {
        return Err(io::Error::other(
            "Direct-invocation control requires an ordinary host",
        ));
    }
    assert_eq!(
        super::run_worker(&[
            OsString::from("--audio-import-worker"),
            OsString::from("C:\\unprovisioned-input-capability-control.wav"),
        ]),
        ExitCode::FAILURE
    );
    Ok(())
}
