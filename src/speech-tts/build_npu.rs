//! Exact-source overlays that align Qwen3-TTS with the signed Hexagon NPU (`HTP0`) runtime.

use std::{fs, io, path::Path};

type Transform = fn(&str) -> io::Result<String>;

const TRANSFORMS: [(&str, Transform); 13] = [
    ("ggml/include/ggml.h", ggml_header),
    ("ggml/src/ggml.c", ggml_source),
    ("ggml/src/ggml-cpu/ggml-cpu.c", ggml_cpu),
    ("ggml/src/ggml-backend-meta.cpp", ggml_backend_meta),
    ("ggml/src/ggml-backend.cpp", ggml_backend),
    ("src/backend.h", backend_header),
    ("src/kv-cache.h", kv_cache),
    ("src/talker-weights.h", talker_weights),
    ("src/code-predictor-weights.h", code_predictor_weights),
    ("src/talker-forward.h", talker_forward),
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

fn code_predictor_weights(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    struct ggml_tensor *              norm_w;\n",
        "    struct ggml_tensor *              norm_w;\n    struct ggml_tensor *              talker_codec_embedding;\n",
    )?;
    replace(
        &text,
        "    cw->norm_w = gf_load_tensor(&wctx, gf, \"code_pred.output_norm.weight\");\n",
        "    cw->norm_w                 = gf_load_tensor(&wctx, gf, \"code_pred.output_norm.weight\");\n    cw->talker_codec_embedding = gf_load_tensor(&wctx, gf, \"talker.codec_embd.weight\");\n",
    )
}

