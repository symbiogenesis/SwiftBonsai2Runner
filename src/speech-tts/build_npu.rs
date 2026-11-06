//! Exact-source overlays that align Qwen3-TTS with the signed Hexagon NPU (`HTP0`) runtime.

use std::{fs, io, path::Path};

type Transform = fn(&str) -> io::Result<String>;

const TRANSFORMS: [(&str, Transform); 14] = [
    ("ggml/include/ggml.h", ggml_header),
    ("ggml/src/ggml.c", ggml_source),
    ("ggml/src/ggml-cpu/ggml-cpu.c", ggml_cpu),
    ("ggml/src/ggml-backend-meta.cpp", ggml_backend_meta),
    ("ggml/src/ggml-backend.cpp", ggml_backend),
    ("src/backend.h", backend_header),
    ("src/gguf-weights.h", gguf_weights),
    ("src/kv-cache.h", kv_cache),
    ("src/talker-weights.h", talker_weights),
    ("src/pipeline-tts.h", pipeline_tts_header),
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

fn gguf_weights(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "            gf_close(gf);\n            return false;\n        }\n    }",
        "            gf_close(gf);\n            return false;\n        }\n        t->data = gf->mapping + gf->data_offset + toff;\n    }",
    )?;
    replace(
        &text,
        "static const void * gf_get_data(const GGUFModel & gf, const char * name) {\n    int64_t idx = gguf_find_tensor(gf.gguf, name);\n    if (idx < 0) {\n        return NULL;\n    }\n    size_t offset = gguf_get_tensor_offset(gf.gguf, idx);\n    return gf.mapping + gf.data_offset + offset;\n}",
        "static const void * gf_get_data(const GGUFModel & gf, const char * name) {\n    struct ggml_tensor * src = ggml_get_tensor(gf.meta, name);\n    return src ? src->data : NULL;\n}",
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
    let text = replace(
        text,
        "    tw->codec_embedding = gf_load_tensor(&wctx, gf, \"talker.codec_embd.weight\");\n    tw->text_embedding  = gf_load_tensor(&wctx, gf, \"talker.text_embd.weight\");\n    tw->text_proj_fc1_w = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc1.weight\");\n    tw->text_proj_fc1_b = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc1.bias\");\n    tw->text_proj_fc2_w = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc2.weight\");\n    tw->text_proj_fc2_b = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc2.bias\");",
        "    tw->codec_embedding = ggml_get_tensor(gf.meta, \"talker.codec_embd.weight\");\n    tw->text_embedding  = ggml_get_tensor(gf.meta, \"talker.text_embd.weight\");\n    tw->text_proj_fc1_w = ggml_get_tensor(gf.meta, \"talker.text_proj.fc1.weight\");\n    tw->text_proj_fc1_b = ggml_get_tensor(gf.meta, \"talker.text_proj.fc1.bias\");\n    tw->text_proj_fc2_w = ggml_get_tensor(gf.meta, \"talker.text_proj.fc2.weight\");\n    tw->text_proj_fc2_b = ggml_get_tensor(gf.meta, \"talker.text_proj.fc2.bias\");",
    )?;
    replace(
        &text,
        "    tw->layers.clear();\n}",
        "    tw->layers.clear();\n}\n\nstatic void talker_weights_bind_cpu(TalkerWeights * tw_cpu, const TalkerWeights & tw_src, const GGUFModel & gf) {\n    *tw_cpu                 = tw_src;\n    tw_cpu->weight_ctx      = NULL;\n    tw_cpu->weight_buf      = NULL;\n    tw_cpu->codec_embedding = ggml_get_tensor(gf.meta, \"talker.codec_embd.weight\");\n    tw_cpu->text_embedding  = ggml_get_tensor(gf.meta, \"talker.text_embd.weight\");\n    tw_cpu->text_proj_fc1_w = ggml_get_tensor(gf.meta, \"talker.text_proj.fc1.weight\");\n    tw_cpu->text_proj_fc1_b = ggml_get_tensor(gf.meta, \"talker.text_proj.fc1.bias\");\n    tw_cpu->text_proj_fc2_w = ggml_get_tensor(gf.meta, \"talker.text_proj.fc2.weight\");\n    tw_cpu->text_proj_fc2_b = ggml_get_tensor(gf.meta, \"talker.text_proj.fc2.bias\");\n    tw_cpu->codec_head_w    = ggml_get_tensor(gf.meta, \"talker.codec_head.weight\");\n    tw_cpu->norm_w          = ggml_get_tensor(gf.meta, \"talker.output_norm.weight\");\n    tw_cpu->layers.resize((size_t) tw_src.num_hidden_layers);\n    for (int l = 0; l < tw_src.num_hidden_layers; l++) {\n        TalkerLayer & layer = tw_cpu->layers[(size_t) l];\n        char          name[160];\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_norm.weight\", l);\n        layer.input_norm_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.ffn_norm.weight\", l);\n        layer.post_attn_norm_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_q.weight\", l);\n        layer.attn.q_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_k.weight\", l);\n        layer.attn.k_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_v.weight\", l);\n        layer.attn.v_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_output.weight\", l);\n        layer.attn.o_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_q_norm.weight\", l);\n        layer.attn.q_norm_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.attn_k_norm.weight\", l);\n        layer.attn.k_norm_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.ffn_gate.weight\", l);\n        layer.mlp.gate_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.ffn_up.weight\", l);\n        layer.mlp.up_proj_w = ggml_get_tensor(gf.meta, name);\n        snprintf(name, sizeof(name), \"talker.blk.%d.ffn_down.weight\", l);\n        layer.mlp.down_proj_w = ggml_get_tensor(gf.meta, name);\n    }\n}",
    )
}

fn pipeline_tts_header(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    std::vector<PromptPrefixCacheEntry> prefix_entries;\n    size_t                              max_prefix_entries;\n};",
        "    std::vector<PromptPrefixCacheEntry> prefix_entries;\n    size_t                              max_prefix_entries;\n    uint64_t                            icl_codec_key = 0;\n    int                                 icl_codec_T   = 0;\n    std::vector<float>                  icl_codec_stream;\n    std::string                         icl_ref_text;\n    int                                 icl_N_ref_text = 0;\n    std::vector<float>                  icl_ref_text_stream;\n};",
    )?;
    replace(
        &text,
        "struct PipelineTTS {\n    GGUFModel             gguf_talker;\n    TalkerWeights         talker;",
        "struct PipelineTTS {\n    GGUFModel             gguf_talker;\n    TalkerWeights         talker;\n    TalkerWeights         talker_cpu;",
    )
}

