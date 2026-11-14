# Direct LoRA weight edit and factual smoke

On November 13, 2026, a separate Abliteration adapter candidate was created by
editing its existing F32 A weights. The native sparse profile retained **1,507,328
fewer factor-storage bytes (1.4375 MiB)**. Both original and edited adapters
answered all four facts correctly and stopped. **Speed comparison is ineligible:
AC power disconnected during the original arm.** Broad accuracy and preservation
of the adapter's intended behavior remain unqualified. No application profile,
runtime pin, approved adapter or packed base was changed.

## Actual weight changes

The exact approved source was authenticated before editing. At every one of its
128 linear sites, retain the largest half of A coefficients by absolute value,
breaking ties by original index; set the others to zero. This changed **753,664
of 1,507,328** linear A coefficients. All 129 target sites, B factor bits,
embedding factors, alpha, metadata, surviving coefficient bits and file layout
were retained. This is a direct adapter edit, not an omitted adapter or a second
additive branch. Both arms use scale 1.3 (`3fa66666`).

The candidate was frozen before factual evaluation. Offline inspection found
retained squared A-factor L2 mass of **0.9329341220106656**, with the worst site
at **0.9269230610275194**. Keeping a fixed number of largest-magnitude coefficients
minimizes coefficient L2 error; these numbers are not answer-accuracy bounds.
The fixed offline screen also inspected one-third and one-quarter retention:
squared mass **0.8273325348183349 / 0.7388828602943233**, respectively. Neither
was published or evaluated. No candidate was tuned against factual answers.

The edited GGUF remains **9,682,464 bytes**. Its working artifact is retained at:

```text
C:\Users\symbi\AppData\Local\SwiftBonsai2\research\lora-edits\88b2cc7dccecc1481a410ad9dc5441c1bcefc9ae5669fe66e10eb74fbdac8366\bonsai-abliterate-lora-a50.gguf
```