fn talker_forward(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    if (record_taps || read_hidden_host) {\n        ggml_set_output(h_final);\n    }\n\n    // Bridge: the last position hidden copies on device into column\n    // `kv_set` of the persistent [hidden, max_batch] tensor. Constant\n    // destination address across steps, so the decode graph topology\n    // stays replayable.\n    struct ggml_tensor * h_last =\n        ggml_view_1d(gctx, h_final, hidden, (size_t) (T - 1) * (size_t) hidden * sizeof(float));\n    struct ggml_tensor * bridge_col = ggml_view_1d(gctx, hidden_bridge, hidden, (size_t) kv_set * hidden_bridge->nb[1]);\n    struct ggml_tensor * bridge_cpy = ggml_cpy(gctx, h_last, bridge_col);\n\n    // codec_head: [hidden, vocab]. ggml_mul_mat returns [vocab, T].\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n\n    if (record_taps) {\n        for (int i = 0; i < TALKER_N_BISECT_LAYERS; i++) {\n            if (taps[(size_t) i]) {\n                ggml_build_forward_expand(gf, taps[(size_t) i]);\n            }\n        }\n        ggml_build_forward_expand(gf, h_final);\n    }\n    ggml_build_forward_expand(gf, logits);\n    ggml_build_forward_expand(gf, bridge_cpy);",
        "    ggml_set_output(h_final);\n    (void) hidden_bridge;\n\n    // codec_head: [hidden, vocab]. ggml_mul_mat returns [vocab, T].\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n\n    if (record_taps) {\n        for (int i = 0; i < TALKER_N_BISECT_LAYERS; i++) {\n            if (taps[(size_t) i]) {\n                ggml_build_forward_expand(gf, taps[(size_t) i]);\n            }\n        }\n    }\n    ggml_build_forward_expand(gf, h_final);\n    ggml_build_forward_expand(gf, logits);",
    )?;
    let text = replace(
        &text,
        "    {\n        size_t row_bytes = (size_t) vocab * sizeof(float);\n        ggml_backend_tensor_get(logits, out->logits_last.data(), (size_t) (T - 1) * row_bytes, row_bytes);\n    }",
        "    {\n        size_t row_bytes = (size_t) vocab * sizeof(float);\n        ggml_backend_tensor_get(logits, out->logits_last.data(), (size_t) (T - 1) * row_bytes, row_bytes);\n        size_t hrow_bytes = (size_t) hidden * sizeof(float);\n        ggml_backend_tensor_get(h_final, (char *) hidden_bridge->data + (size_t) kv_set * hidden_bridge->nb[1],\n                                (size_t) (T - 1) * hrow_bytes, hrow_bytes);\n    }",
    )?;
    let text = replace(
        &text,
        "    struct ggml_tensor * id0  = ggml_view_1d(gctx, ids_in, N, 0);\n    struct ggml_tensor * x_in = ggml_get_rows(gctx, tw->codec_embedding, id0);\n    for (int g = 0; g < n_acoustic; g++) {\n        struct ggml_tensor * idg = ggml_view_1d(gctx, ids_in, N, (size_t) (g + 1) * (size_t) N * sizeof(int32_t));\n        x_in                     = ggml_add(gctx, x_in, ggml_get_rows(gctx, acoustic_embd[g], idg));\n    }\n    x_in = ggml_add(gctx, x_in, overlay);\n    ggml_set_name(x_in, \"input_embed\");\n\n    struct ggml_cgraph * gf = ggml_new_graph_custom(gctx, max_nodes, false);\n\n    struct ggml_tensor * h = x_in;\n    for (int l = 0; l < n_layers; l++) {\n        h = talker_layer_forward_batch(gctx, tw, tw->layers[(size_t) l], h, pos_in, mask_in, rows_in,\n                                       kv->k4[(size_t) l], kv->v4[(size_t) l], N, n_kv_pad, use_flash_attn, clamp_fp16,\n                                       gf);\n    }\n\n    struct ggml_tensor * h_final = ggml_rms_norm(gctx, h, tw->rms_norm_eps);\n    h_final                      = ggml_mul(gctx, h_final, tw->norm_w);\n    ggml_set_name(h_final, \"hidden_final\");\n\n    struct ggml_tensor * bridge_cols = ggml_view_2d(gctx, hidden_bridge, hidden, N, hidden_bridge->nb[1], 0);\n    struct ggml_tensor * bridge_cpy  = ggml_cpy(gctx, h_final, bridge_cols);\n\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n    ggml_set_output(logits);\n    ggml_build_forward_expand(gf, logits);\n    ggml_build_forward_expand(gf, bridge_cpy);",
        "    (void) ids_in;\n    (void) acoustic_embd;\n    (void) n_acoustic;\n    (void) hidden_bridge;\n    struct ggml_tensor * x_in = overlay;\n    ggml_set_name(x_in, \"input_embed\");\n\n    struct ggml_cgraph * gf = ggml_new_graph_custom(gctx, max_nodes, false);\n\n    struct ggml_tensor * h = x_in;\n    for (int l = 0; l < n_layers; l++) {\n        h = talker_layer_forward_batch(gctx, tw, tw->layers[(size_t) l], h, pos_in, mask_in, rows_in,\n                                       kv->k4[(size_t) l], kv->v4[(size_t) l], N, n_kv_pad, use_flash_attn, clamp_fp16,\n                                       gf);\n    }\n\n    struct ggml_tensor * h_final = ggml_rms_norm(gctx, h, tw->rms_norm_eps);\n    h_final                      = ggml_mul(gctx, h_final, tw->norm_w);\n    ggml_set_name(h_final, \"hidden_final\");\n    ggml_set_output(h_final);\n\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n    ggml_set_output(logits);\n    ggml_build_forward_expand(gf, h_final);\n    ggml_build_forward_expand(gf, logits);",
    )?;
    let text = replace(
        &text,
        "    ggml_backend_tensor_set(tg->ids_in, frame_ids, 0, (size_t) (1 + n_acoustic) * (size_t) N * sizeof(int32_t));\n    ggml_backend_tensor_set(tg->overlay, overlays, 0, (size_t) tw->hidden_size * (size_t) N * sizeof(float));",
        "    (void) frame_ids;\n    ggml_backend_tensor_set(tg->overlay, overlays, 0, (size_t) tw->hidden_size * (size_t) N * sizeof(float));",
    )?;
    replace(
        &text,
        "    out->logits_last.assign((size_t) tw->vocab_size * (size_t) N, 0.0f);\n    ggml_backend_tensor_get(tg->logits, out->logits_last.data(), 0,\n                            (size_t) tw->vocab_size * (size_t) N * sizeof(float));",
        "    out->logits_last.assign((size_t) tw->vocab_size * (size_t) N, 0.0f);\n    ggml_backend_tensor_get(tg->logits, out->logits_last.data(), 0,\n                            (size_t) tw->vocab_size * (size_t) N * sizeof(float));\n    ggml_backend_tensor_get(ggml_get_tensor(tg->ctx, \"hidden_final\"), hidden_bridge->data, 0,\n                            (size_t) tw->hidden_size * (size_t) N * sizeof(float));",
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
        "            s.sampler_buf = ggml_backend_alloc_ctx_tensors(s.sampler_ctx, pt->backend);\n        }\n        if (!s.sampler_ctx || !s.sampler_buf) {\n            qt_log(QT_LOG_ERROR, \"[Pipeline] sampler state allocation failed (N=%d)\", N);\n            if (s.sampler_ctx) {\n                ggml_free(s.sampler_ctx);\n                s.sampler_ctx = NULL;\n            }\n            return false;\n        }\n        ggml_backend_buffer_clear(s.sampler_buf, 0);\n    }\n\n    return code_predictor_frame_graph_build(&pt->code_predictor, &pt->code_predictor_kv, pt->backend,\n                                            pt->talker.codec_embedding, pt->hidden_bridge, &s.sampler, tail, N,\n                                            pt->use_flash_attn, pt->clamp_fp16, &frame);",
        "            s.sampler_buf = ggml_backend_alloc_ctx_tensors(s.sampler_ctx, pt->bp.cpu_backend);\n        }\n        if (!s.sampler_ctx || !s.sampler_buf) {\n            qt_log(QT_LOG_ERROR, \"[Pipeline] sampler state allocation failed (N=%d)\", N);\n            if (s.sampler_ctx) {\n                ggml_free(s.sampler_ctx);\n                s.sampler_ctx = NULL;\n            }\n            return false;\n        }\n        ggml_backend_buffer_clear(s.sampler_buf, 0);\n    }\n\n    return code_predictor_frame_graph_build(&pt->code_predictor, &pt->code_predictor_kv, pt->bp.cpu_backend,\n                                            pt->code_predictor.talker_codec_embedding, pt->hidden_bridge, &s.sampler,\n                                            tail, N, false, pt->clamp_fp16, &frame);",
    )?;
    let text = replace(
        &text,
        "    if (!code_predictor_weights_load(&pt->code_predictor, pt->gguf_talker, pt->backend)) {",
        "    if (!code_predictor_weights_load(&pt->code_predictor, pt->gguf_talker, pt->bp.cpu_backend)) {",
    )?;
    let text = replace(
        &text,
        "    if (!pipeline_codec_load(&pt->codec, codec_gguf_path, bp)) {",
        "    BackendPair codec_bp = bp;\n    if (std::strncmp(ggml_backend_name(bp.backend), \"HTP\", 3) == 0) {\n        codec_bp.backend = bp.cpu_backend;\n        codec_bp.has_gpu = false;\n    }\n    if (!pipeline_codec_load(&pt->codec, codec_gguf_path, codec_bp)) {",
    )?;
    let text = replace(
        &text,
        "    if (!kv_cache_init(&pt->code_predictor_kv, pt->code_predictor.num_hidden_layers,\n                       pt->code_predictor.num_key_value_heads, pt->code_predictor.head_dim, pt->num_code_groups,\n                       pt->max_batch, pt->backend)) {",
        "    if (!kv_cache_init(&pt->code_predictor_kv, pt->code_predictor.num_hidden_layers,\n                       pt->code_predictor.num_key_value_heads, pt->code_predictor.head_dim, pt->num_code_groups,\n                       pt->max_batch, pt->bp.cpu_backend)) {",
    )?;
    let text = replace(
        &text,
        "            pt->bridge_buf = ggml_backend_alloc_ctx_tensors(pt->bridge_ctx, pt->backend);",
        "            pt->bridge_buf = ggml_backend_alloc_ctx_tensors(pt->bridge_ctx, pt->bp.cpu_backend);",
    )?;
    let text = replace(
        &text,
        "        if (!speaker_encoder_weights_load(&pt->speaker_encoder, pt->gguf_talker, pt->backend) ||",
        "        if (!speaker_encoder_weights_load(&pt->speaker_encoder, pt->gguf_talker, pt->bp.cpu_backend) ||",
    )?;
    let text = replace(
        &text,
        "        if (!speaker_encoder_extract(&pt->speaker_encoder, pt->sched, params->ref_audio_24k, params->ref_n_samples,",
        "        if (!speaker_encoder_extract(&pt->speaker_encoder, pt->codec.sched, params->ref_audio_24k, params->ref_n_samples,",
    )?;
    let text = replace(
        &text,
        "            const int chunk_frames    = pt->codec_chunk_frames;\n            const int left_ctx_frames = pt->codec_left_ctx_frames;\n\n            const int T_frames = (int) s.all_codes.size();\n            int       seed     = 0;\n            if (s.ref_codes_ptr != NULL) {\n                seed = s.ref_codes_T < left_ctx_frames ? s.ref_codes_T : left_ctx_frames;\n            }\n            const int            T_dec = seed + T_frames;\n            std::vector<int32_t> codes_kt((size_t) num_codebooks * (size_t) T_dec);\n            for (int k = 0; k < num_codebooks; k++) {\n                int32_t * row = codes_kt.data() + (size_t) k * (size_t) T_dec;\n                if (seed > 0) {\n                    std::memcpy(row,\n                                s.ref_codes_ptr + (size_t) k * (size_t) s.ref_codes_T + (size_t) (s.ref_codes_T - seed),\n                                (size_t) seed * sizeof(int32_t));\n                }\n                for (int t = 0; t < T_frames; t++) {\n                    row[(size_t) (seed + t)] = s.all_codes[(size_t) t][(size_t) k];\n                }\n            }\n            Timer              t_codec;\n            std::vector<float> audio =\n                codec_chunked_decode(&pt->codec, codes_kt.data(), num_codebooks, T_dec, chunk_frames, left_ctx_frames);\n            s.perf.codec_ms += t_codec.ms();\n            if (audio.empty()) {\n                qt_set_error(\"tts_slot_complete: codec decode returned no audio\");\n                qt_log(QT_LOG_ERROR, \"[Pipeline] codec decode returned no audio\");\n                st = QT_STATUS_GENERATE_FAILED;\n            } else {\n                if (seed > 0) {\n                    audio.erase(audio.begin(), audio.begin() + (size_t) seed * (size_t) TOKENIZER_HOP_LENGTH);\n                }",
        "            const int       T_frames = (int) s.all_codes.size();\n            PipelineCodec * pc       = &pt->codec;\n            const int       staging  = pt->max_batch;\n            Timer           t_codec;\n            bool            stream_ok = true;\n            if (s.ref_codes_ptr != NULL) {\n                const uint64_t key = pipeline_codec_ref_key(s.ref_codes_ptr, num_codebooks, s.ref_codes_T);\n                if (!pipeline_codec_stream_restore(pc, key, staging)) {\n                    stream_ok = pipeline_codec_stream_reset(pc, staging);\n                    std::vector<int32_t> ref_chunk((size_t) (1 << (CODEC_STREAM_CLASSES - 1)) * (size_t) num_codebooks);\n                    int                  t0 = 0;\n                    while (stream_ok && t0 < s.ref_codes_T) {\n                        int T = 1 << (CODEC_STREAM_CLASSES - 1);\n                        while (T > s.ref_codes_T - t0) {\n                            T >>= 1;\n                        }\n                        for (int k = 0; k < num_codebooks; k++) {\n                            for (int t = 0; t < T; t++) {\n                                ref_chunk[(size_t) k * (size_t) T + (size_t) t] =\n                                    s.ref_codes_ptr[(size_t) k * (size_t) s.ref_codes_T + (size_t) (t0 + t)];\n                            }\n                        }\n                        stream_ok = pipeline_codec_decode_stream(pc, ref_chunk.data(), T, NULL);\n                        t0 += T;\n                    }\n                    if (stream_ok) {\n                        stream_ok = pipeline_codec_stream_snapshot(pc, key, staging);\n                    }\n                }\n            } else {\n                stream_ok = pipeline_codec_stream_reset(pc, staging);\n            }\n            std::vector<float> audio;\n            if (stream_ok) {\n                audio.resize((size_t) T_frames * (size_t) TOKENIZER_HOP_LENGTH);\n                std::vector<int32_t> chunk((size_t) (1 << (CODEC_STREAM_CLASSES - 1)) * (size_t) num_codebooks);\n                int                  t0 = 0;\n                while (stream_ok && t0 < T_frames) {\n                    int T = 1 << (CODEC_STREAM_CLASSES - 1);\n                    while (T > T_frames - t0) {\n                        T >>= 1;\n                    }\n                    for (int k = 0; k < num_codebooks; k++) {\n                        for (int t = 0; t < T; t++) {\n                            chunk[(size_t) k * (size_t) T + (size_t) t] = s.all_codes[(size_t) (t0 + t)][(size_t) k];\n                        }\n                    }\n                    stream_ok = pipeline_codec_decode_stream(\n                        pc, chunk.data(), T, audio.data() + (size_t) t0 * (size_t) TOKENIZER_HOP_LENGTH);\n                    t0 += T;\n                }\n                if (!stream_ok) {\n                    audio.clear();\n                }\n            }\n            s.perf.codec_ms += t_codec.ms();\n            if (audio.empty()) {\n                qt_set_error(\"tts_slot_complete: codec decode returned no audio\");\n                qt_log(QT_LOG_ERROR, \"[Pipeline] codec decode returned no audio\");\n                st = QT_STATUS_GENERATE_FAILED;\n            } else {",
    )?;
    let text = replace(
        &text,
        "        for (int i = 0; i < N_dec; i++) {\n            TtsSlot & s = e->slots[(size_t) i];\n            for (int g = 0; g < num_codebooks; g++) {\n                ids[(size_t) g * (size_t) N_dec + (size_t) i] = s.prev_ids[(size_t) g];\n            }\n            std::memcpy(overlays.data() + (size_t) i * (size_t) hidden, s.prev_overlay,\n                        (size_t) hidden * sizeof(float));\n        }",
        "        std::vector<float> tmp_emb((size_t) hidden);\n        for (int i = 0; i < N_dec; i++) {\n            TtsSlot & s   = e->slots[(size_t) i];\n            float *   dst = overlays.data() + (size_t) i * (size_t) hidden;\n            std::memcpy(dst, s.prev_overlay, (size_t) hidden * sizeof(float));\n            embed_row_from_gguf(pt->gguf_talker, \"talker.codec_embd.weight\", s.prev_ids[0], hidden, tmp_emb.data());\n            for (int j = 0; j < hidden; j++) {\n                dst[j] += tmp_emb[(size_t) j];\n            }\n            for (int g = 0; g < n_acoustic; g++) {\n                char name[64];\n                snprintf(name, sizeof(name), \"code_pred.codec_embd.%d.weight\", g);\n                embed_row_from_gguf(pt->gguf_talker, name, s.prev_ids[(size_t) (g + 1)], hidden, tmp_emb.data());\n                for (int j = 0; j < hidden; j++) {\n                    dst[j] += tmp_emb[(size_t) j];\n                }\n            }\n        }",
    )?;
    replace(
        &text,
        "            bool pred_ok = code_predictor_frame_step(&pt->code_predictor, pt->backend, &frame, &gs.sampler, c0s.data(),\n                                                     slots.data(), N, cp_dump, &cp);",
        "            bool pred_ok = code_predictor_frame_step(&pt->code_predictor, pt->bp.cpu_backend, &frame, &gs.sampler,\n                                                     c0s.data(), slots.data(), N, cp_dump, &cp);",
    )
}

