//! Owned Rust execution of the pinned 24 kHz Qwen tokenizer and speaker graphs.
//!
//! Tensor names, geometry, padding, nonlinearities and residual order follow
//! qwentts.cpp 5916bf7c79dbb4e66fc059cdc507a79a23a28bb4. The original
//! MIT notice is retained in LICENSE.qwen-core. No native code runs.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private checked graph loaders share the speech service's io::Result contract."
)]

use std::io;

use crate::{
    SAMPLE_RATE,
    codec::{
        CausalConv1d, CausalTransConv1d, CodecStreamState, ConvNeXtBlock, DacBlock, DacDecoder,
        DacResUnit, PadMode, RvqDecoder, RvqEncodeSide, RvqGroup, SnakeBeta, TOKENIZER_HOP_24K,
    },
    engine::{LinearProjection, apply_rope_neox_in_place, rms_norm},
    speaker_encoder::{
        AttentiveStatPool, AudioMelConfig, EcapaSpeakerEncoder, MelSpectrogramExtractor,
        SeRes2NetBlock, SqueezeExcitation, TdnnConv1d,
    },
    weights::Weights,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Unsupported speech codec graph or tensor geometry.",
    )
}

fn dimension(weights: &Weights, key: &str) -> io::Result<usize> {
    let value = usize::try_from(weights.u32(key)?).map_err(|_| invalid())?;
    if value == 0 || value > 8192 {
        return Err(invalid());
    }
    Ok(value)
}

fn count_float(value: usize) -> io::Result<f32> {
    Ok(f32::from(u16::try_from(value).map_err(|_| invalid())?))
}

fn projection(
    weights: &mut Weights,
    name: &str,
    input: usize,
    output: usize,
) -> io::Result<LinearProjection> {
    weights.projection(&format!("{name}.weight"), input, output)
}

fn biased_projection(
    weights: &mut Weights,
    name: &str,
    input: usize,
    output: usize,
) -> io::Result<LinearProjection> {
    let mut layer = projection(weights, name, input, output)?;
    layer.bias = Some(weights.tensor_f32(&format!("{name}.bias"), &[output])?);
    Ok(layer)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit convolution geometry is validated before loading weights."
)]
fn conv(
    weights: &mut Weights,
    name: &str,
    input: usize,
    output: usize,
    kernel: usize,
    dilation: usize,
    stride: usize,
    bias: bool,
    pad_mode: PadMode,
) -> io::Result<CausalConv1d> {
    Ok(CausalConv1d {
        out_c: output,
        in_c: input,
        kernel,
        dilation,
        stride,
        pad_mode,
        weight: weights.tensor_f32(&format!("{name}.weight"), &[kernel, input, output])?,
        bias: if bias {
            Some(weights.tensor_f32(&format!("{name}.bias"), &[output])?)
        } else {
            None
        },
    })
}

fn trans_conv(
    weights: &mut Weights,
    name: &str,
    input: usize,
    output: usize,
    kernel: usize,
    stride: usize,
) -> io::Result<CausalTransConv1d> {
    Ok(CausalTransConv1d {
        in_c: input,
        out_c: output,
        kernel,
        stride,
        // GGUF stores the PyTorch [IC, OC, K] array without a permutation.
        weight: weights.tensor_f32(&format!("{name}.weight"), &[kernel, output, input])?,
        bias: Some(weights.tensor_f32(&format!("{name}.bias"), &[output])?),
    })
}

fn snake(weights: &mut Weights, name: &str, channels: usize) -> io::Result<SnakeBeta> {
    SnakeBeta::from_log_params(
        &weights.tensor_f32(&format!("{name}.alpha"), &[channels])?,
        &weights.tensor_f32(&format!("{name}.beta"), &[channels])?,
    )
}

struct TransformerLayer {
    norm: Vec<f32>,
    norm_bias: Option<Vec<f32>>,
    q: LinearProjection,
    k: LinearProjection,
    v: LinearProjection,
    output: LinearProjection,
    attn_scale: Vec<f32>,
    ffn_norm: Vec<f32>,
    ffn_bias: Option<Vec<f32>>,
    gate: Option<LinearProjection>,
    up: LinearProjection,
    down: LinearProjection,
    ffn_scale: Vec<f32>,
}

#[derive(Clone, Default)]
struct KvState {
    keys: Vec<f32>,
    values: Vec<f32>,
}

#[derive(Clone, Default)]
struct TransformerState {
    position: usize,
    layers: Vec<KvState>,
}

struct Transformer {
    hidden: usize,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    window: Option<usize>,
    theta: f32,
    eps: f32,
    input: Option<LinearProjection>,
    output: Option<LinearProjection>,
    final_norm: Option<Vec<f32>>,
    layers: Vec<TransformerLayer>,
}

