//! Exact-source overlays that align Qwen3-TTS with the signed Hexagon NPU (`HTP0`) runtime.

use std::{fs, io, path::Path};

type Transform = fn(&str) -> io::Result<String>;

const TRANSFORMS: [(&str, Transform); 11] = [
    ("ggml/include/ggml.h", ggml_header),
    ("ggml/src/ggml.c", ggml_source),
    ("ggml/src/ggml-cpu/ggml-cpu.c", ggml_cpu),
    ("ggml/src/ggml-backend-meta.cpp", ggml_backend_meta),
    ("ggml/src/ggml-backend.cpp", ggml_backend),
    ("src/backend.h", backend_header),
    ("src/kv-cache.h", kv_cache),
    ("src/talker-weights.h", talker_weights),
    ("src/prompt-builder.h", prompt_builder),
    ("src/pipeline-tts.cpp", pipeline_tts),
    ("src/qwen.cpp", qwen_cpp),
];

fn replace(text: &str, before: &str, after: &str) -> io::Result<String> {
    if text.matches(before).count() != 1 {
        return Err(io::Error::other(
            "Pinned Qwen NPU overlay anchor differs.",
        ));
    }
    Ok(text.replacen(before, after, 1))
}

#[allow(
    clippy::redundant_pub_crate,
    reason = "Only the parent source-copy overlay invokes these reviewed transforms."
)]
pub(super) fn apply(destination: &Path) -> io::Result<()> {
    for &(relative, transform) in &TRANSFORMS {
        let path = destination.join(relative);
        let original = fs::read_to_string(&path)?.replace("\r\n", "\n");
        fs::write(path, transform(&original)?)?;
    }
    Ok(())
}

fn ggml_header(text: &str) -> io::Result<String> {
    replace(
        text,
        "        GGML_OP_DSV4_HC_POST,\n\n        GGML_OP_UNARY,",
        "        GGML_OP_DSV4_HC_POST,\n        GGML_OP_TOP_K_MASK,\n\n        GGML_OP_UNARY,",
    )
}

fn ggml_source(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    \"DSV4_HC_POST\",\n\n    \"UNARY\",",
        "    \"DSV4_HC_POST\",\n    \"TOP_K_MASK\",\n\n    \"UNARY\",",
    )?;
    let text = replace(
        &text,
        "static_assert(GGML_OP_COUNT == 101, \"GGML_OP_COUNT != 101\");\n\nstatic const char * GGML_OP_SYMBOL",
        "static_assert(GGML_OP_COUNT == 102, \"GGML_OP_COUNT != 102\");\n\nstatic const char * GGML_OP_SYMBOL",
    )?;
    let text = replace(
        &text,
        "    \"dsv4_hc_post(x, residual, post, comb)\",\n\n    \"unary(x)\",",
        "    \"dsv4_hc_post(x, residual, post, comb)\",\n    \"top_k_mask(x)\",\n\n    \"unary(x)\",",
    )?;
    replace(
        &text,
        "static_assert(GGML_OP_COUNT == 101, \"GGML_OP_COUNT != 101\");\n\nstatic_assert(GGML_OP_POOL_COUNT",
        "static_assert(GGML_OP_COUNT == 102, \"GGML_OP_COUNT != 102\");\n\nstatic_assert(GGML_OP_POOL_COUNT",
    )
}

fn ggml_cpu(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "        case GGML_OP_COUNT:\n            {\n                GGML_ABORT(\"fatal error\");\n            }\n    }\n}",
        "        case GGML_OP_TOP_K_MASK:\n        case GGML_OP_COUNT:\n            {\n                GGML_ABORT(\"fatal error\");\n            }\n    }\n}",
    )?;
    replace(
        &text,
        "        case GGML_OP_DSV4_HC_POST:\n            {\n                n_tasks = n_threads;\n            } break;",
        "        case GGML_OP_DSV4_HC_POST:\n        case GGML_OP_TOP_K_MASK:\n            {\n                n_tasks = n_threads;\n            } break;",
    )
}

fn ggml_backend_meta(text: &str) -> io::Result<String> {
    replace(
        text,
        "            case GGML_OP_DSV4_HC_POST: {\n                split_state = handle_generic(src_ss, /*scalar_only =*/ true);",
        "            case GGML_OP_DSV4_HC_POST:\n            case GGML_OP_TOP_K_MASK: {\n                split_state = handle_generic(src_ss, /*scalar_only =*/ true);",
    )
}

