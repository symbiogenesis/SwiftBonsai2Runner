//! CPU Talker/CodePredictor serving for the pinned ordinary Qwen GGUF models.
//!
//! Prompt and frame ordering follows the reviewed qwentts.cpp revision recorded
//! by `ENGINE_REVISION`. This module opens no files, devices or network connections.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep the serving types private to this crate."
)]

use std::io;

use crate::{
    engine::{
        CodePredictorModel, DecoupledSamplingParams, LayerKvCache, LinearProjection,
        QwenDecoderLayer, TOTAL_CODE_GROUPS, rms_norm,
    },
    sampling::{self, SampleTopKPParams},
    tokenizer::Tokenizer,
    weights::Weights,
};

pub(crate) const REFERENCE_TEXT: &str = "The bright morning light warms the quiet courtyard path near the old glass arch in clear weather.";
const VOICE_SEED: i64 = 42;
const MAX_NEW_TOKENS: usize = 1500;
const MAX_CONTEXT: usize = 4096;
const MAX_REFERENCE_FRAMES: usize = 375;

pub(crate) struct Reference {
    pub(crate) speaker: Vec<f32>,
    pub(crate) codes: Vec<[i32; TOTAL_CODE_GROUPS]>,
}

struct Geometry {
    hidden: usize,
    intermediate: usize,
    layers: usize,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    vocab: usize,
    rope_theta: f32,
    norm_eps: f32,
}

impl Geometry {
    fn load(weights: &Weights, prefix: &str) -> io::Result<Self> {
        let n = |suffix: &str, max: usize| -> io::Result<usize> {
            let value = usize::try_from(weights.u32(&format!("{prefix}.{suffix}"))?)
                .map_err(|_| invalid("Speech geometry is out of range."))?;
            if value == 0 || value > max {
                return Err(invalid("Speech geometry is out of range."));
            }
            Ok(value)
        };
        let result = Self {
            hidden: n("embedding_length", 4096)?,
            intermediate: n("feed_forward_length", 32_768)?,
            layers: n("block_count", 64)?,
            heads: n("attention.head_count", 64)?,
            kv_heads: n("attention.head_count_kv", 64)?,
            head_dim: n("attention.key_length", 256)?,
            vocab: n("vocab_size", 8192)?,
            rope_theta: weights.f32_metadata(&format!("{prefix}.rope.freq_base"))?,
            norm_eps: weights
                .f32_metadata(&format!("{prefix}.attention.layer_norm_rms_epsilon"))?,
        };
        if !result.heads.is_multiple_of(result.kv_heads)
            || !result.head_dim.is_multiple_of(2)
            || !result.rope_theta.is_finite()
            || result.rope_theta <= 0.0
            || !result.norm_eps.is_finite()
            || result.norm_eps <= 0.0
        {
            return Err(invalid("Unsupported speech attention geometry."));
        }
        Ok(result)
    }

    fn load_layers(
        &self,
        weights: &mut Weights,
        prefix: &str,
    ) -> io::Result<Vec<QwenDecoderLayer>> {
        let mut result = Vec::with_capacity(self.layers);
        let q = self.heads * self.head_dim;
        let kv = self.kv_heads * self.head_dim;
        for index in 0..self.layers {
            let name = format!("{prefix}.blk.{index}");
            result.push(QwenDecoderLayer {
                attn_norm_w: weights
                    .tensor_f32(&format!("{name}.attn_norm.weight"), &[self.hidden])?,
                q_proj: weights.projection(&format!("{name}.attn_q.weight"), self.hidden, q)?,
                k_proj: weights.projection(&format!("{name}.attn_k.weight"), self.hidden, kv)?,
                v_proj: weights.projection(&format!("{name}.attn_v.weight"), self.hidden, kv)?,
                o_proj: weights.projection(
                    &format!("{name}.attn_output.weight"),
                    q,
                    self.hidden,
                )?,
                q_norm_w: weights
                    .tensor_f32(&format!("{name}.attn_q_norm.weight"), &[self.head_dim])?,
                k_norm_w: weights
                    .tensor_f32(&format!("{name}.attn_k_norm.weight"), &[self.head_dim])?,
                ffn_norm_w: weights
                    .tensor_f32(&format!("{name}.ffn_norm.weight"), &[self.hidden])?,
                gate_proj: weights.projection(
                    &format!("{name}.ffn_gate.weight"),
                    self.hidden,
                    self.intermediate,
                )?,
                up_proj: weights.projection(
                    &format!("{name}.ffn_up.weight"),
                    self.hidden,
                    self.intermediate,
                )?,
                down_proj: weights.projection(
                    &format!("{name}.ffn_down.weight"),
                    self.intermediate,
                    self.hidden,
                )?,
            });
        }
        Ok(result)
    }
}