impl Transformer {
    #[expect(
        clippy::too_many_lines,
        reason = "Load the two explicitly different pinned transformer graphs in one auditable tensor-name mapping."
    )]
    fn load(weights: &mut Weights, decoder: bool) -> io::Result<Self> {
        let metadata = if decoder {
            "qwen3-tts-tokenizer.decoder"
        } else {
            "qwen3-tts-tokenizer.encoder"
        };
        let prefix = if decoder {
            "tok_dec.pre_tfm"
        } else {
            "tok_enc"
        };
        let hidden = dimension(weights, &format!("{metadata}.hidden_size"))?;
        let heads = dimension(weights, &format!("{metadata}.num_attention_heads"))?;
        let kv_heads = dimension(weights, &format!("{metadata}.num_key_value_heads"))?;
        let head_dim = dimension(weights, &format!("{metadata}.head_dim"))?;
        let intermediate = dimension(weights, &format!("{metadata}.intermediate_size"))?;
        let count = dimension(weights, &format!("{metadata}.num_hidden_layers"))?;
        let theta = weights.f32_metadata(&format!("{metadata}.rope_theta"))?;
        let eps = weights.f32_metadata(&format!(
            "{metadata}.{}",
            if decoder { "rms_norm_eps" } else { "norm_eps" }
        ))?;
        if count > 16
            || !heads.is_multiple_of(kv_heads)
            || !head_dim.is_multiple_of(2)
            || heads * head_dim > 8192
            || kv_heads * head_dim > 8192
            || !theta.is_finite()
            || theta <= 1.0
            || !eps.is_finite()
            || eps <= 0.0
        {
            return Err(invalid());
        }
        let mut layers = Vec::with_capacity(count);
        for index in 0..count {
            let name = format!("{prefix}.blk.{index}");
            layers.push(TransformerLayer {
                norm: weights.tensor_f32(&format!("{name}.attn_norm.weight"), &[hidden])?,
                norm_bias: if decoder {
                    None
                } else {
                    Some(weights.tensor_f32(&format!("{name}.attn_norm.bias"), &[hidden])?)
                },
                q: projection(weights, &format!("{name}.attn_q"), hidden, heads * head_dim)?,
                k: projection(
                    weights,
                    &format!("{name}.attn_k"),
                    hidden,
                    kv_heads * head_dim,
                )?,
                v: projection(
                    weights,
                    &format!("{name}.attn_v"),
                    hidden,
                    kv_heads * head_dim,
                )?,
                output: projection(
                    weights,
                    &format!("{name}.attn_output"),
                    heads * head_dim,
                    hidden,
                )?,
                attn_scale: weights.tensor_f32(&format!("{name}.attn_scale"), &[hidden])?,
                ffn_norm: weights.tensor_f32(&format!("{name}.ffn_norm.weight"), &[hidden])?,
                ffn_bias: if decoder {
                    None
                } else {
                    Some(weights.tensor_f32(&format!("{name}.ffn_norm.bias"), &[hidden])?)
                },
                gate: if decoder {
                    Some(projection(
                        weights,
                        &format!("{name}.ffn_gate"),
                        hidden,
                        intermediate,
                    )?)
                } else {
                    None
                },
                up: projection(weights, &format!("{name}.ffn_up"), hidden, intermediate)?,
                down: projection(weights, &format!("{name}.ffn_down"), intermediate, hidden)?,
                ffn_scale: weights.tensor_f32(&format!("{name}.ffn_scale"), &[hidden])?,
            });
        }
        let (input, output, final_norm, window) = if decoder {
            let latent = dimension(weights, &format!("{metadata}.latent_dim"))?;
            let window = dimension(weights, &format!("{metadata}.sliding_window"))?;
            (
                Some(biased_projection(
                    weights,
                    &format!("{prefix}.input_proj"),
                    latent,
                    hidden,
                )?),
                Some(biased_projection(
                    weights,
                    &format!("{prefix}.output_proj"),
                    hidden,
                    latent,
                )?),
                Some(weights.tensor_f32(&format!("{prefix}.norm.weight"), &[hidden])?),
                Some(window),
            )
        } else {
            (None, None, None, None)
        };
        Ok(Self {
            hidden,
            heads,
            kv_heads,
            head_dim,
            window,
            theta,
            eps,
            input,
            output,
            final_norm,
            layers,
        })
    }

    fn step(&self, input: &[f32], state: &mut TransformerState) -> io::Result<Vec<f32>> {
        if state.layers.is_empty() {
            state
                .layers
                .resize_with(self.layers.len(), KvState::default);
        }
        if state.layers.len() != self.layers.len() {
            return Err(invalid());
        }
        let mut hidden = if let Some(proj) = &self.input {
            proj.forward_vec(input, None)?
        } else {
            input.to_vec()
        };
        if hidden.len() != self.hidden {
            return Err(invalid());
        }
        for (layer, cache) in self.layers.iter().zip(&mut state.layers) {
            let normalized = normalize(&hidden, &layer.norm, layer.norm_bias.as_deref(), self.eps)?;
            let mut q = layer.q.forward_vec(&normalized, None)?;
            let mut k = layer.k.forward_vec(&normalized, None)?;
            let v = layer.v.forward_vec(&normalized, None)?;
            apply_rope_neox_in_place(
                &mut q,
                self.heads,
                self.head_dim,
                state.position,
                self.theta,
            )?;
            apply_rope_neox_in_place(
                &mut k,
                self.kv_heads,
                self.head_dim,
                state.position,
                self.theta,
            )?;
            let attended = causal_attention(
                &q,
                &k,
                &v,
                self.heads,
                self.kv_heads,
                self.head_dim,
                self.window,
                cache,
            )?;
            let projected = layer.output.forward_vec(&attended, None)?;
            for ((dst, value), scale) in hidden.iter_mut().zip(projected).zip(&layer.attn_scale) {
                *dst += value * scale;
            }
            let normalized = normalize(
                &hidden,
                &layer.ffn_norm,
                layer.ffn_bias.as_deref(),
                self.eps,
            )?;
            let mut up = layer.up.forward_vec(&normalized, None)?;
            if let Some(gate) = &layer.gate {
                let gate = gate.forward_vec(&normalized, None)?;
                for (value, gate) in up.iter_mut().zip(gate) {
                    *value *= gate / (1.0 + (-gate).exp());
                }
            } else {
                for value in &mut up {
                    *value = gelu_erf(*value);
                }
            }
            let projected = layer.down.forward_vec(&up, None)?;
            for ((dst, value), scale) in hidden.iter_mut().zip(projected).zip(&layer.ffn_scale) {
                *dst += value * scale;
            }
        }
        state.position = state.position.checked_add(1).ok_or_else(invalid)?;
        if let Some(norm) = &self.final_norm {
            hidden = rms_norm(&hidden, norm, self.eps)?;
        }
        if let Some(proj) = &self.output {
            hidden = proj.forward_vec(&hidden, None)?;
        }
        Ok(hidden)
    }
}