Candidate SHA-256:
`88b2cc7dccecc1481a410ad9dc5441c1bcefc9ae5669fe66e10eb74fbdac8366`.
Publication was create-new, followed by an independent file digest check. The
original adapter's post-workload digest still matches
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`.
The edit receipt plus candidate digest binds derivation; the runtime candidate
loader authenticates bytes and structure, not derivation by itself.

## Storage and answers

The explicit source-only candidate loader replaces profitable zero-pruned A
vectors with F32 values and U16 indices, releasing dense copies. Its ascending
index F64 accumulation, rounded to F32 before B application, has a distinct graph
identity. The original arm uses its unchanged dense NEON arithmetic.

| Retained factor storage, bytes | Original | Edited |
| --- | ---: | ---: |
| Linear A values | 6029312 | 3014656 |
| Sparse indices | 0 | 1507328 |
| Embedding A | 993280 | 993280 |
| Shared decoded B | 20480 | 20480 |
| Total | 7043072 | 5535744 |

Linear A plus indices shrank 25%; total factor allocation payload shrank about
21.4%. These counts include vector capacity and exclude allocator/container
overhead, scratch, packed base and state. They do not imply comparable whole-model
RAM savings or smaller GGUF downloads. All 128 linear sites used sparse storage.

| Fact | Exact answer in both arms | Prompt/output tokens | Original request ms | Edited request ms |
| --- | --- | ---: | ---: | ---: |
| France's capital | `Paris` | 26 / 1 | 6165.9282 | 5732.3985 |
| `2+2` | `4` | 26 / 1 | 6060.4567 | 5999.2888 |
| Water formula | `H2O` | 30 / 3 | 7739.0989 | 7485.2072 |
| Largest planet | `Jupiter` | 28 / 2 | 6794.3899 | 6795.4326 |

All eight answers passed the exact normalized-answer gate and ended with model
`stop`; truncation cannot pass. Four-request totals were **26.7598737 / 26.0123271
seconds**, load times **5379.7953 / 5163.5672 ms**, helper elapsed times
**32493.0787 / 31471.6054 ms**, and supervised wall times **32548 / 31547 ms**.
These are raw observations, not a speed gain: power changed, order was fixed,
filesystem cache was uncontrolled and only one pair was executed.

| Process memory observation, bytes | Original | Edited |
| --- | ---: | ---: |
| Peak Job commit | 7581810688 | 7580479488 |
| Sampled peak private | 7564361728 | 7562403840 |
| OS lifetime peak working set | 7568330752 | 7567814656 |

The 1,331,200-byte Job-peak difference is an unqualified observation. Root memory
was sampled every second (33/32 samples); transients may be missed and these
counters are not additive or phase-attributed. No energy verdict follows.

Four familiar facts test only a tiny sanity boundary. They cannot establish
general accuracy, instruction following, creative quality, refusal behavior,
intended abliteration strength, long-context behavior or numerical parity with
other execution profiles. The unresolved original SSM authoring-basis gate is
unchanged. No LP behavioral fit or accuracy improvement is claimed.

## Fixed budget, power and source

This was a new edited-weight workload; the previous omission pilot stayed closed.
The single predeclared allowance was **300 seconds**, conservatively estimated
at 245 seconds including candidate creation, setup, both arms, power, retention
and cleanup. Each native helper had an original 110-second deadline; native Job
supervision used 110-second process and 125-second total bounds. Setup began
**23:26:57.723 CST**, and both arms finished by **23:28:02.126 CST** (64.422 seconds).
The original aggregate deadline was **23:31:57.723 CST**. No retries, additional
model arms, push, CI operation or release publication occurred.

Both supervisors returned `completed`, exit 0, after owned-process teardown;
the original owner finished before the edited owner started. Each Job had a
16 GiB commit limit, one active process, a 4 GiB physical reserve and a
12,348,030,976-byte available-memory requirement. Original PQ2, four native CPU
workers, experimental NEON, K4/V4 cache, context 256, seed 42, Thinking off,
no prompt reuse or accelerator; four fixed prompts, eight output tokens each.

`GetSystemPowerStatus` observed battery saver off and charge 87% at every model
boundary. Aggregate AC changed **connected to disconnected**; original arm AC
also changed **connected to disconnected**; edited arm observed disconnected
at both endpoints. The original supervisor independently reports
`power-source-changed`, `Comparable: false`. Thus
**performanceComparisonEligible: false**. A separate retention observation at
**23:28:44 CST** still found AC disconnected, saver off, charge 87%; it does not
replace earlier observations or establish uninterrupted stability.

Host: Snapdragon X2 Elite Extreme X2E94100, native Windows ARM64. Pinned compiler:
`nightly-2026-11-12`. Prism source declaration:
`e126b8ff20bb76d8128ebd0e9a64a805f4a5662b`. Runner editor/supervisor source:
local `25d23c8b5ba4792d2b6d739f437742da00fe6619`. Helper SHA-256:
`711ef7e3280d8cd67dbccc732a5efafdfbe58db0a9dfc59fdffa4c4d992eed0d`
(3,298,816 bytes). Supervisor SHA-256:
`391048ad7cab1f4cc884fb6e7482d390181cd71ee9c6884c6e553ee9261225c2`.
Source identities are local declarations, not independently attested release
provenance. Original PQ2 hash:
`5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5`.

Before model setup, a separate seven-minute implementation allowance ran from
**23:21:15.538 to 23:25:43.060 CST** (267.522 seconds), plus immediate scratch
cleanup before its original 23:28:15.538 deadline. AC was connected before/after,
saver off, charge 86% to 87%; endpoints do not prove uninterrupted stability.
All **35 focused Release tests** passed: 5 editor, 4 shared-header/direction,
1 CLI discovery, 12 rank-one unit, 11 graph/state and 2 factual helper controls.
Strict Release all-target Clippy passed for `bonsai-tools` and `prism-inference`
with `parallel-rows`; the owned-language check passed. Initial lint findings were
corrected within that same allowance. Helpers were built before model setup.
No cfg-sensitive NEON dispatch changed. Full suites, other targets, app integration
and broader quality/device qualification remain pending owner-operated CI.

## Selected retained bytes

The following five nonempty files are selected for independent derivation,
candidate identity, factual/accounting, native supervision, resource and power
review. Fact/supervision bytes are original collector files; the edit CLI response
was captured as UTF-8. Copies were digest-verified. The candidate itself remains
in external research storage for replay; no duplicate adapter, source copy,
executable, blank stdout/stderr or whole scratch tree is archived here.
Candidate post-execution hashing, selected receipt retention and removal of the
named duplicate scratch files, empty directories and temporary driver completed
by **23:30:05.379 CST**, about **187.656 seconds** from the original model-workload
start and within its unchanged five-minute allowance. Later delivery edits and
Git commits do not authorize further model work.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `edit.json` | 52964 | `957339419e3279f6b967b523dd635e7dc518d7583ede627da95532a1319bfbc9` |
| `standard-facts.json` | 3584 | `0d6a1082d3033b42a822e6d377537fd8b842762d175ed45dd9f482d27b6321ef` |
| `standard-supervision.json` | 22774 | `c3842aa6a9023339a80527e58e6d394ef8316526b47785075fc480835345e9de` |
| `edited-facts.json` | 3689 | `44a427951211cbf493e233efafbb35af128cef52f0f14adbb7287ff610515b33` |
| `edited-supervision.json` | 22737 | `8274beae8ba073750c3df4b4f93e4f4fb973fa699da8788a9284712c90c17ca2` |
