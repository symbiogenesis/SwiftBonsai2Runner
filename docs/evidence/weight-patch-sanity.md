# Initial adapter-cost and factual smoke result

The November 13, 2026 owner-device pilot found no speed benefit from removing
the approved adapter. Both arms answered all four facts correctly and stopped.
No model, adapter, application default or runtime pin changed. No LP correction
was fitted: the new offline fitter concerns behavioral corrections, not a direct
runtime/memory objective.

## Fixed work and limits

One standard-adapter arm followed by one absent-adapter arm, each in a fresh
native process. Four fixed answer-only prompts cover France's capital, `2+2`,
water's ASCII formula and the largest Solar System planet. Each reserves eight
output tokens and requires model-stop termination plus an exact normalized
answer. Original PQ2, native CPU four workers, experimental NEON dispatch,
K4/V4 cache, context 256, seed 42, Thinking off, no prompt reuse or accelerator.
This is a small CPU diagnostic, not application, broad-quality or throughput
qualification.

The predeclared aggregate allowance was 300 seconds, with a conservative
240-second estimate, two 110-second arm limits, bounded native Job supervision
and cleanup reserves. Workload ran **23:06:53–23:09:17 CST**, 143.999 seconds;
retention used the same original deadline, **23:11:53 CST**. No retry, extra
model arm, CI operation, push or publication occurred. The supervisor returned
`completed`, exit 0, after owned-process teardown for each arm. Empty stdout and
stderr files were removed; only the explicitly selected receipts below are
retained for independent factual, accounting, resource and power review.
Receipt copies were digest-verified and duplicate scratch inputs, executable,
orchestration metadata and directories were removed by **23:11:03 CST**, within
the original allowance (about 250 seconds including retention/cleanup).

## Observed answers and resources

| Fact | Exact answer in both arms | Prompt/output tokens | Standard request ms | No-adapter request ms |
| --- | --- | ---: | ---: | ---: |
| Capital | `Paris` | 26 / 1 | 11511.6603 | 14128.5946 |
| Arithmetic | `4` | 26 / 1 | 11686.7672 | 13667.7268 |
| Chemistry | `H2O` | 30 / 3 | 13907.8005 | 17171.7584 |
| Planet | `Jupiter` | 28 / 2 | 12808.4515 | 14859.8262 |

All eight answers passed and finished with `stop`; no truncation passed.
The four request times total approximately **49.91 s standard / 59.83 s absent**.
Load times were **23869.3622 / 8654.1738 ms**, respectively, and process wall
times **74464 / 69251 ms**. Faster second-arm loading is not a generation gain;
the fixed order and filesystem-cache effects are uncontrolled.

| Memory observation, bytes | Standard | No adapter |
| --- | ---: | ---: |
| Peak Job commit | 7578492928 | 7571308544 |
| Sampled peak root private | 7564054528 | 7556755456 |
| OS lifetime peak root working set | 7564607488 | 7557423104 |

The observed Job-commit difference is 7,184,384 bytes, about 6.9 MiB. These
counters are not additive or model-phase attribution; sampled private-memory
peaks can miss transients. No stable speed or memory reduction is qualified by
one ordered pair. Four facts cannot establish creative quality, refusal or
instruction-following preservation, longer-context behavior or daily service.

## Power and identity

`GetSystemPowerStatus` observed AC disconnected and battery saver off before and
after the whole workload and separately in each arm. Charge was **86%→86%** for
standard, **86%→85%** for absent, and **86%→85%** overall. CPU condition endpoints
were comparable, with balanced effective mode; availability/parking varied.
Endpoint agreement does not prove uninterrupted power, clocks or thermal
stability. No energy claim or qualified performance comparison follows.
An additional post-retention snapshot at **23:10:38 CST** observed AC
disconnected, saver off and 84% charge; it does not replace either arm's earlier
snapshots or establish power at unobserved times.

Host: Snapdragon X2 Elite Extreme X2E94100, native Windows ARM64, kernel
10.0.26300. Helpers were compiled before the model allowance with the pinned
`nightly-2026-11-12` toolchain. The model helper uses exact Swift artifact checks.

| Identity | Value |
| --- | --- |
| Prism source | `d30bcfec3c5ca8f57814aad428979a2abe258e57` |
| `swift_sanity_check.exe` SHA-256 | `3ba1041a019260d64af9997278ed97686eecc31ed991e489818e572cc7735acc` |
| Native supervisor SHA-256 | `d619837c8b9e47be9d3d7a237a4ccb71d15e8db94bf41b0ce695c4311cc7996c` |
| Original PQ2 SHA-256 | `5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5` |
| Approved adapter SHA-256 | `f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42` |
| Adapter scale bits | Standard `3fa66666`; absent `00000000` |

The supervision executable's digest is retained independently of its source
declaration; these local results grant no release/package acceptance. Exact
graph identities, complete prompts, answers, terminations and observed counters
remain in the receipts. Existing local-source lossless storage changes are not
new savings from this pilot.

## Selected replay receipts

Files under `weight-patch-sanity/` are the original nonempty JSON bytes. Their
consumer is independent review of this fixed factual smoke, supervision limits,
resources and power; they are not activation/fitting data or proof of another
runtime's quality. No whole scratch tree, binary, model, source copy or build log
is retained.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `standard-facts.json` | 3232 | `0b8c4ba3956fcc1beb4c0252f3c8abac3e52cb5e704b6fba05c200b9ac6c6e24` |
| `standard-supervision.json` | 22745 | `82d79f03cd90b6d4c3d3f72a5082dfadae2d653f556a112413cb106690a19f0c` |
| `none-facts.json` | 3037 | `7b2d70ba4b63a7135a78bc5ec308e24f23a18b2e04d202d85b3b413637ef022f` |
| `none-supervision.json` | 22745 | `42ad4756908303ac4535ca060e2e5d014dca27a23f2a893ac49470bb460d791d` |