fn normalize(
    input: &[f32],
    weight: &[f32],
    bias: Option<&[f32]>,
    eps: f32,
) -> io::Result<Vec<f32>> {
    let Some(bias) = bias else {
        return rms_norm(input, weight, eps);
    };
    if input.is_empty() || input.len() != weight.len() || input.len() != bias.len() {
        return Err(invalid());
    }
    let count = count_float(input.len())?;
    let mean = input.iter().sum::<f32>() / count;
    let variance = input
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f32>()
        / count;
    let inv = 1.0 / (variance + eps).sqrt();
    Ok(input
        .iter()
        .zip(weight)
        .zip(bias)
        .map(|((&value, &scale), &offset)| ((value - mean) * inv).mul_add(scale, offset))
        .collect())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Keep query/key/value geometry and owned cache explicit."
)]
fn causal_attention(
    query: &[f32],
    key: &[f32],
    value: &[f32],
    heads: usize,
    kv_heads: usize,
    width: usize,
    window: Option<usize>,
    state: &mut KvState,
) -> io::Result<Vec<f32>> {
    let stride = kv_heads * width;
    if heads == 0
        || kv_heads == 0
        || width == 0
        || !heads.is_multiple_of(kv_heads)
        || query.len() != heads * width
        || key.len() != stride
        || value.len() != stride
        || state.keys.len() != state.values.len()
        || !state.keys.len().is_multiple_of(stride)
        || window == Some(0)
    {
        return Err(invalid());
    }
    state.keys.extend_from_slice(key);
    state.values.extend_from_slice(value);
    let mut frames = state.keys.len() / stride;
    if let Some(window) = window
        && frames > window
    {
        let discard = (frames - window) * stride;
        state.keys.drain(..discard);
        state.values.drain(..discard);
        frames = window;
    }
    let mut output = vec![0.0; query.len()];
    let mut scores = vec![0.0; frames];
    let scale = 1.0 / count_float(width)?.sqrt();
    for head in 0..heads {
        let kv_head = head / (heads / kv_heads);
        let q = &query[head * width..(head + 1) * width];
        for (frame, score) in scores.iter_mut().enumerate() {
            let offset = frame * stride + kv_head * width;
            *score = q
                .iter()
                .zip(&state.keys[offset..offset + width])
                .map(|(&a, &b)| a * b)
                .sum::<f32>()
                * scale;
        }
        let maximum = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum = scores
            .iter_mut()
            .map(|score| {
                *score = (*score - maximum).exp();
                *score
            })
            .sum::<f32>();
        if !sum.is_finite() || sum <= 0.0 {
            return Err(invalid());
        }
        for (frame, score) in scores.iter().enumerate() {
            let offset = frame * stride + kv_head * width;
            for (dst, &value) in output[head * width..(head + 1) * width]
                .iter_mut()
                .zip(&state.values[offset..offset + width])
            {
                *dst = (score / sum).mul_add(value, *dst);
            }
        }
    }
    Ok(output)
}