fn qwen_cpp(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "        if (!speaker_encoder_weights_load(&q->pt.speaker_encoder, q->pt.gguf_talker, q->pt.backend) ||",
        "        if (!speaker_encoder_weights_load(&q->pt.speaker_encoder, q->pt.gguf_talker, q->pt.bp.cpu_backend) ||",
    )?;
    let text = replace(
        &text,
        "    if (!speaker_encoder_extract(&q->pt.speaker_encoder, q->pt.sched, job->audio, job->n_samples, emb)) {",
        "    if (!speaker_encoder_extract(&q->pt.speaker_encoder, q->pt.codec.sched, job->audio, job->n_samples, emb)) {",
    )?;
    replace(
        &text,
        "        if (codes_T != ref_T) {\n            qt_set_error(\"qt_extract_voice_ref: encoded frame count %d mismatches aligned frame count %d\", codes_T,\n                         ref_T);\n            return QT_STATUS_GENERATE_FAILED;\n        }",
        "        if (codes_T != ref_T) {\n            qt_set_error(\"qt_extract_voice_ref: encoded frame count %d mismatches aligned frame count %d\", codes_T,\n                         ref_T);\n            return QT_STATUS_GENERATE_FAILED;\n        }\n        {\n            PipelineCodec * pc      = &q->pt.codec;\n            const int       staging = q->pt.max_batch;\n            const uint64_t  key     = pipeline_codec_ref_key(codes.data(), num_codebooks, ref_T);\n            if (!pipeline_codec_stream_restore(pc, key, staging) && pipeline_codec_stream_reset(pc, staging)) {\n                std::vector<int32_t> chunk((size_t) (1 << (CODEC_STREAM_CLASSES - 1)) * (size_t) num_codebooks);\n                int                  t0       = 0;\n                bool                 prime_ok = true;\n                while (prime_ok && t0 < ref_T) {\n                    int T = 1 << (CODEC_STREAM_CLASSES - 1);\n                    while (T > ref_T - t0) {\n                        T >>= 1;\n                    }\n                    for (int k = 0; k < num_codebooks; k++) {\n                        for (int t = 0; t < T; t++) {\n                            chunk[(size_t) k * (size_t) T + (size_t) t] =\n                                codes[(size_t) k * (size_t) ref_T + (size_t) (t0 + t)];\n                        }\n                    }\n                    prime_ok = pipeline_codec_decode_stream(pc, chunk.data(), T, NULL);\n                    t0 += T;\n                }\n                if (prime_ok) {\n                    (void) pipeline_codec_stream_snapshot(pc, key, staging);\n                }\n            }\n        }",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        backend_header, code_predictor_weights, ggml_backend, ggml_backend_meta, ggml_cpu,
        ggml_header, ggml_source, kv_cache, pipeline_tts, prompt_builder, qwen_cpp, talker_forward,
        talker_weights,
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
            code_predictor_weights,
            talker_forward,
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
