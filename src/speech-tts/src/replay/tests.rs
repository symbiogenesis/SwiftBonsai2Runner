//! Literal miniature observations exercise the wire contract, not native parity.

use std::io;

use super::{Binding, Provider, Replay, Tolerance, Tolerances, request_digest};
use crate::{
    engine::LayerKvCache,
    observation::{
        ARITHMETIC_IDENTITY, Observation, Phase, PredictorObservation, TalkerObservation,
    },
};

fn binding() -> Binding<'static> {
    Binding {
        reference: Provider {
            revision: [1; 20],
            compiler: "literal mathematical observations",
            target: "miniature-f32",
            executable: [2; 32],
            arithmetic: "literal-tts-miniature-v1",
        },
        candidate: Provider {
            revision: [3; 20],
            compiler: "test-compiler",
            target: "test-target",
            executable: [4; 32],
            arithmetic: ARITHMETIC_IDENTITY,
        },
        model_lock: [5; 32],
        models: [[6; 32], [7; 32], [8; 32]],
        request: [9; 32],
        workers: 2,
        retained_voice: false,
    }
}

fn number(bytes: &mut Vec<u8>, value: usize) -> io::Result<()> {
    bytes.extend(
        u64::try_from(value)
            .map_err(io::Error::other)?
            .to_le_bytes(),
    );
    Ok(())
}

fn text(bytes: &mut Vec<u8>, value: &str) -> io::Result<()> {
    number(bytes, value.len())?;
    bytes.extend(value.as_bytes());
    Ok(())
}

fn floats(bytes: &mut Vec<u8>, values: &[f32]) -> io::Result<()> {
    number(bytes, values.len())?;
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
    Ok(())
}

fn header() -> io::Result<Vec<u8>> {
    let binding = binding();
    let mut bytes = b"TTSREPL1".to_vec();
    for provider in [&binding.reference, &binding.candidate] {
        bytes.extend(provider.revision);
        text(&mut bytes, provider.compiler)?;
        text(&mut bytes, provider.target)?;
        bytes.extend(provider.executable);
        text(&mut bytes, provider.arithmetic)?;
    }
    bytes.extend(binding.model_lock);
    for model in binding.models {
        bytes.extend(model);
    }
    bytes.extend(binding.request);
    number(&mut bytes, binding.workers)?;
    bytes.push(0);
    bytes.extend([0; 6]);
    Ok(bytes)
}

fn cache_packet(bytes: &mut Vec<u8>, positions: usize) -> io::Result<()> {
    number(bytes, 2)?;
    for (index, keys, values) in [
        (0, [0.0_f32, -0.0], [1.0_f32, 2.0]),
        (1, [3.0, 4.0], [-5.0, 6.0]),
    ] {
        number(bytes, index)?;
        number(bytes, positions)?;
        floats(bytes, &keys.repeat(positions))?;
        floats(bytes, &values.repeat(positions))?;
    }
    Ok(())
}

fn caches(positions: usize) -> Vec<LayerKvCache> {
    vec![
        LayerKvCache {
            k: [0.0, -0.0].repeat(positions),
            v: [1.0, 2.0].repeat(positions),
            seq_len: positions,
        },
        LayerKvCache {
            k: [3.0, 4.0].repeat(positions),
            v: [-5.0, 6.0].repeat(positions),
            seq_len: positions,
        },
    ]
}

fn graph_packet(bytes: &mut Vec<u8>, phase: u8) -> io::Result<()> {
    bytes.extend([1, phase]);
    number(bytes, 2)?;
    floats(bytes, &[1.0, -0.0])?;
    floats(bytes, &[2.0, -3.0])?;
    number(bytes, 1)?;
    floats(bytes, &[-1.0, 0.0])?;
    number(bytes, 1)?;
    bytes.extend([2, phase]);
    number(bytes, 0)?;
    floats(bytes, &[3.0, -4.0])?;
    floats(bytes, &[0.25, -0.5])?;
    cache_packet(bytes, 2)?;
    bytes.extend([3, phase]);
    number(bytes, 0)?;
    bytes.extend(0_i32.to_le_bytes());
    bytes.push(0);
    bytes.extend([4, phase]);
    number(bytes, 0)?;
    cache_packet(bytes, 1)?;
    for codebook in 0..15 {
        bytes.extend([5, phase]);
        number(bytes, 0)?;
        number(bytes, codebook)?;
        floats(bytes, &[-2.0, 1.0])?;
        floats(bytes, &[9.0, -9.0])?;
        cache_packet(bytes, codebook + 2)?;
        bytes.extend([6, phase]);
        number(bytes, 0)?;
        number(bytes, codebook)?;
        bytes.extend(0_i32.to_le_bytes());
    }
    bytes.extend([7, phase]);
    number(bytes, 0)?;
    bytes.extend([0; 16 * 4]);
    if phase == 1 {
        bytes.extend([10, phase]);
        floats(bytes, &[0.5, -0.25])?;
    }
    bytes.extend([2, phase]);
    number(bytes, 1)?;
    floats(bytes, &[-3.0, 4.0])?;
    floats(bytes, &[-0.25, 0.5])?;
    cache_packet(bytes, 3)?;
    bytes.extend([3, phase]);
    number(bytes, 1)?;
    bytes.extend(1_i32.to_le_bytes());
    bytes.push(1);
    Ok(())
}