// Polynomial erf approximation. Mimi uses erf GELU; the DAC's ConvNeXt
// uses the separate tanh GELU. Actual-model numerical parity remains required.
fn gelu_erf(value: f32) -> f32 {
    let x = value * std::f32::consts::FRAC_1_SQRT_2;
    let t = 1.0 / 0.327_591_1_f32.mul_add(x.abs(), 1.0);
    let polynomial = 1.061_405_4_f32
        .mul_add(t, -1.453_152_1)
        .mul_add(t, 1.421_413_8)
        .mul_add(t, -0.284_496_72)
        .mul_add(t, 0.254_829_6);
    let erf = x.signum() * (polynomial * t).mul_add(-(-x * x).exp(), 1.0);
    0.5 * value * (1.0 + erf)
}

struct Upsample {
    conv: CausalTransConv1d,
    block: ConvNeXtBlock,
}

struct EncoderStage {
    first: CausalConv1d,
    second: CausalConv1d,
    down: CausalConv1d,
}

struct Encoder {
    first: CausalConv1d,
    stages: Vec<EncoderStage>,
    last: CausalConv1d,
    transformer: Transformer,
    down: CausalConv1d,
    semantic: RvqEncodeSide,
    acoustic: RvqEncodeSide,
}

impl Encoder {
    #[expect(
        clippy::too_many_lines,
        reason = "Bind every pinned SEANet stage, Mimi layer and RVQ side before returning an executable reference encoder."
    )]
    fn load(weights: &mut Weights) -> io::Result<Self> {
        let key = "qwen3-tts-tokenizer.encoder";
        let filters = dimension(weights, &format!("{key}.num_filters"))?;
        let hidden = dimension(weights, &format!("{key}.hidden_size"))?;
        let compress = dimension(weights, &format!("{key}.compress"))?;
        let kernel = dimension(weights, &format!("{key}.kernel_size"))?;
        let residual = dimension(weights, &format!("{key}.residual_kernel_size"))?;
        let last_kernel = dimension(weights, &format!("{key}.last_kernel_size"))?;
        let ratios = weights.u32_array(&format!("{key}.upsampling_ratios"))?;
        if filters != 64
            || hidden != 512
            || compress != 2
            || kernel != 7
            || residual != 3
            || last_kernel != 3
            || ratios != [8, 6, 5, 4]
        {
            return Err(invalid());
        }
        let first = conv(
            weights,
            "tok_enc.conv.0",
            1,
            filters,
            kernel,
            1,
            1,
            true,
            PadMode::Constant,
        )?;
        let mut channels = filters;
        let mut stages = Vec::with_capacity(4);
        for (index, ratio) in ratios.into_iter().rev().enumerate() {
            let ratio = usize::try_from(ratio).map_err(|_| invalid())?;
            let block = 1 + index * 3;
            stages.push(EncoderStage {
                first: conv(
                    weights,
                    &format!("tok_enc.res.{block}.blk.1"),
                    channels,
                    channels / compress,
                    residual,
                    1,
                    1,
                    true,
                    PadMode::Constant,
                )?,
                second: conv(
                    weights,
                    &format!("tok_enc.res.{block}.blk.3"),
                    channels / compress,
                    channels,
                    1,
                    1,
                    1,
                    true,
                    PadMode::Constant,
                )?,
                down: conv(
                    weights,
                    &format!("tok_enc.conv.{}", block + 2),
                    channels,
                    channels * 2,
                    ratio * 2,
                    1,
                    ratio,
                    true,
                    PadMode::Constant,
                )?,
            });
            channels *= 2;
        }
        let codebook_size = dimension(weights, &format!("{key}.codebook_size"))?;
        let codebook_dim = dimension(weights, &format!("{key}.vector_quantization_hidden_dim"))?;
        if codebook_size != 2048 || codebook_dim != 256 {
            return Err(invalid());
        }
        Ok(Self {
            first,
            stages,
            last: conv(
                weights,
                "tok_enc.conv.14",
                channels,
                hidden,
                last_kernel,
                1,
                1,
                true,
                PadMode::Constant,
            )?,
            transformer: Transformer::load(weights, false)?,
            down: conv(
                weights,
                "tok_enc.downsample",
                hidden,
                hidden,
                4,
                1,
                2,
                false,
                PadMode::Replicate,
            )?,
            semantic: load_encode_side(
                weights,
                "tok_enc.vq_semantic",
                1,
                codebook_size,
                codebook_dim,
                hidden,
            )?,
            acoustic: load_encode_side(
                weights,
                "tok_enc.vq_acoustic",
                15,
                codebook_size,
                codebook_dim,
                hidden,
            )?,
        })
    }

    fn encode(&self, samples: &[f32]) -> io::Result<Vec<[i32; 16]>> {
        if samples.len() < TOKENIZER_HOP_24K
            || samples.len() > 24_000 * 30
            || !samples.iter().all(|value| value.is_finite())
        {
            return Err(invalid());
        }
        // Preserve qt_extract_voice_ref's whole-frame reference alignment.
        // Generated references already contain an integral number of frames.
        let mut frames = samples.len() / TOKENIZER_HOP_24K * TOKENIZER_HOP_24K;
        let (mut hidden, _) = self.first.forward(&samples[..frames], frames)?;
        for stage in &self.stages {
            let residual = hidden.clone();
            elu(&mut hidden);
            (hidden, _) = stage.first.forward(&hidden, frames)?;
            elu(&mut hidden);
            (hidden, _) = stage.second.forward(&hidden, frames)?;
            for (value, residual) in hidden.iter_mut().zip(residual) {
                *value += residual;
            }
            elu(&mut hidden);
            (hidden, frames) = stage.down.forward(&hidden, frames)?;
        }
        elu(&mut hidden);
        (hidden, frames) = self.last.forward(&hidden, frames)?;
        let mut state = TransformerState::default();
        let mut transformed = Vec::with_capacity(hidden.len());
        for frame in hidden.chunks_exact(self.transformer.hidden) {
            transformed.extend(self.transformer.step(frame, &mut state)?);
        }
        (hidden, frames) = self.down.forward(&transformed, frames)?;
        let mut flat = Vec::with_capacity(frames * 16);
        self.semantic.encode(&hidden, frames, &mut flat)?;
        self.acoustic.encode(&hidden, frames, &mut flat)?;
        codebook_major_to_frames(&flat, frames)
    }
}