struct Specials {
    pad: i32,
    bos: i32,
    eos: i32,
    nothink: i32,
    think_bos: i32,
    think_eos: i32,
    text_pad: i32,
    text_bos: i32,
    text_eos: i32,
}

impl Specials {
    fn load(weights: &Weights, vocab: usize, text_vocab: usize) -> io::Result<Self> {
        let id = |name: &str, limit: usize| -> io::Result<i32> {
            let value = weights.u32(&format!("qwen3-tts.{name}"))?;
            if usize::try_from(value).map_or(true, |n| n >= limit) {
                return Err(invalid("Speech special token is out of range."));
            }
            i32::try_from(value).map_err(|_| invalid("Speech special token is out of range."))
        };
        Ok(Self {
            pad: id("codec.pad_id", vocab)?,
            bos: id("codec.bos_id", vocab)?,
            eos: id("codec.eos_id", vocab)?,
            nothink: id("codec.nothink_id", vocab)?,
            think_bos: id("codec.think_bos_id", vocab)?,
            think_eos: id("codec.think_eos_id", vocab)?,
            text_pad: id("text.tts_pad_id", text_vocab)?,
            text_bos: id("text.tts_bos_id", text_vocab)?,
            text_eos: id("text.tts_eos_id", text_vocab)?,
        })
    }
}

pub(crate) struct Talker {
    geometry: Geometry,
    layers: Vec<QwenDecoderLayer>,
    norm: Vec<f32>,
    head: LinearProjection,
    text_embedding: LinearProjection,
    text_fc1: LinearProjection,
    text_fc2: LinearProjection,
    codec_embedding: Vec<f32>,
    predictor: CodePredictorModel,
    tokenizer: Tokenizer,
    specials: Specials,
    tts_pad: Vec<f32>,
    tts_bos: Vec<f32>,
    tts_eos: Vec<f32>,
    threads: usize,
    base: bool,
}

struct Prompt {
    rows: Vec<Vec<f32>>,
    trailing: Vec<Vec<f32>>,
}