fn packet() -> io::Result<Vec<u8>> {
    let mut bytes = header()?;
    graph_packet(&mut bytes, 0)?;
    bytes.extend([10, 0]);
    floats(&mut bytes, &[0.125, -0.125, 0.25])?;
    bytes.push(8);
    floats(&mut bytes, &[2.0, -3.0])?;
    bytes.push(9);
    number(&mut bytes, 1)?;
    bytes.extend([0; 16 * 4]);
    graph_packet(&mut bytes, 1)?;
    bytes.push(11);
    number(&mut bytes, 2)?;
    bytes.extend(24_000_u32.to_le_bytes());
    bytes.push(255);
    number(&mut bytes, 79)?;
    Ok(bytes)
}

fn graph<R: io::Read>(
    replay: &mut Replay<R>,
    phase: Phase,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<()> {
    let mut observe = |observation: TalkerObservation<'_>| {
        replay.observe(Observation::Talker { phase, observation }, cancel)
    };
    observe(TalkerObservation::Prompt {
        rows: &[vec![1.0, -0.0], vec![2.0, -3.0]],
        trailing: &[vec![-1.0, 0.0]],
        cacheable_rows: 1,
    })?;
    observe(TalkerObservation::Logits {
        frame: 0,
        hidden: &[3.0, -4.0],
        logits: &[0.25, -0.5],
        cache: &caches(2),
    })?;
    observe(TalkerObservation::Decision {
        frame: 0,
        token: 0,
        eos: false,
    })?;
    observe(TalkerObservation::Predictor {
        frame: 0,
        observation: PredictorObservation::Prefill { cache: &caches(1) },
    })?;
    for codebook in 0..15 {
        observe(TalkerObservation::Predictor {
            frame: 0,
            observation: PredictorObservation::Logits {
                codebook,
                hidden: &[-2.0, 1.0],
                logits: &[9.0, -9.0],
                cache: &caches(codebook + 2),
            },
        })?;
        observe(TalkerObservation::Predictor {
            frame: 0,
            observation: PredictorObservation::Decision { codebook, token: 0 },
        })?;
    }
    observe(TalkerObservation::Frame {
        frame: 0,
        codes: &[0; 16],
    })?;
    if phase == Phase::Base {
        replay.observe(
            Observation::Pcm {
                phase,
                samples: &[0.5, -0.25],
            },
            cancel,
        )?;
    }
    replay.observe(
        Observation::Talker {
            phase,
            observation: TalkerObservation::Logits {
                frame: 1,
                hidden: &[-3.0, 4.0],
                logits: &[-0.25, 0.5],
                cache: &caches(3),
            },
        },
        cancel,
    )?;
    replay.observe(
        Observation::Talker {
            phase,
            observation: TalkerObservation::Decision {
                frame: 1,
                token: 1,
                eos: true,
            },
        },
        cancel,
    )
}

fn drive<R: io::Read>(replay: &mut Replay<R>, cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
    graph(replay, Phase::Design, cancel)?;
    replay.observe(
        Observation::Pcm {
            phase: Phase::Design,
            samples: &[0.125, -0.125, 0.25],
        },
        cancel,
    )?;
    replay.observe(Observation::Speaker(&[2.0, -3.0]), cancel)?;
    replay.observe(Observation::ReferenceCodes(&[[0; 16]]), cancel)?;
    graph(replay, Phase::Base, cancel)?;
    replay.observe(
        Observation::Complete {
            samples: 2,
            sample_rate: 24_000,
        },
        cancel,
    )
}

#[test]
fn tts_replay_compares_every_literal_graph_cache_code_pcm_and_terminal() -> io::Result<()> {
    let bytes = packet()?;
    let mut replay = Replay::new(bytes.as_slice(), &binding(), Tolerances::BITS)?;
    drive(&mut replay, &mut || false)?;
    assert!(replay.complete && !replay.failed);
    assert_eq!(replay.events, 79);
    assert!(
        replay
            .observe(Observation::Speaker(&[]), &mut || false)
            .is_err()
    );
    Ok(())
}

