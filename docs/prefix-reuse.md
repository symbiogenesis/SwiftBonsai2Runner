# Experimental exact latest-prefix reuse

`bonsai-tools chat --prompt-reuse latest` explicitly selects Prism's latest
committed live-prefix policy. The PowerShell equivalent is
`Start-Chat.ps1 -PromptReuse latest`. Omission and `--prompt-reuse disabled`
retain the current reset-every-request policy. The publisher runtime cannot
select `latest`.

The selection survives deferred preparation, native chat launch, broker launch
and explicit Retry. The broker forwards only the fixed `--prompt-reuse latest`
argument to its verified contained model worker. Capability is limited to the
reviewed Prism 0.1.14 source
`9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57`, whose immutable release files are
already pinned. A version number, unreviewed repin or claimed readiness alone
does not grant capability. Authenticated model readiness must report exactly
`"prompt_reuse":"latest"` when selected and omit it when disabled. Missing,
unexpected, duplicate or malformed identities fail startup without fallback.

Reuse requires an exact token-prefix match in the same model session at a
bit-identical adapter scale. It retains only current committed state and its
logits; it adds no saved-prefix snapshots. New, shortened, switched or edited
histories that do not extend that complete prefix recompute from empty state.
Scale changes, including signed zero and change-back, invalidate prior state.
Cancellation or provisional-output failure restores the request transaction;
the private service still closes admission until an explicit successful reset.
Reset clears the count seal and model prefix state and reseeds the sampler.
Changing reuse policy clears model state while preserving sampler progression.

The native UI, artifact verification, sandbox, context/output reservations and
memory admission are unchanged. Per-request `cache_prompt` and other cache
overrides remain unsupported. Reuse can reduce repeated-history prefill; it does
not itself increase ongoing token generation speed or help unrelated prompts.

Focused model-free controls cover CLI/schema rejection, deferred selection,
Retry preservation, native-only routing, exact source capability and readiness
agreement. Existing Prism graph controls compare complete logits/state and
sampler progression against fresh recomputation across edits, switches, scale
changes, cancellation, page boundaries and reset; the added private-service
control covers acknowledged requests, count purity, failed output, closed
admission and reset recovery with latest reuse selected. Owning hosted checks,
actual-model service acceptance and matched latency/resource evidence remain
pending. No performance benefit or default change is claimed.