impl Talker {
    #[expect(
        clippy::too_many_lines,
        reason = "Load and validate the complete pinned Talker/predictor tensor inventory atomically."
    )]
    pub(crate) fn load(
        weights: &mut Weights,
        tokenizer: Tokenizer,
        threads: usize,
    ) -> io::Result<Self> {
        if !(1..=256).contains(&threads) {
            return Err(invalid("Invalid speech CPU worker count."));
        }
        let base = match weights.string("qwen3-tts.model_type")? {
            "base" => true,
            "voice_design" => false,
            _ => return Err(invalid("Unsupported speech model type.")),
        };
        if weights.u32("qwen3-tts.num_code_groups")? != 16 {
            return Err(invalid("Speech requires sixteen codebooks."));
        }
        let geometry = Geometry::load(weights, "qwen3-tts.talker")?;
        let predictor_geometry = Geometry::load(weights, "qwen3-tts.code_pred")?;
        let text_shape = weights.tensor_shape("talker.text_embd.weight")?;
        if text_shape.len() != 2 || text_shape[0] == 0 || text_shape[0] > 4096 {
            return Err(invalid("Unsupported speech text embedding geometry."));
        }
        let text_dim = text_shape[0];
        let text_vocab = text_shape[1];
        if text_vocab
            != usize::try_from(weights.u32("qwen3-tts.talker.text_vocab_size")?)
                .map_err(|_| invalid("Unsupported speech text vocabulary."))?
            || text_vocab > 200_000
            || geometry.vocab <= 1024
            || predictor_geometry.vocab != geometry.vocab - 1024
        {
            return Err(invalid("Unsupported speech vocabulary geometry."));
        }
        let specials = Specials::load(weights, geometry.vocab, text_vocab)?;
        let text_embedding = weights.projection("talker.text_embd.weight", text_dim, text_vocab)?;
        let mut text_fc1 = weights.projection("talker.text_proj.fc1.weight", text_dim, text_dim)?;
        text_fc1.bias = Some(weights.tensor_f32("talker.text_proj.fc1.bias", &[text_dim])?);
        let mut text_fc2 =
            weights.projection("talker.text_proj.fc2.weight", text_dim, geometry.hidden)?;
        text_fc2.bias = Some(weights.tensor_f32("talker.text_proj.fc2.bias", &[geometry.hidden])?);
        let codec_embedding = weights.tensor_f32(
            "talker.codec_embd.weight",
            &[geometry.hidden, geometry.vocab],
        )?;
        let head =
            weights.projection("talker.codec_head.weight", geometry.hidden, geometry.vocab)?;
        let norm = weights.tensor_f32("talker.output_norm.weight", &[geometry.hidden])?;
        let layers = geometry.load_layers(weights, "talker")?;
        let mtp_proj = if predictor_geometry.hidden == geometry.hidden {
            None
        } else {
            let mut projection = weights.projection(
                "code_pred.mtp_proj.weight",
                geometry.hidden,
                predictor_geometry.hidden,
            )?;
            projection.bias =
                Some(weights.tensor_f32("code_pred.mtp_proj.bias", &[predictor_geometry.hidden])?);
            Some(projection)
        };
        let mut codec_embeddings = Vec::with_capacity(15);
        let mut lm_heads = Vec::with_capacity(15);
        for codebook in 0..15 {
            codec_embeddings.push(weights.tensor_f32(
                &format!("code_pred.codec_embd.{codebook}.weight"),
                &[geometry.hidden, predictor_geometry.vocab],
            )?);
            lm_heads.push(weights.projection(
                &format!("code_pred.lm_head.{codebook}.weight"),
                predictor_geometry.hidden,
                predictor_geometry.vocab,
            )?);
        }
        let predictor = CodePredictorModel {
            hidden_size: predictor_geometry.hidden,
            num_q_heads: predictor_geometry.heads,
            num_kv_heads: predictor_geometry.kv_heads,
            head_dim: predictor_geometry.head_dim,
            vocab_size: predictor_geometry.vocab,
            num_acoustic_codebooks: 15,
            rope_theta: predictor_geometry.rope_theta,
            rms_norm_eps: predictor_geometry.norm_eps,
            mtp_proj,
            layers: predictor_geometry.load_layers(weights, "code_pred")?,
            norm_w: weights
                .tensor_f32("code_pred.output_norm.weight", &[predictor_geometry.hidden])?,
            codec_embeddings,
            lm_heads,
        };
        let mut result = Self {
            geometry,
            layers,
            norm,
            head,
            text_embedding,
            text_fc1,
            text_fc2,
            codec_embedding,
            predictor,
            tokenizer,
            specials,
            tts_pad: Vec::new(),
            tts_bos: Vec::new(),
            tts_eos: Vec::new(),
            threads,
            base,
        };
        result.tts_pad = result.project_text(result.specials.text_pad)?;
        result.tts_bos = result.project_text(result.specials.text_bos)?;
        result.tts_eos = result.project_text(result.specials.text_eos)?;
        Ok(result)
    }

    fn project_text(&self, id: i32) -> io::Result<Vec<f32>> {
        let id = usize::try_from(id).map_err(|_| invalid("Negative speech text token."))?;
        let row = self.text_embedding.row(id)?;
        let mut h = self
            .text_fc1
            .forward_vec_parallel(&row, None, self.threads)?;
        for value in &mut h {
            *value /= 1.0 + (-*value).exp();
        }
        self.text_fc2.forward_vec_parallel(&h, None, self.threads)
    }

    fn codec_row(&self, id: i32) -> io::Result<&[f32]> {
        embedding_row(&self.codec_embedding, self.geometry.hidden, id)
    }

    fn sum_codes(&self, frame: &[i32; TOTAL_CODE_GROUPS]) -> io::Result<Vec<f32>> {
        let mut row = self.codec_row(frame[0])?.to_vec();
        for (table, &code) in self.predictor.codec_embeddings.iter().zip(&frame[1..]) {
            add(&mut row, embedding_row(table, self.geometry.hidden, code)?)?;
        }
        Ok(row)
    }

    fn framed_ids(&self, text: &str) -> io::Result<Vec<i32>> {
        let ids = self.tokenizer.encode(
            format!("<|im_start|>assistant\n{text}<|im_end|>\n<|im_start|>assistant\n").as_bytes(),
            false,
        )?;
        if ids.len() <= 8 || ids.len() > MAX_CONTEXT {
            return Err(invalid("Speech prompt framing is invalid or too long."));
        }
        Ok(ids)
    }

    fn prompt(
        &self,
        text: &str,
        instruct: Option<&str>,
        reference: Option<&Reference>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<Prompt> {
        if self.base != reference.is_some() || (self.base && instruct.is_some()) {
            return Err(invalid(
                "Speech conditioning does not match the selected model.",
            ));
        }
        let ids = self.framed_ids(text)?;
        let mut rows = Vec::new();
        if let Some(instruct) = instruct.filter(|value| !value.is_empty()) {
            let head = self.tokenizer.encode(
                format!("<|im_start|>user\n{instruct}<|im_end|>\n").as_bytes(),
                false,
            )?;
            if head.len() > MAX_CONTEXT {
                return Err(invalid("Speech instruction is too long."));
            }
            for id in head {
                cancel()?;
                rows.push(self.project_text(id)?);
            }
        }
        for &id in &ids[..3] {
            cancel()?;
            rows.push(self.project_text(id)?);
        }
        for id in [
            self.specials.nothink,
            self.specials.think_bos,
            self.specials.think_eos,
        ] {
            let mut row = self.tts_pad.clone();
            add(&mut row, self.codec_row(id)?)?;
            rows.push(row);
        }
        if let Some(reference) = reference {
            if reference.speaker.len() != self.geometry.hidden
                || reference.speaker.iter().any(|value| !value.is_finite())
                || reference.codes.is_empty()
                || reference.codes.len() > MAX_REFERENCE_FRAMES
            {
                return Err(invalid("Invalid speech voice reference."));
            }
            let mut row = self.tts_pad.clone();
            add(&mut row, &reference.speaker)?;
            rows.push(row);
        }
        let mut prefix = self.tts_bos.clone();
        add(&mut prefix, self.codec_row(self.specials.pad)?)?;
        rows.push(prefix);

        let body = &ids[3..ids.len() - 5];
        let trailing = if let Some(reference) = reference {
            let reference_ids = self.framed_ids(REFERENCE_TEXT)?;
            let mut text_rows = Vec::new();
            for &id in reference_ids[3..reference_ids.len() - 5].iter().chain(body) {
                cancel()?;
                text_rows.push(self.project_text(id)?);
            }
            text_rows.push(self.tts_eos.clone());
            let mut codec_rows = vec![self.codec_row(self.specials.bos)?.to_vec()];
            for frame in &reference.codes {
                cancel()?;
                codec_rows.push(self.sum_codes(frame)?);
            }
            let aligned = align_icl(text_rows, codec_rows, &self.tts_pad)?;
            rows.extend(aligned.rows);
            aligned.trailing
        } else {
            for &id in body {
                cancel()?;
                let mut row = self.project_text(id)?;
                add(&mut row, self.codec_row(self.specials.pad)?)?;
                rows.push(row);
            }
            let mut eos = self.tts_eos.clone();
            add(&mut eos, self.codec_row(self.specials.pad)?)?;
            rows.push(eos);
            let mut end = self.tts_pad.clone();
            add(&mut end, self.codec_row(self.specials.bos)?)?;
            rows.push(end);
            vec![self.tts_pad.clone()]
        };
        if rows.len() >= MAX_CONTEXT {
            return Err(invalid("Speech prompt exceeds its context budget."));
        }
        Ok(Prompt { rows, trailing })
    }

    fn forward(
        &self,
        row: &[f32],
        caches: &mut [LayerKvCache],
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<Vec<f32>> {
        let mut hidden = row.to_vec();
        for (layer, cache) in self.layers.iter().zip(caches) {
            cancel()?;
            hidden = layer.forward_step_parallel(
                &hidden,
                self.geometry.heads,
                self.geometry.kv_heads,
                self.geometry.head_dim,
                self.geometry.rope_theta,
                self.geometry.norm_eps,
                cache,
                None,
                self.threads,
            )?;
        }
        rms_norm(&hidden, &self.norm, self.geometry.norm_eps)
    }

    pub(crate) fn generate(
        &self,
        text: &str,
        instruct: Option<&str>,
        reference: Option<&Reference>,
        sink: &mut dyn FnMut([i32; TOTAL_CODE_GROUPS]) -> io::Result<()>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        cancel()?;
        let prompt = self.prompt(text, instruct, reference, cancel)?;
        let mut caches = vec![LayerKvCache::default(); self.layers.len()];
        let mut hidden = Vec::new();
        for row in &prompt.rows {
            hidden = self.forward(row, &mut caches, cancel)?;
        }
        let mut history = Vec::with_capacity(MAX_NEW_TOKENS);
        let mut scratch = Vec::new();
        let mut params = if self.base {
            DecoupledSamplingParams::base_clone()
        } else {
            DecoupledSamplingParams::voice_design()
        };
        // Current Base serving keeps the residual codebooks at full top-p.
        params.subtalker_top_p = 1.0;
        for step in 0..MAX_NEW_TOKENS {
            cancel()?;
            let mut logits = self
                .head
                .forward_vec_parallel(&hidden, None, self.threads)?;
            let vocab = i32::try_from(self.geometry.vocab)
                .map_err(|_| invalid("Speech vocabulary exceeds sampler limits."))?;
            sampling::suppress(&mut logits, vocab - 1024, vocab, self.specials.eos);
            let subsequence = i64::try_from(step * TOTAL_CODE_GROUPS)
                .map_err(|_| invalid("Speech sampler sequence overflow."))?;
            let token = sampling::sample_top_k_p(
                &mut logits,
                SampleTopKPParams {
                    temperature: params.temperature,
                    top_k: params.top_k,
                    top_p: params.top_p,
                    repetition_penalty: params.repetition_penalty,
                    seed: VOICE_SEED,
                    philox_subseq: subsequence,
                },
                &history,
                &mut scratch,
                None,
            )
            .ok_or_else(|| invalid("Speech semantic sampling failed."))?;
            let token =
                i32::try_from(token).map_err(|_| invalid("Invalid speech semantic token."))?;
            if token == self.specials.eos {
                return Ok(());
            }
            let frame = self.predictor.predict_frame_with_control(
                &hidden,
                token,
                &self.codec_embedding,
                params,
                VOICE_SEED,
                subsequence,
                self.threads,
                cancel,
            )?;
            sink(frame)?;
            history.push(token);
            if step + 1 == MAX_NEW_TOKENS {
                break;
            }
            if caches
                .first()
                .is_some_and(|cache| cache.seq_len >= MAX_CONTEXT)
            {
                return Err(invalid(
                    "Speech reached its context limit before a complete end marker.",
                ));
            }
            let mut next = self.sum_codes(&frame)?;
            let overlay = prompt.trailing.get(step).unwrap_or(&self.tts_pad);
            add(&mut next, overlay)?;
            hidden = self.forward(&next, &mut caches, cancel)?;
        }
        Err(invalid(
            "Speech reached its token ceiling before a complete end marker.",
        ))
    }
}

fn align_icl(mut text: Vec<Vec<f32>>, codec: Vec<Vec<f32>>, pad: &[f32]) -> io::Result<Prompt> {
    if codec.is_empty() || pad.is_empty() {
        return Err(invalid("Invalid speech reference alignment."));
    }
    let trailing = if text.len() > codec.len() {
        text.split_off(codec.len())
    } else {
        vec![pad.to_vec()]
    };
    text.resize(codec.len(), pad.to_vec());
    for (row, code) in text.iter_mut().zip(codec) {
        add(row, &code)?;
    }
    Ok(Prompt {
        rows: text,
        trailing,
    })
}

fn embedding_row(table: &[f32], hidden: usize, id: i32) -> io::Result<&[f32]> {
    let id = usize::try_from(id).map_err(|_| invalid("Negative speech codec token."))?;
    let start = id
        .checked_mul(hidden)
        .ok_or_else(|| invalid("Speech embedding offset overflow."))?;
    let end = start
        .checked_add(hidden)
        .ok_or_else(|| invalid("Speech embedding offset overflow."))?;
    table
        .get(start..end)
        .ok_or_else(|| invalid("Speech codec token is out of range."))
}

fn add(left: &mut [f32], right: &[f32]) -> io::Result<()> {
    if left.len() != right.len() {
        return Err(invalid("Speech embedding dimensions differ."));
    }
    for (left, right) in left.iter_mut().zip(right) {
        *left += right;
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{align_icl, embedding_row};

    #[test]
    fn icl_alignment_retains_unconsumed_text_and_pads_short_text() -> std::io::Result<()> {
        let long = align_icl(
            vec![vec![1.0], vec![2.0], vec![3.0]],
            vec![vec![10.0], vec![20.0]],
            &[100.0],
        )?;
        assert_eq!(long.rows, [vec![11.0], vec![22.0]]);
        assert_eq!(long.trailing, [vec![3.0]]);
        let short = align_icl(vec![vec![1.0]], vec![vec![10.0], vec![20.0]], &[100.0])?;
        assert_eq!(short.rows, [vec![11.0], vec![120.0]]);
        assert_eq!(short.trailing, [vec![100.0]]);
        let equal = align_icl(vec![vec![1.0]], vec![vec![10.0]], &[100.0])?;
        assert_eq!(equal.trailing, [vec![100.0]]);
        Ok(())
    }

    #[test]
    fn codec_embedding_lookup_rejects_negative_and_out_of_range_codes() {
        assert_eq!(
            embedding_row(&[1.0, 2.0, 3.0, 4.0], 2, 1).ok(),
            Some(&[3.0, 4.0][..])
        );
        assert!(embedding_row(&[1.0, 2.0], 2, -1).is_err());
        assert!(embedding_row(&[1.0, 2.0], 2, 1).is_err());
        assert!(embedding_row(&[1.0], usize::MAX, 2).is_err());
    }
}