fn talker_forward(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    if (record_taps || read_hidden_host) {\n        ggml_set_output(h_final);\n    }\n\n    // Bridge: the last position hidden copies on device into column\n    // `kv_set` of the persistent [hidden, max_batch] tensor. Constant\n    // destination address across steps, so the decode graph topology\n    // stays replayable.\n    struct ggml_tensor * h_last =\n        ggml_view_1d(gctx, h_final, hidden, (size_t) (T - 1) * (size_t) hidden * sizeof(float));\n    struct ggml_tensor * bridge_col = ggml_view_1d(gctx, hidden_bridge, hidden, (size_t) kv_set * hidden_bridge->nb[1]);\n    struct ggml_tensor * bridge_cpy = ggml_cpy(gctx, h_last, bridge_col);\n\n    // codec_head: [hidden, vocab]. ggml_mul_mat returns [vocab, T].\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n\n    if (record_taps) {\n        for (int i = 0; i < TALKER_N_BISECT_LAYERS; i++) {\n            if (taps[(size_t) i]) {\n                ggml_build_forward_expand(gf, taps[(size_t) i]);\n            }\n        }\n        ggml_build_forward_expand(gf, h_final);\n    }\n    ggml_build_forward_expand(gf, logits);\n    ggml_build_forward_expand(gf, bridge_cpy);",
        "    ggml_set_output(h_final);\n    (void) hidden_bridge;\n\n    // codec_head: [hidden, vocab]. ggml_mul_mat returns [vocab, T].\n    struct ggml_tensor * logits = ggml_mul_mat(gctx, tw->codec_head_w, h_final);\n    ggml_set_name(logits, \"logits\");\n\n    if (record_taps) {\n        for (int i = 0; i < TALKER_N_BISECT_LAYERS; i++) {\n            if (taps[(size_t) i]) {\n                ggml_build_forward_expand(gf, taps[(size_t) i]);\n            }\n        }\n    }\n    ggml_build_forward_expand(gf, h_final);\n    ggml_build_forward_expand(gf, logits);",
    )?;
    replace(
        &text,
        "    {\n        size_t row_bytes = (size_t) vocab * sizeof(float);\n        ggml_backend_tensor_get(logits, out->logits_last.data(), (size_t) (T - 1) * row_bytes, row_bytes);\n    }",
        "    {\n        size_t row_bytes = (size_t) vocab * sizeof(float);\n        ggml_backend_tensor_get(logits, out->logits_last.data(), (size_t) (T - 1) * row_bytes, row_bytes);\n        size_t hrow_bytes = (size_t) hidden * sizeof(float);\n        ggml_backend_tensor_get(h_final, (char *) hidden_bridge->data + (size_t) kv_set * hidden_bridge->nb[1],\n                                (size_t) (T - 1) * hrow_bytes, hrow_bytes);\n    }",
    )
}

