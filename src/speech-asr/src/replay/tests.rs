//! Literal miniature-graph replay answers; no expected values come from capture.

use super::{Binding, Provider, Replay, Stage, Tolerance, Tolerances};
use crate::{
    protocol::PromptTokens,
    rust_engine::{
        ARITHMETIC_IDENTITY, Observation,
        tests::{assert_cleared, recognizer},
    },
};
use sha2::{Digest as _, Sha256};
use std::io;

fn binding() -> Binding<'static> {
    Binding {
        reference: Provider {
            revision: [1; 20],
            compiler: "independent mathematical fixture",
            target: "mathematical-f32",
            executable: [2; 32],
            arithmetic: "literal-asr-miniature-v1",
        },
        candidate: Provider {
            revision: [3; 20],
            compiler: "test-compiler",
            target: "test-target",
            executable: [4; 32],
            arithmetic: ARITHMETIC_IDENTITY,
        },
        model: [5; 32],
        pcm: Sha256::digest([0; 201 * 4]).into(),
    }
}

fn number(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend(value.to_le_bytes());
}
fn text(bytes: &mut Vec<u8>, value: &str) {
    number(bytes, u64::try_from(value.len()).unwrap_or_default());
    bytes.extend(value.as_bytes());
}
fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    number(bytes, u64::try_from(values.len()).unwrap_or_default());
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}

struct Packet {
    bytes: Vec<u8>,
    frontend: usize,
    encoder: usize,
    prompt: usize,
    decoder: usize,
    cache: usize,
    terminal: usize,
}

fn packet() -> Packet {
    let binding = binding();
    let mut bytes = b"ASRREPL1".to_vec();
    for provider in [&binding.reference, &binding.candidate] {
        bytes.extend(provider.revision);
        text(&mut bytes, provider.compiler);
        text(&mut bytes, provider.target);
        bytes.extend(provider.executable);
        text(&mut bytes, provider.arithmetic);
    }
    bytes.extend(binding.model);
    bytes.extend(binding.pcm);
    bytes.extend([0; 4]);
    let frontend = bytes.len();
    bytes.push(1);
    number(&mut bytes, 1);
    floats(&mut bytes, &[-1.5; 128]);
    let encoder = bytes.len();
    bytes.push(2);
    number(&mut bytes, 1);
    number(&mut bytes, 2);
    floats(&mut bytes, &[2.0, -3.0]);
    let prompt = bytes.len();
    bytes.push(3);
    number(&mut bytes, 16);
    for id in [1_u32, 4, 3, 2, 3, 1, 5, 3, 8, 9, 10, 2, 3, 1, 6, 3] {
        bytes.extend(id.to_le_bytes());
    }
    number(&mut bytes, 9);
    number(&mut bytes, 10);
    let decoder = bytes.len();
    bytes.push(4);
    number(&mut bytes, 0);
    floats(&mut bytes, &[1.0 / 1.5_f32.sqrt(); 11]);
    bytes.extend(0_u32.to_le_bytes());
    number(&mut bytes, 16);
    number(&mut bytes, 2);
    let cache = bytes.len();
    for layer in 0..2 {
        number(&mut bytes, layer);
        // Independently known RoPE zero signs and F16-rounded unit/audio rows.
        let mut keys = [0.0_f32; 32];
        for index in [4, 6, 9, 16, 18, 21, 30] {
            keys[index] = -0.0;
        }
        floats(&mut bytes, &keys);
        let mut values = [0.0_f32; 32];
        if layer == 1 {
            for row in values.as_chunks_mut::<2>().0 {
                row[0] = 0.816_406_25;
            }
            values[18..20].copy_from_slice(&[0.730_468_75, -1.095_703_1]);
        }
        floats(&mut bytes, &values);
    }
    let terminal = bytes.len();
    bytes.push(5);
    number(&mut bytes, 1);
    number(&mut bytes, 0);
    Packet {
        bytes,
        frontend,
        encoder,
        prompt,
        decoder,
        cache,
        terminal,
    }
}