fn ggml_backend(text: &str) -> io::Result<String> {
    replace(
        text,
        "bool ggml_backend_buffer_is_multi_buffer(ggml_backend_buffer_t buffer) {\n    GGML_ASSERT(buffer);\n    return buffer->iface.free_buffer == ggml_backend_multi_buffer_free_buffer;\n}",
        "bool ggml_backend_buffer_is_multi_buffer(ggml_backend_buffer_t buffer) {\n    GGML_ASSERT(buffer);\n    return buffer->iface.free_buffer == ggml_backend_multi_buffer_free_buffer ||\n           (buffer->iface.free_buffer != NULL && buffer->iface.get_base == NULL &&\n            buffer->iface.set_tensor == NULL && buffer->iface.clear != NULL);\n}",
    )
}

fn backend_header(text: &str) -> io::Result<String> {
    replace(
        text,
        "    bp.backend     = device ? ggml_backend_init_by_name(device, nullptr) : ggml_backend_init_best();\n    if (!bp.backend) {",
        "    bp.backend     = device ? ggml_backend_init_by_name(device, nullptr) : ggml_backend_init_best();\n    if (!bp.backend && device && std::strcmp(device, \"CPU\") != 0) {\n        qt_log(QT_LOG_WARN, \"[Load] device %s not available, falling back to CPU\", device);\n        bp.backend = ggml_backend_init_by_name(\"CPU\", nullptr);\n    }\n    if (!bp.backend) {",
    )
}

fn kv_cache(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "#include <cstdlib>\n",
        "#include <cstdlib>\n#include <cstring>\n",
    )?;
    let text = replace(
        &text,
        "    for (int l = 0; l < n_layers; l++) {\n        kv->k4[(size_t) l] = ggml_new_tensor_4d(kv->ctx, GGML_TYPE_F32, head_dim, max_seq_len, n_kv_heads, n_sets);\n        kv->v4[(size_t) l] = ggml_new_tensor_4d(kv->ctx, GGML_TYPE_F32, head_dim, max_seq_len, n_kv_heads, n_sets);",
        "    const enum ggml_type kv_type = (std::strcmp(ggml_backend_name(backend), \"CPU\") == 0) ? GGML_TYPE_F32 : GGML_TYPE_F16;\n    for (int l = 0; l < n_layers; l++) {\n        kv->k4[(size_t) l] = ggml_new_tensor_4d(kv->ctx, kv_type, head_dim, max_seq_len, n_kv_heads, n_sets);\n        kv->v4[(size_t) l] = ggml_new_tensor_4d(kv->ctx, kv_type, head_dim, max_seq_len, n_kv_heads, n_sets);",
    )?;
    replace(
        &text,
        "        (size_t) head_dim * (size_t) max_seq_len * (size_t) n_kv_heads * (size_t) n_sets * sizeof(float);",
        "        (size_t) head_dim * (size_t) max_seq_len * (size_t) n_kv_heads * (size_t) n_sets * ggml_type_size(kv_type);",
    )
}

fn talker_weights(text: &str) -> io::Result<String> {
    replace(
        text,
        "    tw->text_embedding  = gf_load_tensor(&wctx, gf, \"talker.text_embd.weight\");",
        "    tw->text_embedding  = ggml_get_tensor(gf.meta, \"talker.text_embd.weight\");",
    )
}

fn prompt_builder(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    struct ggml_tensor * ids_in = ggml_new_tensor_1d(gctx, GGML_TYPE_I32, count);\n    ggml_set_name(ids_in, \"prompt_text_ids\");\n    ggml_set_input(ids_in);\n\n    struct ggml_tensor * h = ggml_get_rows(gctx, pt->talker.text_embedding, ids_in);\n    ggml_set_name(h, \"prompt_text_embeds\");",
        "    const int          text_dim = (int) pt->talker.text_embedding->ne[0];\n    std::vector<float> embeds((size_t) count * (size_t) text_dim);\n    for (int i = 0; i < count; i++) {\n        embed_row_to_f32(pt->gguf_talker, \"talker.text_embd.weight\", (int) ids[i], text_dim,\n                         embeds.data() + (size_t) i * (size_t) text_dim);\n    }\n    struct ggml_tensor * embeds_in = ggml_new_tensor_2d(gctx, GGML_TYPE_F32, text_dim, count);\n    ggml_set_name(embeds_in, \"prompt_text_embeds\");\n    ggml_set_input(embeds_in);\n    struct ggml_tensor * h = embeds_in;",
    )?;
    replace(
        &text,
        "    ggml_backend_tensor_set(ids_in, ids, 0, (size_t) count * sizeof(int32_t));",
        "    ggml_backend_tensor_set(embeds_in, embeds.data(), 0, embeds.size() * sizeof(float));",
    )
}