fn prompt_builder(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "    ggml_backend_sched_reset(pt->sched);\n    if (!ggml_backend_sched_alloc_graph(pt->sched, graph)) {\n        ggml_backend_sched_reset(pt->sched);\n        ggml_free(gctx);\n        return false;\n    }\n\n    ggml_backend_tensor_set(ids_in, ids, 0, (size_t) count * sizeof(int32_t));\n    if (ggml_backend_sched_graph_compute(pt->sched, graph) != GGML_STATUS_SUCCESS) {\n        ggml_backend_sched_reset(pt->sched);\n        ggml_free(gctx);\n        return false;\n    }\n\n    ggml_backend_tensor_get(out, dst, 0, (size_t) count * (size_t) hidden * sizeof(float));\n    ggml_backend_sched_reset(pt->sched);\n    ggml_free(gctx);",
        "    ggml_gallocr_t ga = ggml_gallocr_new(ggml_backend_get_default_buffer_type(pt->bp.cpu_backend));\n    if (!ga || !ggml_gallocr_alloc_graph(ga, graph)) {\n        if (ga) {\n            ggml_gallocr_free(ga);\n        }\n        ggml_free(gctx);\n        return false;\n    }\n\n    ggml_backend_tensor_set(ids_in, ids, 0, (size_t) count * sizeof(int32_t));\n    if (ggml_backend_graph_compute(pt->bp.cpu_backend, graph) != GGML_STATUS_SUCCESS) {\n        ggml_gallocr_free(ga);\n        ggml_free(gctx);\n        return false;\n    }\n\n    ggml_backend_tensor_get(out, dst, 0, (size_t) count * (size_t) hidden * sizeof(float));\n    ggml_gallocr_free(ga);\n    ggml_free(gctx);",
    )?;
    let text = replace(
        &text,
        "    ggml_backend_sched_reset(pt->sched);\n    if (!ggml_backend_sched_alloc_graph(pt->sched, graph)) {\n        ggml_backend_sched_reset(pt->sched);\n        ggml_free(gctx);\n        qt_throw(\"[Prompt] ref codes graph alloc failed\");\n    }\n\n    ggml_backend_tensor_set(ids_all, ref_codes, 0, (size_t) groups * (size_t) ref_codes_T * sizeof(int32_t));\n    if (ggml_backend_sched_graph_compute(pt->sched, graph) != GGML_STATUS_SUCCESS) {\n        ggml_backend_sched_reset(pt->sched);\n        ggml_free(gctx);\n        qt_throw(\"[Prompt] ref codes graph compute failed\");\n    }\n\n    ggml_backend_tensor_get(sum, dst, 0, (size_t) ref_codes_T * (size_t) hidden * sizeof(float));\n    ggml_backend_sched_reset(pt->sched);\n    ggml_free(gctx);",
        "    ggml_gallocr_t ga = ggml_gallocr_new(ggml_backend_get_default_buffer_type(pt->bp.cpu_backend));\n    if (!ga || !ggml_gallocr_alloc_graph(ga, graph)) {\n        if (ga) {\n            ggml_gallocr_free(ga);\n        }\n        ggml_free(gctx);\n        qt_throw(\"[Prompt] ref codes graph alloc failed\");\n    }\n\n    ggml_backend_tensor_set(ids_all, ref_codes, 0, (size_t) groups * (size_t) ref_codes_T * sizeof(int32_t));\n    if (ggml_backend_graph_compute(pt->bp.cpu_backend, graph) != GGML_STATUS_SUCCESS) {\n        ggml_gallocr_free(ga);\n        ggml_free(gctx);\n        qt_throw(\"[Prompt] ref codes graph compute failed\");\n    }\n\n    ggml_backend_tensor_get(sum, dst, 0, (size_t) ref_codes_T * (size_t) hidden * sizeof(float));\n    ggml_gallocr_free(ga);\n    ggml_free(gctx);",
    )?;
    let text = replace(
        &text,
        "static std::string prompt_prefix_cache_key(const std::vector<int> & instruct_ids,\n                                           const int *              role_ids,\n                                           const std::vector<int> & codec_left) {\n    std::string key;\n    key.reserve(128 + instruct_ids.size() * 8 + codec_left.size() * 8);\n    append_ints_to_key(key, \"instruct\", instruct_ids);\n    std::vector<int> role(role_ids, role_ids + 3);\n    append_ints_to_key(key, \"role\", role);\n    append_ints_to_key(key, \"codec\", codec_left);\n    return key;\n}",
        "static std::string prompt_prefix_cache_key(const std::vector<int> & instruct_ids,\n                                           const int *              role_ids,\n                                           const std::vector<int> & codec_left,\n                                           const float *            ref_spk_emb,\n                                           int                      hidden) {\n    std::string key;\n    key.reserve(160 + instruct_ids.size() * 8 + codec_left.size() * 8);\n    append_ints_to_key(key, \"instruct\", instruct_ids);\n    std::vector<int> role(role_ids, role_ids + 3);\n    append_ints_to_key(key, \"role\", role);\n    append_ints_to_key(key, \"codec\", codec_left);\n    if (ref_spk_emb != NULL && hidden > 0) {\n        uint64_t        h     = 1469598103934665603ULL;\n        const uint8_t * bytes = (const uint8_t *) ref_spk_emb;\n        const size_t    n     = (size_t) hidden * sizeof(float);\n        for (size_t i = 0; i < n; i++) {\n            h ^= (uint64_t) bytes[i];\n            h *= 1099511628211ULL;\n        }\n        key += \"spk=\";\n        key += std::to_string(h);\n        key += ';';\n    }\n    return key;\n}",
    )?;
    let text = replace(
        &text,
        "    std::vector<int> ref_ids;\n    int              N_ref_text = 0;\n    if (icl) {\n        std::string ref_full;\n        ref_full.reserve(ref_text.size() + 64);\n        ref_full = \"<|im_start|>assistant\\n\";\n        ref_full += ref_text;\n        ref_full += \"<|im_end|>\\n<|im_start|>assistant\\n\";\n        ref_ids = bpe_encode(tok, ref_full, /*add_eos=*/false);\n        if ((int) ref_ids.size() < 8) {\n            qt_log(QT_LOG_ERROR, \"[Prompt] FATAL: ref_text tokenized too short (%d tokens)\", (int) ref_ids.size());\n            return false;\n        }\n        // ref_ids[3: -5] is the inner ref text body without role tokens\n        N_ref_text = (int) ref_ids.size() - 3 - 5;\n        if (N_ref_text <= 0) {\n            qt_log(QT_LOG_ERROR, \"[Prompt] FATAL: empty ref_text body\");\n            return false;\n        }\n    }",
        "    std::vector<int> ref_ids;\n    int              N_ref_text = 0;\n    if (icl) {\n        if (pc.icl_ref_text == ref_text && pc.icl_N_ref_text > 0 &&\n            pc.icl_ref_text_stream.size() == (size_t) pc.icl_N_ref_text * (size_t) hidden) {\n            N_ref_text = pc.icl_N_ref_text;\n        } else {\n            std::string ref_full;\n            ref_full.reserve(ref_text.size() + 64);\n            ref_full = \"<|im_start|>assistant\\n\";\n            ref_full += ref_text;\n            ref_full += \"<|im_end|>\\n<|im_start|>assistant\\n\";\n            ref_ids = bpe_encode(tok, ref_full, /*add_eos=*/false);\n            if ((int) ref_ids.size() < 8) {\n                qt_log(QT_LOG_ERROR, \"[Prompt] FATAL: ref_text tokenized too short (%d tokens)\", (int) ref_ids.size());\n                return false;\n            }\n            // ref_ids[3: -5] is the inner ref text body without role tokens\n            N_ref_text = (int) ref_ids.size() - 3 - 5;\n            if (N_ref_text <= 0) {\n                qt_log(QT_LOG_ERROR, \"[Prompt] FATAL: empty ref_text body\");\n                return false;\n            }\n        }\n    }",
    )?;
    let text = replace(
        &text,
        "    const int                prefix_rows      = N_instruct + 3 + (int) codec_left.size();\n    const bool               cacheable_prefix = !icl && ref_spk_emb == NULL;\n    std::string              prefix_key;\n    PromptPrefixCacheEntry * prefix_hit = NULL;\n    if (cacheable_prefix) {\n        prefix_key = prompt_prefix_cache_key(instruct_ids, ids.data(), codec_left);\n        prefix_hit = prompt_prefix_cache_find(pc, prefix_key);\n    }",
        "    const int                prefix_rows      = N_instruct + 3 + (int) codec_left.size();\n    const bool               cacheable_prefix = true;\n    std::string              prefix_key;\n    PromptPrefixCacheEntry * prefix_hit = NULL;\n    if (cacheable_prefix) {\n        prefix_key = prompt_prefix_cache_key(instruct_ids, ids.data(), codec_left, ref_spk_emb, hidden);\n        prefix_hit = prompt_prefix_cache_find(pc, prefix_key);\n    }",
    )?;
    replace(
        &text,
        "        std::vector<float> codec_stream((size_t) T_icl * (size_t) hidden, 0.0f);\n        std::memcpy(codec_stream.data(), codec_bos_emb.data(), (size_t) hidden * sizeof(float));\n        project_ref_codes_backend(pt, ref_codes, ref_codes_T, codec_stream.data() + (size_t) hidden);\n\n        // Build the text stream [text_lens_icl, hidden] = text_proj of\n        // [ref_text ; utterance_text] then tts_eos. The ref and utterance\n        // bodies are contiguous, projected in one backend pass.\n        std::vector<float> text_stream((size_t) text_lens_icl * (size_t) hidden, 0.0f);\n        {\n            std::vector<int32_t> text_ids;\n            text_ids.reserve((size_t) N_ref_text + (size_t) N_text);\n            for (int i = 0; i < N_ref_text; i++) {\n                text_ids.push_back((int32_t) ref_ids[3 + i]);\n            }\n            for (int i = 0; i < N_text; i++) {\n                text_ids.push_back((int32_t) ids[3 + i]);\n            }\n            project_text_ids(pt, text_ids, text_stream.data());\n        }",
        "        const uint64_t ref_key = pipeline_codec_ref_key(ref_codes, pt->num_code_groups, ref_codes_T);\n        if (pc.icl_codec_key != ref_key || pc.icl_codec_T != ref_codes_T ||\n            pc.icl_codec_stream.size() != (size_t) T_icl * (size_t) hidden) {\n            pc.icl_codec_stream.assign((size_t) T_icl * (size_t) hidden, 0.0f);\n            std::memcpy(pc.icl_codec_stream.data(), codec_bos_emb.data(), (size_t) hidden * sizeof(float));\n            project_ref_codes_backend(pt, ref_codes, ref_codes_T, pc.icl_codec_stream.data() + (size_t) hidden);\n            pc.icl_codec_key = ref_key;\n            pc.icl_codec_T   = ref_codes_T;\n        }\n        const std::vector<float> & codec_stream = pc.icl_codec_stream;\n\n        std::vector<float> text_stream((size_t) text_lens_icl * (size_t) hidden, 0.0f);\n        if (!ref_ids.empty()) {\n            pc.icl_ref_text_stream.assign((size_t) N_ref_text * (size_t) hidden, 0.0f);\n            project_text_range(pt, ref_ids.data(), 3, 3 + N_ref_text, pc.icl_ref_text_stream.data());\n            pc.icl_ref_text   = ref_text;\n            pc.icl_N_ref_text = N_ref_text;\n        }\n        std::memcpy(text_stream.data(), pc.icl_ref_text_stream.data(),\n                    (size_t) N_ref_text * (size_t) hidden * sizeof(float));\n        project_text_range(pt, ids.data(), 3, 3 + N_text,\n                           text_stream.data() + (size_t) N_ref_text * (size_t) hidden);",
    )
}

