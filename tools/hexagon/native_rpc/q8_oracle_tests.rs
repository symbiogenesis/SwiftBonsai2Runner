use super::*;

fn inputs(projection: Q8Matmul) -> (Vec<u8>, Vec<f32>) {
    let mut weights = vec![0; projection.weight_bytes()];
    for (index, block) in weights.as_chunks_mut::<34>().0.iter_mut().enumerate() {
        // Four independently specified signed scales: 1, 1/2, -1, 2.
        let scale = [0x3c00_u16, 0x3800, 0xbc00, 0x4000][index % 4];
        block[..2].copy_from_slice(&scale.to_le_bytes());
        block[2] = 0x80; // -128 must never become an unsigned dot operand.
        block[3] = 0x7f;
    }
    let mut activation = vec![0.0; projection.columns() * projection.batch()];
    for (index, vector) in activation
        .chunks_exact_mut(projection.columns())
        .enumerate()
    {
        for block in vector.as_chunks_mut::<32>().0 {
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            block[0] = 127.0 * sign;
            block[1] = -127.0 * sign;
        }
    }
    (weights, activation)
}

#[test]
fn q8_oracle_independent_signed_dots_scale_order_and_all_vectors() -> Result<(), Error> {
    let projection = Q8Matmul::new(128, 64, 4, 200_000)?;
    let (mut weights, activation) = inputs(projection);
    // Last row's last block additionally observes a signed tail code.
    let last = weights.len() - 1;
    weights[last] = 0xff;
    let mut activation = activation;
    for vector in activation.as_chunks_mut::<128>().0 {
        vector[127] = 3.0;
    }
    let oracle = Q8ExactOracle::new(projection, &weights, &activation)?;
    // dot=-128*127 + 127*(-127)=-32385, scaled by 1+.5-1+2=2.5.
    let mut expected = vec![0.0; 256];
    for (index, vector) in expected.as_chunks_mut::<64>().0.iter_mut().enumerate() {
        vector.fill(if index % 2 == 0 { -80_962.5 } else { 80_962.5 });
        vector[63] -= 6.0; // final code -1 * activation 3 * scale 2.
    }
    oracle.review(&expected)?;
    expected[255] += 1.0;
    assert!(oracle.review(&expected).is_err());
    expected[255] = f32::NAN;
    assert!(oracle.review(&expected).is_err());
    assert!(oracle.review(&expected[..255]).is_err());
    Ok(())
}

#[test]
fn q8_oracle_rejects_general_quantization_and_unsupported_half_scales() -> Result<(), Error> {
    let projection = Q8Matmul::new(128, 32, 2, 100_000)?;
    let (weights, activation) = inputs(projection);
    for invalid in [f32::NAN, f32::INFINITY, -128.0, 127.25, 0.5] {
        let mut changed = activation.clone();
        changed[255] = invalid;
        assert!(Q8ExactOracle::new(projection, &weights, &changed).is_err());
    }
    let mut no_anchor = activation.clone();
    no_anchor[224..256].fill(1.0);
    assert!(Q8ExactOracle::new(projection, &weights, &no_anchor).is_err());
    for invalid in [0x7c00_u16, 0x7e00, 0x0001, 0x1800, 0x6000, 0x3c01] {
        let mut changed = weights.clone();
        let end = changed.len();
        changed[end - 34..end - 32].copy_from_slice(&invalid.to_le_bytes());
        assert!(Q8ExactOracle::new(projection, &changed, &activation).is_err());
    }
    assert!(Q8ExactOracle::new(projection, &weights[..weights.len() - 1], &activation).is_err());
    assert!(Q8ExactOracle::new(projection, &weights, &activation[..255]).is_err());
    Ok(())
}

#[test]
fn q8_oracle_zero_sign_and_rounded_prefixes_cannot_grant_exact_acceptance() -> Result<(), Error> {
    let projection = Q8Matmul::new(128, 32, 1, 100_000)?;
    let (mut weights, mut activation) = inputs(projection);
    for block in weights.as_chunks_mut::<34>().0 {
        block[..2].copy_from_slice(&0x8000_u16.to_le_bytes());
    }
    let zero = Q8ExactOracle::new(projection, &weights, &activation)?;
    zero.review(&[0.0; 32])?;
    assert!(zero.review(&[-0.0; 32]).is_err());
    // A large exact first block is followed by one 2^-8 unit.
    // Even if later cancellation could recover an exact final sum, admission
    // must reject the first inexact prefix of the DSP's ordered additions.
    for block in weights.as_chunks_mut::<34>().0 {
        block.fill(0);
    }
    weights[..2].copy_from_slice(&0x5c00_u16.to_le_bytes()); // scale 256
    weights[2] = 0x80;
    weights[3] = 0x80;
    weights[4] = 0x80;
    weights[5] = 0x80;
    activation[..32].fill(0.0);
    activation[..4].fill(-127.0);
    // 4*128*127*256 = 16,646,144, exact but ulp=1.
    weights[34..36].copy_from_slice(&0x1c00_u16.to_le_bytes()); // scale 2^-8
    weights[36] = 1;
    activation[32] = 1.0;
    activation[33] = 127.0; // retain unit quantization without a weight here.
    assert!(Q8ExactOracle::new(projection, &weights, &activation).is_err());
    Ok(())
}

struct ReviewedTransport<'a> {
    weights: &'a [u8],
    activation: &'a [f32],
    value: f32,
    calls: u32,
}

impl Q8Submitter for ReviewedTransport<'_> {
    fn submit(
        &mut self,
        weights: &[u8],
        activation: &[f32],
        output: &mut [f32],
        oracle: &mut dyn FnMut(&[f32]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        assert!(std::ptr::eq(weights, self.weights));
        assert!(std::ptr::eq(activation, self.activation));
        self.calls += 1;
        let candidate = vec![self.value; output.len()];
        oracle(&candidate)?;
        output.copy_from_slice(&candidate);
        Ok(())
    }
}

#[test]
fn q8_oracle_submits_original_inputs_and_refuses_corrupt_result_before_publication()
-> Result<(), Error> {
    let projection = Q8Matmul::new(128, 32, 1, 100_000)?;
    let (weights, activation) = inputs(projection);
    let oracle = Q8ExactOracle::new(projection, &weights, &activation)?;
    let mut transport = ReviewedTransport {
        weights: &weights,
        activation: &activation,
        value: -80_962.5,
        calls: 0,
    };
    let mut output = [77.0; 32];
    oracle.submit(&mut transport, &mut output)?;
    assert_eq!(output, [-80_962.5; 32]);
    transport.value = 80_962.5;
    assert!(oracle.submit(&mut transport, &mut output).is_err());
    assert_eq!(output, [-80_962.5; 32]);
    assert_eq!(transport.calls, 2);
    Ok(())
}
