# Runtime conformance boundary

[Prism's conformance guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-upstream-tests.md)
owns runtime algorithms, codecs, source assertions and supported subsets. Its
[catalog](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/catalog.json),
[migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json)
and [conformance workflow](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml)
are authoritative for upstream coverage. [PLAN.md](../PLAN.md#deferred-prism-migration)
owns Runner's deferred migration work.

Runner's [Foundation](../.github/workflows/foundation.yml) checks the app, broker,
process ownership, provisioning and packaging. It does not run Prism's runtime
catalog. Broader upstream APIs are outside Runner's [local API](broker-policy.md#public-request-contract)
and [Swift runtime subset](native-rust-engine.md).

## Source fidelity and evidence

Replacements preserve imported inputs, assertions, tolerances, schedules,
unsupported outcomes and production bridges. A focused helper cannot stand in
for complete source execution. Preserve independent controls under the
[retirement rules](cpp-retirement.md#replacement-and-deletion-rules).

Upstream acceptance includes the complete 71-source inventory (70 applicable to
Windows), all 22,817 ordered backend geometries in evaluation and support modes,
and a separate 1,530-workload performance schedule. Keep original source IDs,
required green results, exclusions, report hashes and the tested revision.
Do not turn an unsupported reference operation or abort into a passing result.
Approved DSpark saver and Llama 3 decoding corrections remain part of the pinned
reference identity.

Full suites and benchmarks run in the owning repository's CI under
[validation policy](validation.md). Hosted ARM64 results do not establish
Snapdragon performance; device claims follow the
[cross-generation rule](cpu-kernels.md#cross-generation-cpu-promotion).

## Numerical operations and codec contracts

Use the upstream guide for exact operation/type inventories, layouts, native
encoder availability, numerical tolerances and source defects. Preserve shape,
stride, aliasing, arithmetic, allocation and failure behavior. Packed decoding,
packed-activation dots, encoding and graph routing are separate capabilities;
float GEMV cannot substitute for packed dots. Undefined native bytes have no
byte-parity contract.

## Chat, parser and tokenizer contracts

Use upstream source contracts for template analysis, parser/capability schedules,
tokenizers, grammar and reasoning/tool conversions. Runner separately requires
independent exact-token/template checks for the pinned Swift artifact, including
forged role markers, literal lookalikes and malformed inputs. Preserve legitimate
user text and the broker's supported subset.

## Acquisition, model and session contracts

Upstream owns CLI, retrieval, architecture, model, state and concurrency coverage.
Vocabulary-only sessions, copied architecture lists, standalone checkpoints and
model-resolution helpers do not qualify the corresponding production APIs.

## DSpark, adapters, grammar and ABI

Preserve upstream's complete DSpark schedules and approved corrections. LoRA
conformance includes conversion, merge/application and inference. LLGuidance's
model-session grammar contract is distinct from GBNF conversion. The public
`test-c.c` contract is compile-only; it has no behavioral assertions.

## Source reuse review

Pin extraction sources, licenses, hardware assumptions and supported subsets
before reuse. Camelid attribution is retained in [NOTICE](../NOTICE) and its
[MIT license](https://github.com/symbiogenesis/prism-inference/blob/main/CAMELID-LICENSE.txt);
ggml attribution covers direct Prism adaptations. The upstream conformance guide
owns implementation-specific reuse decisions and their remaining gaps.