fn pipeline_tts(text: &str) -> io::Result<String> {
    let text = replace(
        text,
        "            s.sampler_buf = ggml_backend_alloc_ctx_tensors(s.sampler_ctx, pt->backend);\n        }\n        if (!s.sampler_ctx || !s.sampler_buf) {\n            qt_log(QT_LOG_ERROR, \"[Pipeline] sampler state allocation failed (N=%d)\", N);\n            if (s.sampler_ctx) {\n                ggml_free(s.sampler_ctx);\n                s.sampler_ctx = NULL;\n            }\n            return false;\n        }\n        ggml_backend_buffer_clear(s.sampler_buf, 0);\n    }\n\n    return code_predictor_frame_graph_build(&pt->code_predictor, &pt->code_predictor_kv, pt->backend,\n                                            pt->talker.codec_embedding, pt->hidden_bridge, &s.sampler, tail, N,\n                                            pt->use_flash_attn, pt->clamp_fp16, &frame);",
        "            s.sampler_buf = ggml_backend_alloc_ctx_tensors(s.sampler_ctx, pt->bp.cpu_backend);\n        }\n        if (!s.sampler_ctx || !s.sampler_buf) {\n            qt_log(QT_LOG_ERROR, \"[Pipeline] sampler state allocation failed (N=%d)\", N);\n            if (s.sampler_ctx) {\n                ggml_free(s.sampler_ctx);\n                s.sampler_ctx = NULL;\n            }\n            return false;\n        }\n        ggml_backend_buffer_clear(s.sampler_buf, 0);\n    }\n\n    return code_predictor_frame_graph_build(&pt->code_predictor, &pt->code_predictor_kv, pt->bp.cpu_backend,\n                                            pt->talker_cpu.codec_embedding, pt->hidden_bridge, &s.sampler, tail, N,\n                                            true, pt->clamp_fp16, &frame);",
    )?;
    let text = replace(
        &text,
        "    if (!talker_weights_load(&pt->talker, pt->gguf_talker, pt->backend)) {\n        gf_close(&pt->gguf_talker);\n        return false;\n    }",
        "    if (!talker_weights_load(&pt->talker, pt->gguf_talker, pt->backend)) {\n        gf_close(&pt->gguf_talker);\n        return false;\n    }\n    talker_weights_bind_cpu(&pt->talker_cpu, pt->talker, pt->gguf_talker);",
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
        "    code_predictor_weights_free(&pt->code_predictor);\n    talker_weights_free(&pt->talker);",
        "    code_predictor_weights_free(&pt->code_predictor);\n    talker_weights_free(&pt->talker_cpu);\n    talker_weights_free(&pt->talker);",
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
        "        if (!talker_forward_decode(&pt->talker, &pt->talker_kv, pt->backend, pt->talker_decode_graphs,\n                                   pt->hidden_bridge, ids.data(), pt->code_predictor.codec_embedding.data(), n_acoustic,\n                                   overlays.data(), N_dec, pt->use_flash_attn, pt->clamp_fp16, any_dump, &fw)) {",
        "        if (!talker_forward_decode(&pt->talker_cpu, &pt->talker_kv, pt->bp.cpu_backend, pt->talker_decode_graphs,\n                                   pt->hidden_bridge, ids.data(), pt->code_predictor.codec_embedding.data(), n_acoustic,\n                                   overlays.data(), N_dec, true, pt->clamp_fp16, any_dump, &fw)) {",
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
        backend_header, ggml_backend, ggml_backend_meta, ggml_cpu, ggml_header, ggml_source,
        gguf_weights, kv_cache, pipeline_tts, pipeline_tts_header, prompt_builder, qwen_cpp,
        talker_forward, talker_weights,
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
            gguf_weights,
            kv_cache,
            talker_weights,
            pipeline_tts_header,
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
            "    tw->codec_embedding = gf_load_tensor(&wctx, gf, \"talker.codec_embd.weight\");\n    tw->text_embedding  = gf_load_tensor(&wctx, gf, \"talker.text_embd.weight\");\n    tw->text_proj_fc1_w = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc1.weight\");\n    tw->text_proj_fc1_b = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc1.bias\");\n    tw->text_proj_fc2_w = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc2.weight\");\n    tw->text_proj_fc2_b = gf_load_tensor(&wctx, gf, \"talker.text_proj.fc2.bias\");\n    tw->layers.clear();\n}",
        )?;
        assert!(tw.contains("ggml_get_tensor(gf.meta, \"talker.text_embd.weight\")"));
        assert!(tw.contains("talker_weights_bind_cpu"));
        assert!(talker_weights(&tw).is_err());
        Ok(())
    }
}
