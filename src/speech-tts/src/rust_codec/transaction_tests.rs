//! Literal two-layer codec-transformer answers and transactional failure controls.

use std::{io, panic::AssertUnwindSafe};

use super::{KvState, Transformer, TransformerAppend, TransformerLayer, TransformerState};
use crate::engine::{LinearProjection, WeightQuantization};

fn projection(weight: [f32; 4], bias: [f32; 2]) -> io::Result<LinearProjection> {
    LinearProjection::new(
        "codec-transaction.weight",
        2,
        2,
        WeightQuantization::F32(weight.to_vec()),
        Some(bias.to_vec()),
    )
}

fn graph() -> io::Result<Transformer> {
    let mut layers = Vec::new();
    for value in [[1.0, 2.0], [-2.0, 4.0]] {
        layers.push(TransformerLayer {
            norm: vec![1.0; 2],
            norm_bias: Some(vec![0.0; 2]),
            q: projection([0.0; 4], [0.0; 2])?,
            k: projection([0.0; 4], [0.0; 2])?,
            v: projection([0.0; 4], value)?,
            output: projection([1.0, 0.0, 0.0, 1.0], [0.0; 2])?,
            attn_scale: vec![1.0; 2],
            ffn_norm: vec![1.0; 2],
            ffn_bias: Some(vec![0.0; 2]),
            gate: None,
            up: projection([0.0; 4], [0.0; 2])?,
            down: projection([0.0; 4], [0.5, -0.25])?,
            ffn_scale: vec![1.0; 2],
        });
    }
    Ok(Transformer {
        threads: 1,
        hidden: 2,
        heads: 1,
        kv_heads: 1,
        head_dim: 2,
        window: Some(2),
        theta: 10_000.0,
        eps: 1.0,
        input: None,
        output: None,
        final_norm: None,
        layers,
    })
}

fn same_state(actual: &TransformerState, expected: &TransformerState) {
    assert_eq!(actual.position, expected.position);
    assert_eq!(actual.layers.len(), expected.layers.len());
    for (actual, expected) in actual.layers.iter().zip(&expected.layers) {
        for (actual, expected) in [
            (&actual.keys, &expected.keys),
            (&actual.values, &expected.values),
        ] {
            assert_eq!(
                actual
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn codec_transaction_complete_two_layer_graph_has_literal_outputs_and_window_state()
-> io::Result<()> {
    let graph = graph()?;
    let mut state = TransformerState::default();
    // Q/K are zero: attention is the exact mean of the retained constant V
    // rows. The two attention residuals sum to [-1,6], and the two FFN
    // residuals sum to [1,-0.5]. Thus every complete output is input+[0,5.5].
    for (position, (input, expected)) in [
        ([0.25, -0.5], [0.25, 5.0]),
        ([-1.0, 2.0], [-1.0, 7.5]),
        ([0.5, 1.0], [0.5, 6.5]),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(graph.step(&input, &mut state)?, expected);
        let rows = (position + 1).min(2);
        assert_eq!(state.position, position + 1);
        assert_eq!(state.layers.len(), 2);
        for layer in &state.layers {
            // Zero-valued RoPE lanes may carry either zero sign. The
            // separate rollback control requires their exact preservation.
            assert_eq!(layer.keys, vec![0.0; rows * 2]);
        }
        assert_eq!(state.layers[0].values, [1.0, 2.0].repeat(rows));
        assert_eq!(state.layers[1].values, [-2.0, 4.0].repeat(rows));
    }
    Ok(())
}

#[test]
fn codec_transaction_late_layer_and_final_projection_failures_restore_complete_state()
-> io::Result<()> {
    for failure in 0..4 {
        let mut graph = graph()?;
        let mut state = TransformerState::default();
        graph.step(&[0.25, -0.5], &mut state)?;
        graph.step(&[-1.0, 2.0], &mut state)?;
        // Distinct historical rows make accidental eviction detectable.
        state.layers[0].keys = vec![-0.0, 0.0, 0.0, -0.0];
        state.layers[0].values = vec![8.0, -3.0, 1.0, 2.0];
        let previous = state.clone();
        match failure {
            0 => graph.layers[1].down.bias = Some(Vec::new()),
            1 => graph.output = Some(projection([0.0; 4], [f32::NAN, 0.0])?),
            2 => graph.final_norm = Some(Vec::new()),
            _ => graph.layers[1].ffn_scale[0] = f32::INFINITY,
        }
        assert!(graph.step(&[0.5, 1.0], &mut state).is_err());
        same_state(&state, &previous);
        let graph = self::graph()?;
        assert_eq!(graph.step(&[0.5, 1.0], &mut state)?, [0.5, 6.5]);
        assert_eq!(state.position, 3);
        assert_eq!(state.layers[0].values, [1.0, 2.0, 1.0, 2.0]);
    }
    Ok(())
}

#[test]
fn codec_transaction_cold_failure_and_invalid_state_do_not_publish_a_position() -> io::Result<()> {
    let mut graph = graph()?;
    graph.layers[1].down.bias = Some(Vec::new());
    let mut state = TransformerState::default();
    assert!(graph.step(&[0.25, -0.5], &mut state).is_err());
    same_state(&state, &TransformerState::default());
    let graph = self::graph()?;
    assert!(graph.step(&[f32::NAN, 0.0], &mut state).is_err());
    same_state(&state, &TransformerState::default());
    graph.step(&[0.25, -0.5], &mut state)?;
    for invalid in 0..3 {
        let mut rejected = state.clone();
        match invalid {
            0 => {
                let _ = rejected.layers[1].values.pop();
            }
            1 => {
                rejected.position = usize::MAX;
            }
            _ => {
                rejected.layers.clear();
            }
        }
        let previous = rejected.clone();
        assert!(graph.step(&[0.25, -0.5], &mut rejected).is_err());
        same_state(&rejected, &previous);
    }
    Ok(())
}

#[test]
fn codec_transaction_unwind_restores_signed_bits_without_copying_cache_contents() -> io::Result<()>
{
    let graph = graph()?;
    let mut state = TransformerState::default();
    graph.step(&[0.25, -0.5], &mut state)?;
    state.layers[0].keys[0] = -0.0;
    let previous = state.clone();
    let interrupted = std::panic::catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
        let append = TransformerAppend::new(&mut state, 2, Some(2))?;
        append.state.layers[0].keys.extend([3.0, 4.0]);
        append.state.layers[0].values.extend([5.0, 6.0]);
        append.state.layers.push(KvState::default());
        std::panic::resume_unwind(Box::new("injected codec transformer fault"));
    }));
    assert!(interrupted.is_err());
    same_state(&state, &previous);
    assert_eq!(graph.step(&[0.5, 1.0], &mut state)?, [0.5, 6.5]);
    Ok(())
}