#[test]
fn tts_replay_rejects_changed_identities_workers_voice_and_tolerances() -> io::Result<()> {
    let bytes = header()?;
    for changed in 0..12 {
        let mut binding = binding();
        match changed {
            0 => binding.reference.revision[0] ^= 1,
            1 => binding.reference.compiler = "changed",
            2 => binding.candidate.target = "changed",
            3 => binding.candidate.executable[0] ^= 1,
            4 => binding.candidate.arithmetic = "changed",
            5 => binding.model_lock[0] ^= 1,
            6..=8 => binding.models[changed - 6][0] ^= 1,
            9 => binding.request[0] ^= 1,
            10 => binding.workers = 4,
            _ => binding.retained_voice = true,
        }
        assert!(Replay::new(bytes.as_slice(), &binding, Tolerances::BITS).is_err());
    }
    let mut rules = Tolerances::BITS;
    rules.cache = Tolerance::Finite {
        absolute: -1.0,
        relative: 0.0,
    };
    assert!(Replay::new(bytes.as_slice(), &binding(), rules).is_err());
    assert_ne!(request_digest("ab", "c")?, request_digest("a", "bc")?);
    assert_ne!(request_digest("é", "v")?, request_digest("e", "v")?);
    Ok(())
}

#[test]
fn tts_replay_rejects_truncation_mutation_reordering_and_trailing_data() -> io::Result<()> {
    let original = packet()?;
    let header_len = header()?.len();
    // Truncate at each byte: none of the original replay inputs can disappear.
    for end in header_len..original.len() {
        let mut replay = Replay::new(&original[..end], &binding(), Tolerances::BITS)?;
        assert!(drive(&mut replay, &mut || false).is_err(), "length {end}");
        assert!(replay.failed && !replay.complete);
        assert!(
            replay
                .observe(Observation::Speaker(&[]), &mut || false)
                .is_err()
        );
    }
    // Mutations cover every original record byte, including cache bits and IDs.
    for index in (header_len..original.len()).step_by(37) {
        let mut changed = original.clone();
        changed[index] ^= 1;
        let mut replay = Replay::new(changed.as_slice(), &binding(), Tolerances::BITS)?;
        assert!(drive(&mut replay, &mut || false).is_err(), "byte {index}");
    }
    let mut trailing = original.clone();
    trailing.push(0);
    let mut replay = Replay::new(trailing.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(drive(&mut replay, &mut || false).is_err());
    let mut replay = Replay::new(original.as_slice(), &binding(), Tolerances::BITS)?;
    assert!(
        replay
            .observe(Observation::Speaker(&[2.0, -3.0]), &mut || false)
            .is_err()
    );
    Ok(())
}

#[test]
fn tts_replay_float_rules_preserve_exceptional_bits_and_finite_bounds() {
    assert!(!Tolerance::Bits.accepts(0.0, -0.0));
    assert!(Tolerance::Bits.accepts(f32::from_bits(0x7fc0_0001), f32::from_bits(0x7fc0_0001)));
    assert!(!Tolerance::Bits.accepts(f32::from_bits(0x7fc0_0001), f32::from_bits(0x7fc0_0002)));
    let finite = Tolerance::Finite {
        absolute: 0.25,
        relative: 0.125,
    };
    assert!(finite.accepts(2.0, 2.5));
    assert!(!finite.accepts(2.0, 2.500_001));
    assert!(!finite.accepts(f32::INFINITY, f32::INFINITY));
    assert!(!finite.accepts(f32::NAN, 0.0));
    assert!(
        !Tolerance::Finite {
            absolute: f32::NAN,
            relative: 0.0
        }
        .valid()
    );
}

#[test]
fn tts_replay_cancellation_and_unwind_are_sticky() -> io::Result<()> {
    let bytes = packet()?;
    let mut checks = 0;
    let mut complete = Replay::new(bytes.as_slice(), &binding(), Tolerances::BITS)?;
    drive(&mut complete, &mut || {
        checks += 1;
        false
    })?;
    for boundary in 1..=checks {
        let mut replay = Replay::new(bytes.as_slice(), &binding(), Tolerances::BITS)?;
        let mut seen = 0;
        let error = drive(&mut replay, &mut || {
            seen += 1;
            seen == boundary
        });
        assert_eq!(
            error.err().map(|e| e.kind()),
            Some(io::ErrorKind::Interrupted)
        );
        assert!(replay.failed && !replay.complete);
    }
    let mut replay = Replay::new(bytes.as_slice(), &binding(), Tolerances::BITS)?;
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drive(&mut replay, &mut || {
            std::panic::resume_unwind(Box::new("cancel owner unwound"))
        })
    }));
    assert!(unwind.is_err());
    assert!(replay.failed && !replay.complete);
    assert!(
        replay
            .observe(Observation::Speaker(&[]), &mut || false)
            .is_err()
    );
    Ok(())
}