fn elu(values: &mut [f32]) {
    for value in values {
        if *value < 0.0 {
            *value = value.exp_m1();
        }
    }
}

fn codebook_major_to_frames(codes: &[i32], frames: usize) -> io::Result<Vec<[i32; 16]>> {
    if codes.len() != frames.checked_mul(16).ok_or_else(invalid)? {
        return Err(invalid());
    }
    let mut output = vec![[0; 16]; frames];
    for (frame, row) in output.iter_mut().enumerate() {
        for (book, slot) in row.iter_mut().enumerate() {
            *slot = codes[book * frames + frame];
        }
    }
    Ok(output)
}

fn load_encode_side(
    weights: &mut Weights,
    name: &str,
    count: usize,
    size: usize,
    width: usize,
    hidden: usize,
) -> io::Result<RvqEncodeSide> {
    let projection =
        weights.tensor_f32(&format!("{name}.input_proj.weight"), &[1, hidden, width])?;
    let codebooks = (0..count)
        .map(|index| weights.tensor_f32(&format!("{name}.{index}.codebook"), &[width, size]))
        .collect::<io::Result<Vec<_>>>()?;
    RvqEncodeSide::new(size, width, hidden, projection, codebooks)
}

fn load_rvq_group(
    weights: &mut Weights,
    name: &str,
    count: usize,
    size: usize,
    width: usize,
    hidden: usize,
) -> io::Result<RvqGroup> {
    Ok(RvqGroup {
        num_codebooks: count,
        codebook_size: size,
        codebook_dim: width,
        hidden_dim: hidden,
        codebooks: (0..count)
            .map(|index| weights.tensor_f32(&format!("{name}.{index}.codebook"), &[width, size]))
            .collect::<io::Result<_>>()?,
        out_proj_w: weights
            .tensor_f32(&format!("{name}.output_proj.weight"), &[1, width, hidden])?,
    })
}

#[derive(Clone)]
struct DecodeState {
    audio: CodecStreamState,
    transformer: TransformerState,
}

pub(crate) struct Codec {
    quantizer: RvqDecoder,
    pre: CausalConv1d,
    transformer: Transformer,
    upsample: Vec<Upsample>,
    dac: DacDecoder,
    encoder: Encoder,
    state: DecodeState,
    primed: Option<(Vec<[i32; 16]>, DecodeState)>,
}

