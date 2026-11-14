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
        QwenDecoderLayer, TOTAL_CODE_GROUPS, predictor_workspace::PredictorWorkspace, rms_norm,
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
// A retained snapshot and its restored request copy together consume at most
// 16 MiB. Ordinary request KV growth remains subject to the existing budget.
const MAX_PREFILL_ROWS: usize = 64;
const MAX_PREFILL_CACHE_BYTES: usize = 8 * 1024 * 1024;

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
    prefill_cache: Option<PrefillCache>,
}

struct Prompt {
    rows: Vec<Vec<f32>>,
    trailing: Vec<Vec<f32>>,
    cacheable_rows: usize,
}

struct PrefillCache {
    rows: Vec<Vec<f32>>,
    layers: Vec<LayerKvCache>,
    hidden: Vec<f32>,
}

fn copy_f32(values: &[f32]) -> Option<Vec<f32>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(values.len()).ok()?;
    copy.extend_from_slice(values);
    Some(copy)
}

fn copy_layers(layers: &[LayerKvCache]) -> Option<Vec<LayerKvCache>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(layers.len()).ok()?;
    for layer in layers {
        copy.push(LayerKvCache {
            k: copy_f32(&layer.k)?,
            v: copy_f32(&layer.v)?,
            seq_len: layer.seq_len,
        });
    }
    Some(copy)
}

impl PrefillCache {
    fn row_limit(hidden: usize, kv_width: usize, layers: usize) -> Option<usize> {
        let hidden_bytes = hidden.checked_mul(size_of::<f32>())?;
        let fixed = size_of::<Self>()
            .checked_add(layers.checked_mul(size_of::<LayerKvCache>())?)?
            .checked_add(hidden_bytes)?;
        let per_row = kv_width
            .checked_mul(layers)?
            .checked_mul(2)?
            .checked_mul(size_of::<f32>())?
            .checked_add(hidden_bytes)?
            .checked_add(size_of::<Vec<f32>>())?;
        Some(
            MAX_PREFILL_CACHE_BYTES
                .checked_sub(fixed)?
                .checked_div(per_row)?
                .min(MAX_PREFILL_ROWS),
        )
    }

    fn bytes(rows: &[Vec<f32>], layers: &[LayerKvCache], hidden: &[f32]) -> Option<usize> {
        let mut bytes = size_of::<Self>()
            .checked_add(rows.len().checked_mul(size_of::<Vec<f32>>())?)?
            .checked_add(layers.len().checked_mul(size_of::<LayerKvCache>())?)?
            .checked_add(hidden.len().checked_mul(size_of::<f32>())?)?;
        for row in rows {
            bytes = bytes.checked_add(row.len().checked_mul(size_of::<f32>())?)?;
        }
        for layer in layers {
            bytes = bytes.checked_add(
                layer
                    .k
                    .len()
                    .checked_add(layer.v.len())?
                    .checked_mul(size_of::<f32>())?,
            )?;
        }
        Some(bytes)
    }

    fn capture(rows: &[Vec<f32>], layers: &[LayerKvCache], hidden: &[f32]) -> Option<Self> {
        if rows.is_empty()
            || rows.len() > MAX_PREFILL_ROWS
            || Self::bytes(rows, layers, hidden)? > MAX_PREFILL_CACHE_BYTES
        {
            return None;
        }
        let mut copied_rows = Vec::new();
        copied_rows.try_reserve_exact(rows.len()).ok()?;
        for row in rows {
            copied_rows.push(copy_f32(row)?);
        }
        let snapshot = Self {
            rows: copied_rows,
            layers: copy_layers(layers)?,
            hidden: copy_f32(hidden)?,
        };
        (snapshot.allocated_bytes()? <= MAX_PREFILL_CACHE_BYTES).then_some(snapshot)
    }