fn pipeline_tts(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    if (!pipeline_codec_load(&pt->codec, codec_gguf_path, bp)) {",
        "    BackendPair codec_bp = bp;\n    if (std::strncmp(ggml_backend_name(bp.backend), \"HTP\", 3) == 0) {\n        codec_bp.backend = bp.cpu_backend;\n        codec_bp.has_gpu = false;\n    }\n    if (!pipeline_codec_load(&pt->codec, codec_gguf_path, codec_bp)) {",
    )?;
    let text = replace(
        &text,
        "    if (!kv_cache_init(&pt->code_predictor_kv, pt->code_predictor.num_hidden_layers,\n                       pt->code_predictor.num_key_value_heads, pt->code_predictor.head_dim, pt->num_code_groups,\n                       pt->max_batch, pt->backend)) {",
        "    if (!kv_cache_init(&pt->code_predictor_kv, pt->code_predictor.num_hidden_layers,\n                       pt->code_predictor.num_key_value_heads, pt->code_predictor.head_dim, (int) GGML_PAD(pt->num_code_groups, 64),\n                       pt->max_batch, pt->backend)) {",
    )?;
    let text = replace(
        &text,
        "                     pipeline_tts_cp_graphs_ensure(pt, 1, SAMPLER_TAIL_FIXED);",
        "                     pipeline_tts_cp_graphs_ensure(pt, 1, pt->bp.has_gpu ? SAMPLER_TAIL_FULL : SAMPLER_TAIL_FIXED);",
    )?;
    let text = replace(
        &text,
        "        enum SamplerTail         tail    = SAMPLER_TAIL_FIXED;",
        "        enum SamplerTail         tail    = pt->bp.has_gpu ? SAMPLER_TAIL_FULL : SAMPLER_TAIL_FIXED;",
    )?;
    let text = replace(
        &text,
        "        if (!speaker_encoder_weights_load(&pt->speaker_encoder, pt->gguf_talker, pt->backend) ||",
        "        if (!speaker_encoder_weights_load(&pt->speaker_encoder, pt->gguf_talker, pt->bp.cpu_backend) ||",
    )?;
    replace(
        &text,
        "        if (!speaker_encoder_extract(&pt->speaker_encoder, pt->sched, params->ref_audio_24k, params->ref_n_samples,",
        "        if (!speaker_encoder_extract(&pt->speaker_encoder, pt->codec.sched, params->ref_audio_24k, params->ref_n_samples,",
    )
}

fn qwen_cpp(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "        if (!speaker_encoder_weights_load(&q->pt.speaker_encoder, q->pt.gguf_talker, q->pt.backend) ||",
        "        if (!speaker_encoder_weights_load(&q->pt.speaker_encoder, q->pt.gguf_talker, q->pt.bp.cpu_backend) ||",
    )?;
    replace(
        &text,
        "    if (!speaker_encoder_extract(&q->pt.speaker_encoder, q->pt.sched, job->audio, job->n_samples, emb)) {",
        "    if (!speaker_encoder_extract(&q->pt.speaker_encoder, q->pt.codec.sched, job->audio, job->n_samples, emb)) {",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        backend_header, ggml_backend, ggml_backend_meta, ggml_cpu, ggml_header, ggml_source,
        kv_cache, pipeline_tts, prompt_builder, qwen_cpp, talker_weights,
    };

    #[test]
    fn npu_overlays_reject_source_drift_and_apply_expected_patches() -> std::io::Result<()> {
        for transform in [
            ggml_header,
            ggml_source,
            ggml_cpu,
            ggml_backend_meta,
            ggml_backend,
            backend_header,
            kv_cache,
            talker_weights,
            prompt_builder,
            pipeline_tts,
            qwen_cpp,
        ] {
            assert!(transform("changed").is_err());
        }
        let header = ggml_header("        GGML_OP_DSV4_HC_POST,\n\n        GGML_OP_UNARY,")?;
        assert!(header.contains("GGML_OP_TOP_K_MASK"));
        assert!(ggml_header(&header).is_err());

        let tw = talker_weights(
            "    tw->text_embedding  = gf_load_tensor(&wctx, gf, \"talker.text_embd.weight\");",
        )?;
        assert!(tw.contains("ggml_get_tensor(gf.meta, \"talker.text_embd.weight\")"));
        assert!(talker_weights(&tw).is_err());
        Ok(())
    }
}