impl Codec {
    #[expect(
        clippy::too_many_lines,
        reason = "Keep the complete pinned decoder tensor mapping together; each subgraph validates its own fixed geometry."
    )]
    pub(crate) fn load(weights: &mut Weights) -> io::Result<Self> {
        let key = "qwen3-tts-tokenizer.decoder";
        let size = dimension(weights, &format!("{key}.codebook_size"))?;
        let width = dimension(weights, &format!("{key}.codebook_dim_internal"))?;
        let hidden = dimension(weights, &format!("{key}.vector_quantization_hidden_dim"))?;
        let latent = dimension(weights, &format!("{key}.latent_dim"))?;
        if dimension(weights, &format!("{key}.num_quantizers"))? != 16
            || dimension(weights, &format!("{key}.num_semantic_quantizers"))? != 1
            || size != 2048
            || width != 256
            || hidden != 512
            || latent != 1024
        {
            return Err(invalid());
        }
        let quantizer = RvqDecoder {
            semantic: load_rvq_group(weights, "tok_dec.vq_first", 1, size, width, hidden)?,
            acoustic: load_rvq_group(weights, "tok_dec.vq_rest", 15, size, width, hidden)?,
        };
        let pre = conv(
            weights,
            "tok_dec.pre_conv",
            hidden,
            latent,
            3,
            1,
            1,
            true,
            PadMode::Constant,
        )?;
        let transformer = Transformer::load(weights, true)?;
        let mut upsample = Vec::with_capacity(2);
        for index in 0..2 {
            let name = format!("tok_dec.upsample.{index}");
            upsample.push(Upsample {
                conv: trans_conv(weights, &format!("{name}.conv"), latent, latent, 2, 2)?,
                block: ConvNeXtBlock {
                    channels: latent,
                    kernel: 7,
                    dw_weight: weights
                        .tensor_f32(&format!("{name}.dwconv.weight"), &[7, 1, latent])?,
                    dw_bias: weights.tensor_f32(&format!("{name}.dwconv.bias"), &[latent])?,
                    norm_w: weights.tensor_f32(&format!("{name}.norm.weight"), &[latent])?,
                    norm_b: weights.tensor_f32(&format!("{name}.norm.bias"), &[latent])?,
                    pwconv1_w: weights
                        .tensor_f32(&format!("{name}.pwconv1.weight"), &[latent, latent * 4])?,
                    pwconv1_b: weights
                        .tensor_f32(&format!("{name}.pwconv1.bias"), &[latent * 4])?,
                    pwconv2_w: weights
                        .tensor_f32(&format!("{name}.pwconv2.weight"), &[latent * 4, latent])?,
                    pwconv2_b: weights.tensor_f32(&format!("{name}.pwconv2.bias"), &[latent])?,
                    gamma: weights.tensor_f32(&format!("{name}.gamma"), &[latent])?,
                },
            });
        }
        let mut blocks = Vec::with_capacity(4);
        let mut channels = 1536;
        for (index, stride) in [8, 5, 4, 3].into_iter().enumerate() {
            let next = channels / 2;
            let name = format!("tok_dec.dec.{}", index + 1);
            let mut units = Vec::with_capacity(3);
            for (unit, dilation) in [1, 3, 9].into_iter().enumerate() {
                let name = format!("{name}.res.{unit}");
                units.push(DacResUnit {
                    act1: snake(weights, &format!("{name}.act1"), next)?,
                    conv1: conv(
                        weights,
                        &format!("{name}.conv1"),
                        next,
                        next,
                        7,
                        dilation,
                        1,
                        true,
                        PadMode::Constant,
                    )?,
                    act2: snake(weights, &format!("{name}.act2"), next)?,
                    conv2: conv(
                        weights,
                        &format!("{name}.conv2"),
                        next,
                        next,
                        1,
                        1,
                        1,
                        true,
                        PadMode::Constant,
                    )?,
                });
            }
            blocks.push(DacBlock {
                snake1: snake(weights, &format!("{name}.snake"), channels)?,
                trans_conv: trans_conv(
                    weights,
                    &format!("{name}.conv_t"),
                    channels,
                    next,
                    stride * 2,
                    stride,
                )?,
                res_units: units.try_into().map_err(|_| invalid())?,
            });
            channels = next;
        }
        let dac = DacDecoder {
            conv_pre: conv(
                weights,
                "tok_dec.dec.0.conv",
                latent,
                1536,
                7,
                1,
                1,
                true,
                PadMode::Constant,
            )?,
            blocks,
            snake_post: snake(weights, "tok_dec.dec.5.snake", channels)?,
            conv_post: conv(
                weights,
                "tok_dec.dec.6.conv",
                channels,
                1,
                7,
                1,
                1,
                true,
                PadMode::Constant,
            )?,
        };
        let encoder = Encoder::load(weights)?;
        let mut codec = Self {
            quantizer,
            pre,
            transformer,
            upsample,
            dac,
            encoder,
            state: DecodeState {
                audio: CodecStreamState::default(),
                transformer: TransformerState::default(),
            },
            primed: None,
        };
        codec.reset();
        Ok(codec)
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.dac.sample_rate()
    }

    pub(crate) fn reset(&mut self) {
        self.state = DecodeState {
            audio: CodecStreamState {
                pre_conv: vec![0.0; self.pre.stream_context_frames() * self.pre.in_c],
                upsample_dw: self
                    .upsample
                    .iter()
                    .map(|stage| vec![0.0; (stage.block.kernel - 1) * stage.block.channels])
                    .collect(),
                dac_pre: vec![
                    0.0;
                    self.dac.conv_pre.stream_context_frames() * self.dac.conv_pre.in_c
                ],
                dac_carry: self
                    .dac
                    .blocks
                    .iter()
                    .map(|block| {
                        vec![0.0; block.trans_conv.carry_frames() * block.trans_conv.out_c]
                    })
                    .collect(),
                dac_ru: self
                    .dac
                    .blocks
                    .iter()
                    .map(|block| {
                        std::array::from_fn(|index| {
                            vec![
                                0.0;
                                block.res_units[index].conv1.stream_context_frames()
                                    * block.res_units[index].conv1.in_c
                            ]
                        })
                    })
                    .collect(),
                dac_post: vec![
                    0.0;
                    self.dac.conv_post.stream_context_frames() * self.dac.conv_post.in_c
                ],
                ..CodecStreamState::default()
            },
            transformer: TransformerState::default(),
        };
    }

    pub(crate) fn decode(&mut self, codes: &[[i32; 16]]) -> io::Result<Vec<f32>> {
        self.reset();
        self.decode_stream(codes)
    }

    pub(crate) fn decode_stream(&mut self, codes: &[[i32; 16]]) -> io::Result<Vec<f32>> {
        if codes.is_empty()
            || codes.len() > 1500
            || codes
                .iter()
                .flatten()
                .any(|&code| !(0..2048).contains(&code))
        {
            return Err(invalid());
        }
        let mut output = Vec::with_capacity(codes.len() * TOKENIZER_HOP_24K);
        // Execute one frame at a time so emission chunk boundaries cannot alter
        // attention windows, convolution context, or floating-point grouping.
        for row in codes {
            let latent = self.quantizer.decode(row, 1)?;
            let latent = self
                .pre
                .forward_stream(&latent, 1, &mut self.state.audio.pre_conv)?;
            let mut latent = self
                .transformer
                .step(&latent, &mut self.state.transformer)?;
            let mut frames = 1;
            for (stage, context) in self.upsample.iter().zip(&mut self.state.audio.upsample_dw) {
                (latent, frames) = stage.conv.forward(&latent, frames)?;
                latent = stage.block.forward_stream(&latent, frames, context)?;
            }
            let samples = self
                .dac
                .forward_stream(&latent, frames, &mut self.state.audio)?;
            if samples.len() != TOKENIZER_HOP_24K || !samples.iter().all(|value| value.is_finite())
            {
                return Err(invalid());
            }
            output.extend(samples);
            self.state.audio.pos = self.state.audio.pos.checked_add(1).ok_or_else(invalid)?;
        }
        Ok(output)
    }

    pub(crate) fn prime_reference(&mut self, codes: &[[i32; 16]]) -> io::Result<()> {
        if let Some((reference, state)) = &self.primed
            && reference == codes
        {
            self.state = state.clone();
            return Ok(());
        }
        self.reset();
        for row in codes {
            drop(self.decode_stream(std::slice::from_ref(row))?);
        }
        self.primed = Some((codes.to_vec(), self.state.clone()));
        Ok(())
    }

    pub(crate) fn encode_reference(&self, samples: &[f32]) -> io::Result<Vec<[i32; 16]>> {
        self.encoder.encode(samples)
    }
}