    fn allocated_bytes(&self) -> Option<usize> {
        let mut bytes = size_of::<Self>()
            .checked_add(self.rows.capacity().checked_mul(size_of::<Vec<f32>>())?)?
            .checked_add(
                self.layers
                    .capacity()
                    .checked_mul(size_of::<LayerKvCache>())?,
            )?
            .checked_add(self.hidden.capacity().checked_mul(size_of::<f32>())?)?;
        for row in &self.rows {
            bytes = bytes.checked_add(row.capacity().checked_mul(size_of::<f32>())?)?;
        }
        for layer in &self.layers {
            bytes = bytes.checked_add(
                layer
                    .k
                    .capacity()
                    .checked_add(layer.v.capacity())?
                    .checked_mul(size_of::<f32>())?,
            )?;
        }
        Some(bytes)
    }

    fn matches(&self, rows: &[Vec<f32>], layer_count: usize) -> bool {
        self.layers.len() == layer_count
            && self.rows.len() == rows.len()
            && self.rows.iter().zip(rows).all(|(cached, row)| {
                cached.len() == row.len()
                    && cached
                        .iter()
                        .zip(row)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
            })
    }

    fn restore(&self) -> Option<(Vec<LayerKvCache>, Vec<f32>)> {
        let restored = Self {
            rows: Vec::new(),
            layers: copy_layers(&self.layers)?,
            hidden: copy_f32(&self.hidden)?,
        };
        if restored.allocated_bytes()? > MAX_PREFILL_CACHE_BYTES {
            return None;
        }
        Some((restored.layers, restored.hidden))
    }
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
            prefill_cache: None,
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
        let mut cacheable_rows = 0;
        let trailing = if let Some(reference) = reference {
            let reference_ids = self.framed_ids(REFERENCE_TEXT)?;
            let reference_body = &reference_ids[3..reference_ids.len() - 5];
            let mut text_rows = Vec::new();
            for &id in reference_body.iter().chain(body) {
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
            cacheable_rows =
                (rows.len() + reference_body.len().min(aligned.rows.len())).min(MAX_PREFILL_ROWS);
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
        Ok(Prompt {
            rows,
            trailing,
            cacheable_rows,
        })
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

    #[cfg(test)]
    pub(crate) fn generate(
        &mut self,
        text: &str,
        instruct: Option<&str>,
        reference: Option<&Reference>,
        sink: &mut dyn FnMut([i32; TOTAL_CODE_GROUPS]) -> io::Result<()>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        self.generate_observed(text, instruct, reference, sink, cancel, &mut |_| Ok(()))
    }

    pub(crate) fn generate_observed(
        &mut self,
        text: &str,
        instruct: Option<&str>,
        reference: Option<&Reference>,
        sink: &mut dyn FnMut([i32; TOTAL_CODE_GROUPS]) -> io::Result<()>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
        observe: &mut dyn FnMut(crate::observation::TalkerObservation<'_>) -> io::Result<()>,
    ) -> io::Result<()> {
        // Take the snapshot before prompt construction: every failure, including
        // cancelled projection or sink output, discards all reusable KV state.
        let mut prefill_cache = self.prefill_cache.take();
        let result = self.generate_inner(
            text,
            instruct,
            reference,
            sink,
            cancel,
            &mut prefill_cache,
            observe,
        );
        if result.is_ok() {
            self.prefill_cache = prefill_cache;
        }
        result
    }

    fn prefill(
        &self,
        prompt: &Prompt,
        snapshot: &mut Option<PrefillCache>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<(Vec<LayerKvCache>, Vec<f32>)> {
        cancel()?;
        let prefix = prompt
            .cacheable_rows
            .min(MAX_PREFILL_ROWS)
            .min(
                self.geometry
                    .kv_heads
                    .checked_mul(self.geometry.head_dim)
                    .and_then(|width| {
                        PrefillCache::row_limit(self.geometry.hidden, width, self.layers.len())
                    })
                    .unwrap_or(0),
            )
            .min(prompt.rows.len());
        let restored = snapshot
            .as_ref()
            .filter(|cache| cache.matches(&prompt.rows[..prefix], self.layers.len()))
            .and_then(PrefillCache::restore);
        let reused = restored.is_some();
        if !reused {
            *snapshot = None;
        }
        let (mut caches, mut hidden) = restored
            .unwrap_or_else(|| (vec![LayerKvCache::default(); self.layers.len()], Vec::new()));
        for (position, row) in prompt
            .rows
            .iter()
            .enumerate()
            .skip(if reused { prefix } else { 0 })
        {
            hidden = self.forward(row, &mut caches, cancel)?;
            if !reused && prefix != 0 && position + 1 == prefix {
                *snapshot = PrefillCache::capture(&prompt.rows[..prefix], &caches, &hidden);
            }
        }
        Ok((caches, hidden))
    }

    #[expect(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "Keep ordered serving arithmetic and observer failure inside the existing snapshot transaction."
    )]
    fn generate_inner(
        &self,
        text: &str,
        instruct: Option<&str>,
        reference: Option<&Reference>,
        sink: &mut dyn FnMut([i32; TOTAL_CODE_GROUPS]) -> io::Result<()>,
        cancel: &mut dyn FnMut() -> io::Result<()>,
        prefill_cache: &mut Option<PrefillCache>,
        observe: &mut dyn FnMut(crate::observation::TalkerObservation<'_>) -> io::Result<()>,
    ) -> io::Result<()> {
        use crate::observation::TalkerObservation;

        cancel()?;
        let prompt = self.prompt(text, instruct, reference, cancel)?;
        observe(TalkerObservation::Prompt {
            rows: &prompt.rows,
            trailing: &prompt.trailing,
            cacheable_rows: prompt.cacheable_rows,
        })?;
        let (mut caches, mut hidden) = self.prefill(&prompt, prefill_cache, cancel)?;
        let mut history = Vec::with_capacity(MAX_NEW_TOKENS);
        // The sampler marks vocabulary entries while applying repetition
        // penalties after the first frame; its workspace is caller-owned.
        let mut scratch = vec![0; self.geometry.vocab];
        // Acoustic frames have independent sessions. Retain only their buffer
        // capacities within this utterance; the frame guard clears all KV and
        // sampler lengths on every return, cancellation and unwind.
        let mut predictor_workspace = PredictorWorkspace::default();
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
            observe(TalkerObservation::Logits {
                frame: step,
                hidden: &hidden,
                logits: &logits,
                cache: &caches,
            })?;
            cancel()?;
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
            observe(TalkerObservation::Decision {
                frame: step,
                token,
                eos: token == self.specials.eos,
            })?;
            if token == self.specials.eos {
                return Ok(());
            }
            let frame = self.predictor.predict_frame_observed_reusing(
                &hidden,
                token,
                &self.codec_embedding,
                params,
                VOICE_SEED,
                subsequence,
                self.threads,
                cancel,
                &mut predictor_workspace,
                &mut |observation| {
                    observe(TalkerObservation::Predictor {
                        frame: step,
                        observation,
                    })
                },
            )?;
            observe(TalkerObservation::Frame {
                frame: step,
                codes: &frame,
            })?;
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
    text.resize_with(codec.len(), || pad.to_vec());
    for (row, code) in text.iter_mut().zip(codec) {
        add(row, &code)?;
    }
    Ok(Prompt {
        rows: text,
        trailing,
        cacheable_rows: 0,
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
pub(crate) mod tests {
    use super::{
        Geometry, MAX_PREFILL_CACHE_BYTES, MAX_PREFILL_ROWS, PrefillCache, Prompt, REFERENCE_TEXT,
        Reference, Specials, Talker, align_icl, embedding_row,
    };
    use crate::{
        engine::{CodePredictorModel, LinearProjection, QwenDecoderLayer, WeightQuantization},
        tokenizer::Tokenizer,
    };

    pub(crate) fn prompt_fixture(base: bool) -> std::io::Result<Talker> {
        let mut tokenizer = Tokenizer::new();
        tokenizer.finish()?;
        for (text, id) in [
            ("<|im_start|>", 1),
            ("assistant", 2),
            ("\n", 3),
            ("x", 4),
            ("<|im_end|>", 5),
            ("user", 6),
            ("i", 7),
            (REFERENCE_TEXT, 8),
        ] {
            tokenizer.add_special(text.as_bytes(), id)?;
        }
        let linear =
            || LinearProjection::new("fixture", 1, 1, WeightQuantization::F32(vec![1.0]), None);
        Ok(Talker {
            geometry: Geometry {
                hidden: 1,
                intermediate: 1,
                layers: 0,
                heads: 1,
                kv_heads: 1,
                head_dim: 2,
                vocab: 10,
                rope_theta: 1_000_000.0,
                norm_eps: 1e-6,
            },
            layers: Vec::new(),
            norm: vec![1.0],
            head: linear()?,
            text_embedding: LinearProjection::new(
                "fixture.text",
                1,
                9,
                WeightQuantization::F32(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
                None,
            )?,
            text_fc1: linear()?,
            text_fc2: linear()?,
            codec_embedding: vec![
                0.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0, 8000.0, 9000.0,
            ],
            predictor: CodePredictorModel {
                hidden_size: 1,
                num_q_heads: 1,
                num_kv_heads: 1,
                head_dim: 2,
                vocab_size: 2,
                num_acoustic_codebooks: 15,
                rope_theta: 1_000_000.0,
                rms_norm_eps: 1e-6,
                mtp_proj: None,
                layers: Vec::new(),
                norm_w: vec![1.0],
                codec_embeddings: (1_u8..=15)
                    .map(|codebook| vec![0.0, f32::from(codebook)])
                    .collect(),
                lm_heads: Vec::new(),
            },
            tokenizer,
            specials: Specials {
                pad: 0,
                bos: 1,
                eos: 2,
                nothink: 3,
                think_bos: 4,
                think_eos: 5,
                text_pad: 0,
                text_bos: 0,
                text_eos: 0,
            },
            tts_pad: vec![100.0],
            tts_bos: vec![200.0],
            tts_eos: vec![300.0],
            threads: 1,
            base,
            prefill_cache: None,
        })
    }

    fn assert_rows(rows: &[Vec<f32>], expected: &[f32]) {
        assert_eq!(rows.len(), expected.len());
        for (row, expected) in rows.iter().zip(expected) {
            assert_eq!(row.len(), 1);
            assert!(
                (row[0] - expected).abs() < 0.001,
                "unexpected prompt row: {} vs {expected}",
                row[0]
            );
        }
    }

    fn tiny_layer() -> std::io::Result<QwenDecoderLayer> {
        let projection = |name, input, output, values| {
            LinearProjection::new(name, input, output, WeightQuantization::F32(values), None)
        };
        Ok(QwenDecoderLayer {
            attn_norm_w: vec![1.0],
            q_proj: projection("fixture.q", 1, 2, vec![0.3, -0.7])?,
            k_proj: projection("fixture.k", 1, 2, vec![-0.8, 0.4])?,
            v_proj: projection("fixture.v", 1, 2, vec![0.2, 0.6])?,
            o_proj: projection("fixture.o", 2, 1, vec![-0.5, 0.9])?,
            q_norm_w: vec![1.0, 0.7],
            k_norm_w: vec![0.9, 1.2],
            ffn_norm_w: vec![1.1],
            gate_proj: projection("fixture.gate", 1, 2, vec![0.3, -0.9])?,
            up_proj: projection("fixture.up", 1, 2, vec![-0.4, 0.2])?,
            down_proj: projection("fixture.down", 2, 1, vec![0.7, -0.1])?,
        })
    }

    #[test]
    fn prefix_prefill_reuses_real_decoder_state_with_bit_exact_suffixes() -> std::io::Result<()> {
        let mut talker = prompt_fixture(true)?;
        talker.layers = vec![tiny_layer()?];
        let mut prompt = Prompt {
            rows: vec![vec![0.25], vec![-0.6], vec![0.125]],
            trailing: Vec::new(),
            cacheable_rows: 2,
        };
        let mut snapshot = None;
        let mut cold_calls = 0;
        let cold = talker.prefill(&prompt, &mut snapshot, &mut || {
            cold_calls += 1;
            Ok(())
        })?;
        let mut warm_calls = 0;
        let warm = talker.prefill(&prompt, &mut snapshot, &mut || {
            warm_calls += 1;
            Ok(())
        })?;
        assert_eq!((cold_calls, warm_calls), (4, 2));
        let bits = |values: &[f32]| {
            values
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        };
        let assert_state =
            |a: &(Vec<crate::engine::LayerKvCache>, Vec<f32>),
             b: &(Vec<crate::engine::LayerKvCache>, Vec<f32>)| {
                assert_eq!(bits(&a.1), bits(&b.1));
                assert_eq!(a.0.len(), b.0.len());
                for (a, b) in a.0.iter().zip(&b.0) {
                    assert_eq!(a.seq_len, b.seq_len);
                    assert_eq!(bits(&a.k), bits(&b.k));
                    assert_eq!(bits(&a.v), bits(&b.v));
                }
            };
        assert_state(&cold, &warm);
        prompt.rows[2][0] = -0.375;
        let edited_suffix = talker.prefill(&prompt, &mut snapshot, &mut || Ok(()))?;
        let edited_cold = talker.prefill(&prompt, &mut None, &mut || Ok(()))?;
        assert_state(&edited_suffix, &edited_cold);
        prompt.rows[0][0] = 0.5;
        let mut changed_calls = 0;
        let edited_prefix = talker.prefill(&prompt, &mut snapshot, &mut || {
            changed_calls += 1;
            Ok(())
        })?;
        assert_eq!(changed_calls, cold_calls);
        assert_state(
            &edited_prefix,
            &talker.prefill(&prompt, &mut None, &mut || Ok(()))?,
        );
        Ok(())
    }

    #[test]
    fn prefix_snapshot_checks_bitwise_identity_row_and_byte_caps() {
        assert_eq!(PrefillCache::row_limit(2048, 8 * 128, 28), Some(35));
        assert_eq!(PrefillCache::row_limit(2048, usize::MAX, 28), None);
        let rows = [vec![0.0]];
        let snapshot = PrefillCache::capture(&rows, &[], &[1.0]);
        assert!(
            snapshot
                .as_ref()
                .is_some_and(|cache| cache.matches(&rows, 0))
        );
        assert!(
            snapshot
                .as_ref()
                .is_some_and(|cache| !cache.matches(&[vec![-0.0]], 0))
        );
        assert!(PrefillCache::capture(&vec![vec![0.0]; MAX_PREFILL_ROWS + 1], &[], &[]).is_none());
        assert!(PrefillCache::capture(&[], &[], &[]).is_none());
        let oversized = vec![0.0; MAX_PREFILL_CACHE_BYTES / size_of::<f32>()];
        assert!(PrefillCache::capture(&rows, &[], &oversized).is_none());
    }

    #[test]
    fn prefix_snapshot_discards_on_cancelled_prompt_layer_and_output() -> std::io::Result<()> {
        let mut talker = prompt_fixture(true)?;
        talker.geometry.vocab = 1026;
        talker.codec_embedding.resize(1026, 0.0);
        talker.layers = vec![tiny_layer()?];
        let eos_head = |eos| {
            let mut bias = vec![-1000.0; 1026];
            bias[if eos { 2 } else { 0 }] = 1000.0;
            LinearProjection::new(
                "fixture.semantic",
                1,
                1026,
                WeightQuantization::F32(vec![0.0; 1026]),
                Some(bias),
            )
        };
        talker.head = eos_head(true)?;
        let reference = Reference {
            speaker: vec![6000.0],
            codes: vec![[1; 16]],
        };
        talker.generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || Ok(()))?;
        assert!(talker.prefill_cache.is_some());
        let mut cancelled = || {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ))
        };
        assert!(
            talker
                .generate("x", None, Some(&reference), &mut |_| Ok(()), &mut cancelled)
                .is_err()
        );
        assert!(talker.prefill_cache.is_none());
        talker.generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || Ok(()))?;
        // Count prompt cancellation observations independently of layer work,
        // then interrupt the first uncached decoder position after restore.
        let mut prompt_checks = 0;
        let _ = talker.prompt("x", None, Some(&reference), &mut || {
            prompt_checks += 1;
            Ok(())
        })?;
        let mut checks = 0;
        assert!(
            talker
                .generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || {
                    checks += 1;
                    if checks == prompt_checks + 3 {
                        cancelled()
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(talker.prefill_cache.is_none());
        talker.generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || Ok(()))?;
        talker.head = eos_head(false)?;
        talker.predictor.codec_embeddings = vec![vec![0.0; 2]; 15];
        talker.predictor.lm_heads = (0..15)
            .map(|_| {
                LinearProjection::new(
                    "fixture.acoustic",
                    1,
                    2,
                    WeightQuantization::F32(vec![0.0; 2]),
                    Some(vec![1000.0, -1000.0]),
                )
            })
            .collect::<std::io::Result<_>>()?;
        assert!(
            talker
                .generate(
                    "x",
                    None,
                    Some(&reference),
                    &mut |_| { Err(std::io::Error::other("sink failed")) },
                    &mut || Ok(())
                )
                .is_err()
        );
        assert!(talker.prefill_cache.is_none());
        Ok(())
    }

    fn reference_fixture() -> std::io::Result<Talker> {
        let mut talker = prompt_fixture(false)?;
        talker.geometry.vocab = 1026;
        talker.codec_embedding = vec![0.0; talker.geometry.vocab];
        // Prompt's final codec BOS produces a positive hidden state. The first
        // emitted frame then produces a negative state, selecting EOS on the
        // second semantic sample after repetition history becomes nonempty.
        talker.codec_embedding[0] = -3.0;
        talker.codec_embedding[1] = 2.0;
        talker.tts_pad = vec![-1.0];
        let mut head = vec![0.0; talker.geometry.vocab];
        head[0] = 1000.0;
        head[2] = -1000.0;
        talker.head = LinearProjection::new(
            "fixture.semantic",
            1,
            talker.geometry.vocab,
            WeightQuantization::F32(head),
            None,
        )?;
        talker.predictor.codec_embeddings = vec![vec![0.0; 2]; 15];
        talker.predictor.lm_heads = (0..15)
            .map(|_| {
                LinearProjection::new(
                    "fixture.acoustic",
                    1,
                    2,
                    WeightQuantization::F32(vec![0.0; 2]),
                    Some(vec![1000.0, -1000.0]),
                )
            })
            .collect::<std::io::Result<_>>()?;
        Ok(talker)
    }

    #[test]
    fn reference_generation_reuses_vocabulary_workspace_through_second_sample_and_eos()
    -> std::io::Result<()> {
        let mut talker = reference_fixture()?;
        let mut frames = Vec::new();
        talker.generate(
            REFERENCE_TEXT,
            Some("i"),
            None,
            &mut |frame| {
                frames.push(frame);
                Ok(())
            },
            &mut || Ok(()),
        )?;
        assert_eq!(frames, [[0; 16]]);
        Ok(())
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Retain one complete ordered miniature graph schedule through both semantic decisions and all fifteen residual codebooks."
    )]
    fn tts_observer_preserves_complete_prompt_logits_caches_codes_and_eos() -> std::io::Result<()> {
        use crate::observation::{PredictorObservation, TalkerObservation};

        let mut talker = reference_fixture()?;
        talker.layers = vec![tiny_layer()?, tiny_layer()?];
        talker.predictor.layers = vec![tiny_layer()?, tiny_layer()?];
        let mut schedule = Vec::new();
        let mut prompt_len = 0;
        let mut frames = Vec::new();
        talker.generate_observed(
            "x",
            Some("i"),
            None,
            &mut |frame| {
                frames.push(frame);
                Ok(())
            },
            &mut || Ok(()),
            &mut |event| {
                match event {
                    TalkerObservation::Prompt {
                        rows,
                        trailing,
                        cacheable_rows,
                    } => {
                        prompt_len = rows.len();
                        assert_eq!(prompt_len, 16);
                        assert_eq!(rows[15], [1.0]);
                        assert_eq!(trailing, [vec![-1.0]]);
                        assert_eq!(cacheable_rows, 0);
                        schedule.push((0, 0));
                    }
                    TalkerObservation::Logits {
                        frame,
                        hidden,
                        logits,
                        cache,
                    } => {
                        assert_eq!(hidden.len(), 1);
                        assert_eq!(logits.len(), 1026);
                        assert!(logits[0] * (if frame == 0 { 1.0 } else { -1.0 }) > 900.0);
                        assert_eq!(logits[2].to_bits(), (-logits[0]).to_bits());
                        assert_eq!(cache.len(), 2);
                        for layer in cache {
                            assert_eq!(layer.seq_len, prompt_len + frame);
                            assert_eq!(layer.k.len(), 2 * layer.seq_len);
                            assert_eq!(layer.v.len(), 2 * layer.seq_len);
                            assert!(layer.k.iter().chain(&layer.v).all(|n| n.is_finite()));
                        }
                        // Reserved-token suppression has not yet replaced raw logits.
                        assert_eq!(logits[1025], 0.0);
                        schedule.push((1, frame));
                    }
                    TalkerObservation::Decision { frame, token, eos } => {
                        assert_eq!(
                            (token, eos),
                            if frame == 0 { (0, false) } else { (2, true) }
                        );
                        schedule.push((2, frame));
                    }
                    TalkerObservation::Predictor { frame, observation } => {
                        assert_eq!(frame, 0);
                        match observation {
                            PredictorObservation::Prefill { cache } => {
                                assert_eq!(cache.len(), 2);
                                assert!(cache.iter().all(|layer| layer.seq_len == 1
                                    && layer.k.len() == 2
                                    && layer.v.len() == 2));
                                schedule.push((3, 0));
                            }
                            PredictorObservation::Logits {
                                codebook,
                                hidden,
                                logits,
                                cache,
                            } => {
                                assert_eq!(hidden.len(), 1);
                                assert_eq!(logits, [1000.0, -1000.0]);
                                assert!(cache.iter().all(|layer| layer.seq_len == codebook + 2
                                    && layer.k.len() == (codebook + 2) * 2
                                    && layer.v.len() == (codebook + 2) * 2));
                                schedule.push((4, codebook));
                            }
                            PredictorObservation::Decision { codebook, token } => {
                                assert_eq!(token, 0);
                                schedule.push((5, codebook));
                            }
                        }
                    }
                    TalkerObservation::Frame { frame, codes } => {
                        assert_eq!((frame, *codes), (0, [0; 16]));
                        schedule.push((6, frame));
                    }
                }
                Ok(())
            },
        )?;
        let mut expected = vec![(0, 0), (1, 0), (2, 0), (3, 0)];
        for codebook in 0..15 {
            expected.extend([(4, codebook), (5, codebook)]);
        }
        expected.extend([(6, 0), (1, 1), (2, 1)]);
        assert_eq!(schedule, expected);
        assert_eq!(frames, [[0; 16]]);
        Ok(())
    }

    #[test]
    fn tts_observer_each_graph_boundary_can_refuse_or_unwind_without_partial_completion()
    -> std::io::Result<()> {
        use std::panic::AssertUnwindSafe;

        // Prompt, first raw semantic logits/decision, predictor prefill,
        // fifteen raw-logit/decision pairs, frame, final logits/EOS decision.
        for stop in 1..=37 {
            for unwind in [false, true] {
                let mut talker = reference_fixture()?;
                talker.layers = vec![tiny_layer()?, tiny_layer()?];
                talker.predictor.layers = vec![tiny_layer()?, tiny_layer()?];
                let mut seen = 0;
                let mut delivered = 0;
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    talker.generate_observed(
                        "x",
                        Some("i"),
                        None,
                        &mut |_| {
                            delivered += 1;
                            Ok(())
                        },
                        &mut || Ok(()),
                        &mut |_| {
                            seen += 1;
                            if seen == stop {
                                if unwind {
                                    std::panic::resume_unwind(Box::new("observer refused"));
                                }
                                return Err(std::io::Error::other("observer refused"));
                            }
                            Ok(())
                        },
                    )
                }));
                assert_eq!(seen, stop);
                assert!(result.map_or(unwind, |result| result.is_err()));
                assert_eq!(delivered, usize::from(stop > 35));
                assert!(talker.prefill_cache.is_none());
                let mut fresh_frames = Vec::new();
                talker.generate(
                    "x",
                    Some("i"),
                    None,
                    &mut |frame| {
                        fresh_frames.push(frame);
                        Ok(())
                    },
                    &mut || Ok(()),
                )?;
                assert_eq!(fresh_frames, [[0; 16]]);
            }
        }
        Ok(())
    }

    #[test]
    fn tts_observer_refusal_and_unwind_invalidate_prefix_before_fresh_replay() -> std::io::Result<()>
    {
        use std::panic::AssertUnwindSafe;

        let mut talker = prompt_fixture(true)?;
        talker.geometry.vocab = 1026;
        talker.codec_embedding.resize(1026, 0.0);
        talker.layers = vec![tiny_layer()?, tiny_layer()?];
        let mut bias = vec![-1000.0; 1026];
        bias[2] = 1000.0;
        talker.head = LinearProjection::new(
            "fixture.semantic",
            1,
            1026,
            WeightQuantization::F32(vec![0.0; 1026]),
            Some(bias),
        )?;
        let reference = Reference {
            speaker: vec![6000.0],
            codes: vec![[1; 16]],
        };
        for stop in 1..=3 {
            for unwind in [false, true] {
                talker.generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || Ok(()))?;
                assert!(talker.prefill_cache.is_some());
                let mut seen = 0;
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    talker.generate_observed(
                        "x",
                        None,
                        Some(&reference),
                        &mut |_| Ok(()),
                        &mut || Ok(()),
                        &mut |_| {
                            seen += 1;
                            if seen == stop {
                                if unwind {
                                    std::panic::resume_unwind(Box::new("observer refused"));
                                }
                                return Err(std::io::Error::other("observer refused"));
                            }
                            Ok(())
                        },
                    )
                }));
                assert_eq!(seen, stop);
                assert!(result.map_or(unwind, |result| result.is_err()));
                assert!(talker.prefill_cache.is_none());
            }
        }
        talker.generate("x", None, Some(&reference), &mut |_| Ok(()), &mut || Ok(()))?;
        assert!(talker.prefill_cache.is_some());
        Ok(())
    }

    #[test]
    fn complete_design_prompt_keeps_instruction_role_codec_and_text_order() -> std::io::Result<()> {
        let talker = prompt_fixture(false)?;
        let prompt = talker.prompt("x", Some("i"), None, &mut || Ok(()))?;
        // Independent one-dimensional sentinel vectors identify every prompt slot.
        // Noninteger entries are fixed SiLU(1..=7) observations, not an execution oracle.
        assert_rows(
            &prompt.rows,
            &[
                0.731_058_6,
                5.985_164_6,
                2.857_722_5,
                6.993_623,
                4.966_535_6,
                2.857_722_5,
                0.731_058_6,
                1.761_594,
                2.857_722_5,
                3100.0,
                4100.0,
                5100.0,
                200.0,
                3.928_055,
                300.0,
                1100.0,
            ],
        );
        assert_rows(&prompt.trailing, &[100.0]);
        Ok(())
    }

    #[test]
    fn complete_base_prompt_inserts_speaker_and_keeps_reference_text_suffix() -> std::io::Result<()>
    {
        let talker = prompt_fixture(true)?;
        let mut frame = [1; 16];
        frame[0] = 6;
        let reference = Reference {
            speaker: vec![6000.0],
            codes: vec![frame],
        };
        let prompt = talker.prompt("x", None, Some(&reference), &mut || Ok(()))?;
        assert_rows(
            &prompt.rows,
            &[
                0.731_058_6,
                1.761_594,
                2.857_722_5,
                3100.0,
                4100.0,
                5100.0,
                6100.0,
                200.0,
                1_007.997_3,
                6_123.928,
            ],
        );
        assert_rows(&prompt.trailing, &[300.0]);
        assert!(
            talker
                .prompt("x", Some("i"), Some(&reference), &mut || Ok(()))
                .is_err()
        );
        let cancelled = talker.prompt("x", None, Some(&reference), &mut || {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ))
        });
        assert!(cancelled.is_err_and(|error| error.kind() == std::io::ErrorKind::Interrupted));
        Ok(())
    }

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