#[test]
fn rust_asr_replay_literal_complete_packet_and_fresh_reset() -> io::Result<()> {
    let mut recognizer = recognizer(0)?;
    for _ in 0..2 {
        let packet = packet();
        let replay = Replay::new(packet.bytes.as_slice(), &binding(), Tolerances::BITS)?;
        assert_eq!(
            replay.transcribe(&mut recognizer, &[0.0; 201], &mut || false)?,
            ""
        );
        assert_cleared(&recognizer);
    }
    Ok(())
}

#[test]
fn rust_asr_replay_rejects_changed_identity_input_and_invalid_tolerances() -> io::Result<()> {
    let packet = packet();
    let mut changed_tolerance = packet.bytes.clone();
    changed_tolerance[packet.frontend - 1] = 1;
    assert!(Replay::new(changed_tolerance.as_slice(), &binding(), Tolerances::BITS).is_err());
    let mut identity = binding();
    identity.candidate.executable[0] ^= 1;
    assert!(Replay::new(packet.bytes.as_slice(), &identity, Tolerances::BITS).is_err());
    let mut identity = binding();
    identity.model[0] ^= 1;
    assert!(Replay::new(packet.bytes.as_slice(), &identity, Tolerances::BITS).is_err());
    let mut identity = binding();
    identity.reference.revision = [0; 20];
    assert!(Replay::new(packet.bytes.as_slice(), &identity, Tolerances::BITS).is_err());
    for invalid in [f32::NAN, f32::INFINITY, -1.0] {
        let tolerances = Tolerances {
            logits: Tolerance::Finite {
                absolute: invalid,
                relative: 0.0,
            },
            ..Tolerances::BITS
        };
        assert!(Replay::new(packet.bytes.as_slice(), &binding(), tolerances).is_err());
    }
    let mut recognizer = recognizer(0)?;
    let replay = Replay::new(packet.bytes.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(
        replay
            .transcribe(&mut recognizer, &[0.1; 201], &mut || false)
            .is_err()
    );
    assert_cleared(&recognizer);
    Ok(())
}

#[test]
fn rust_asr_replay_rejects_every_boundary_truncation_cache_bits_and_extra_bytes() -> io::Result<()>
{
    let packet = packet();
    let mut recognizer = recognizer(0)?;
    for offset in [
        packet.frontend,
        packet.encoder,
        packet.prompt,
        packet.decoder,
        packet.cache,
        packet.terminal,
    ] {
        let replay = Replay::new(&packet.bytes[..offset], &binding(), Tolerances::BITS)?;
        assert!(
            replay
                .transcribe(&mut recognizer, &[0.0; 201], &mut || false)
                .is_err()
        );
        assert_cleared(&recognizer);
        let mut changed = packet.bytes.clone();
        changed[offset] ^= 1;
        let replay = Replay::new(changed.as_slice(), &binding(), Tolerances::BITS)?;
        assert!(
            replay
                .transcribe(&mut recognizer, &[0.0; 201], &mut || false)
                .is_err()
        );
        assert_cleared(&recognizer);
    }
    let mut changed = packet.bytes.clone();
    // Layer zero key four is negative zero; exact replay cannot erase its sign.
    changed[packet.cache + 8 + 8 + 4 * 4 + 3] ^= 0x80;
    let replay = Replay::new(changed.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(
        replay
            .transcribe(&mut recognizer, &[0.0; 201], &mut || false)
            .is_err()
    );
    assert_cleared(&recognizer);
    let mut extra = packet.bytes;
    extra.push(0);
    let replay = Replay::new(extra.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(
        replay
            .transcribe(&mut recognizer, &[0.0; 201], &mut || false)
            .is_err()
    );
    assert_cleared(&recognizer);
    Ok(())
}

#[test]
fn rust_asr_replay_tolerance_exception_bits_and_sticky_failed_schedule() -> io::Result<()> {
    let finite = Tolerance::Finite {
        absolute: 0.125,
        relative: 0.25,
    };
    assert!(finite.accepts(2.0, 2.625));
    assert!(!finite.accepts(2.0, 2.626));
    assert!(!finite.accepts(f32::INFINITY, f32::INFINITY));
    assert!(!finite.accepts(f32::NAN, f32::NAN));
    assert!(!Tolerance::Bits.accepts(0.0, -0.0));
    let nan = f32::from_bits(0x7fc0_0123);
    assert!(Tolerance::Bits.accepts(nan, nan));
    assert!(!Tolerance::Bits.accepts(nan, f32::from_bits(0x7fc0_0124)));
    let packet = packet();
    let mut replay = Replay::new(packet.bytes.as_slice(), &binding(), Tolerances::BITS)?;
    let prompt = PromptTokens::new([1, 2, 3, 4, 5, 6], [7, 8, 9], 10)?.automatic_prompt(1, 512)?;
    assert!(
        replay
            .observe(Observation::Prompt(&prompt), &mut || false)
            .is_err()
    );
    assert_eq!(replay.stage, Stage::Failed);
    let mel = crate::frontend::Frontend::from_checkpoint(None, None)?
        .compute(&[0.0; 201], &mut || false)?;
    assert!(
        replay
            .observe(Observation::Frontend(&mel), &mut || false)
            .is_err()
    );
    assert!(replay.finish("", &mut || false).is_err());
    Ok(())
}

#[test]
fn rust_asr_replay_cancelled_and_failed_engine_cannot_complete() -> io::Result<()> {
    let packet = packet();
    let mut cancelled = recognizer(0)?;
    let replay = Replay::new(packet.bytes.as_slice(), &binding(), Tolerances::BITS)?;
    assert_eq!(
        replay
            .transcribe(&mut cancelled, &[0.0; 201], &mut || true)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    assert_cleared(&cancelled);
    // Exact same first greedy decision, but token zero is not this model's EOS.
    // A complete-looking packet cannot turn the failing continuation into success.
    let mut failing = recognizer(1)?;
    let replay = Replay::new(packet.bytes.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(
        replay
            .transcribe(&mut failing, &[0.0; 201], &mut || false)
            .is_err()
    );
    assert_cleared(&failing);
    assert_eq!(
        failing
            .transcribe_cancellable(&[0.0; 201], &mut || true)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    Ok(())
}

struct InterruptedInput<'a> {
    bytes: &'a [u8],
    offset: usize,
    boundary: usize,
    cancel: &'a std::cell::Cell<bool>,
    panic: bool,
}

impl io::Read for InterruptedInput<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.offset >= self.boundary {
            assert!(!self.panic, "injected replay source unwind");
            self.cancel.set(true);
        }
        let length = output.len().min(16).min(self.bytes.len() - self.offset);
        output[..length].copy_from_slice(&self.bytes[self.offset..self.offset + length]);
        self.offset += length;
        Ok(length)
    }
}

#[test]
fn rust_asr_replay_stream_cancellation_and_unwind_reset_active_decoder() -> io::Result<()> {
    let packet = packet();
    for panic in [false, true] {
        let cancelled = std::cell::Cell::new(false);
        let input = InterruptedInput {
            bytes: &packet.bytes,
            offset: 0,
            boundary: packet.cache,
            cancel: &cancelled,
            panic,
        };
        let replay = Replay::new(input, &binding(), Tolerances::BITS)?;
        let mut recognizer = recognizer(0)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            replay.transcribe(&mut recognizer, &[0.0; 201], &mut || cancelled.get())
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result
                    .map_err(|_| io::Error::other("unexpected unwind"))?
                    .err()
                    .map(|error| error.kind()),
                Some(io::ErrorKind::Interrupted)
            );
        }
        assert_cleared(&recognizer);
        assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    }
    Ok(())
}

#[test]
fn rust_asr_replay_terminal_and_eof_cancellation_cannot_complete() -> io::Result<()> {
    let packet = packet();
    for boundary in [packet.terminal, packet.bytes.len()] {
        let cancelled = std::cell::Cell::new(false);
        let input = InterruptedInput {
            bytes: &packet.bytes,
            offset: 0,
            boundary,
            cancel: &cancelled,
            panic: false,
        };
        let replay = Replay::new(input, &binding(), Tolerances::BITS)?;
        let mut recognizer = recognizer(0)?;
        let result = replay.transcribe(&mut recognizer, &[0.0; 201], &mut || cancelled.get());
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(io::ErrorKind::Interrupted)
        );
        assert!(cancelled.get());
        assert_cleared(&recognizer);
        assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    }
    Ok(())
}