pub(crate) struct Speaker(EcapaSpeakerEncoder);

fn tdnn(
    weights: &mut Weights,
    name: &str,
    input: usize,
    output: usize,
    kernel: usize,
    dilation: usize,
) -> io::Result<TdnnConv1d> {
    Ok(TdnnConv1d {
        out_c: output,
        in_c: input,
        kernel,
        dilation,
        weight: weights.tensor_f32(&format!("{name}.weight"), &[kernel, input, output])?,
        bias: weights.tensor_f32(&format!("{name}.bias"), &[output])?,
    })
}

impl Speaker {
    pub(crate) fn load(weights: &mut Weights) -> io::Result<Self> {
        if weights.u32("qwen3-tts.spk_enc.embedding_length")? != 2048
            || weights.u32("qwen3-tts.spk_enc.sample_rate")? != SAMPLE_RATE
        {
            return Err(invalid());
        }
        let mut blocks = Vec::with_capacity(3);
        for (index, dilation) in [2, 3, 4].into_iter().enumerate() {
            let name = format!("spk_enc.blk.{}", index + 1);
            blocks.push(SeRes2NetBlock {
                tdnn1: tdnn(weights, &format!("{name}.tdnn1"), 512, 512, 1, 1)?,
                branches: (0..7)
                    .map(|branch| {
                        tdnn(
                            weights,
                            &format!("{name}.res2net.{branch}"),
                            64,
                            64,
                            3,
                            dilation,
                        )
                    })
                    .collect::<io::Result<_>>()?,
                tdnn2: tdnn(weights, &format!("{name}.tdnn2"), 512, 512, 1, 1)?,
                se: SqueezeExcitation {
                    conv1: tdnn(weights, &format!("{name}.se.conv1"), 512, 128, 1, 1)?,
                    conv2: tdnn(weights, &format!("{name}.se.conv2"), 128, 512, 1, 1)?,
                },
                scale: 8,
            });
        }
        Ok(Self(EcapaSpeakerEncoder {
            mel: MelSpectrogramExtractor::new(AudioMelConfig::default())?,
            conv0: tdnn(weights, "spk_enc.conv0", 128, 512, 5, 1)?,
            blocks: blocks.try_into().map_err(|_| invalid())?,
            mfa: tdnn(weights, "spk_enc.mfa", 1536, 1536, 1, 1)?,
            asp: AttentiveStatPool {
                tdnn: tdnn(weights, "spk_enc.asp.tdnn", 4608, 128, 1, 1)?,
                conv: tdnn(weights, "spk_enc.asp.conv", 128, 1536, 1, 1)?,
            },
            fc: tdnn(weights, "spk_enc.fc", 3072, 2048, 1, 1)?,
        }))
    }

    pub(crate) fn extract(&self, samples: &[f32]) -> io::Result<Vec<f32>> {
        if samples.is_empty()
            || samples.len() > 24_000 * 30
            || !samples.iter().all(|value| value.is_finite())
        {
            return Err(invalid());
        }
        let vector = self.0.extract_xvector(samples)?;
        if vector.len() != 2048 || !vector.iter().all(|value| value.is_finite()) {
            return Err(invalid());
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::{KvState, causal_attention, codebook_major_to_frames, gelu_erf, normalize};

    #[test]
    fn codec_attention_retains_only_the_causal_window_with_independent_heads() -> std::io::Result<()>
    {
        let mut state = KvState::default();
        let query = [0.0; 4];
        for (value, expected) in [(2.0, 2.0), (4.0, 3.0), (8.0, 6.0)] {
            let result = causal_attention(
                &query,
                &[0.0; 4],
                &[value, value + 1.0, value * 10.0, value * 10.0 + 1.0],
                2,
                2,
                2,
                Some(2),
                &mut state,
            )?;
            assert_eq!(
                result,
                [
                    expected,
                    expected + 1.0,
                    expected * 10.0,
                    expected * 10.0 + 1.0
                ]
            );
        }
        assert_eq!(state.keys.len(), 8);
        assert_eq!(state.values.len(), 8);
        Ok(())
    }

    #[test]
    fn codec_reference_code_layout_preserves_all_sixteen_books_and_final_frame()
    -> std::io::Result<()> {
        let input: Vec<_> = (0_i32..16)
            .flat_map(|book| [book * 10, book * 10 + 1, book * 10 + 2])
            .collect();
        let output = codebook_major_to_frames(&input, 3)?;
        assert_eq!(output.len(), 3);
        for (frame, row) in output.iter().enumerate() {
            for (book, &code) in row.iter().enumerate() {
                assert_eq!(usize::try_from(code).ok(), Some(book * 10 + frame));
            }
        }
        assert!(codebook_major_to_frames(&input[..47], 3).is_err());
        Ok(())
    }

    #[test]
    fn mimi_uses_layer_norm_bias_and_erf_gelu_instead_of_decoder_rms_norm() -> std::io::Result<()> {
        let result = normalize(&[1.0, 3.0], &[2.0, 3.0], Some(&[0.5, -0.5]), 1e-5)?;
        assert!((result[0] + 1.499_99).abs() < 1e-5);
        assert!((result[1] - 2.499_985).abs() < 1e-5);
        for (input, expected) in [
            (0.0, 0.0),
            (1.0, 0.841_344_7),
            (-1.0, -0.158_655_26),
            (2.0, 1.954_499_7),
        ] {
            assert!((gelu_erf(input) - expected).abs() < 3e-7);
        }
        Ok(())
    }
}
