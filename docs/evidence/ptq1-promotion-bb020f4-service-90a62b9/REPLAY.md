# Frozen PTQ1 promotion document replay

The exact compiled score has **6 historical gates passed, 13 pending and no new
broad gate passed**. The selected candidate remains Runner `bb020f4`, current
PTQ1, Prism **0.1.13**, four workers and adapter scale 1.3. Only the separately
bound service harness is `90a62b9`. Later source fixes or runtime releases do not
change these execution identities.

`file-score-stdout.json` is the original compiled output, without reformatting.
`evidence.json`, both requests, raw plan output and execution receipts retain
their exact bytes. `file-manifest.json` inventories this archival data directory;
`raw-copy-manifest.json` verifies the original copied bytes. Historical manifests
under `baseline/`, `rollback/` and `service/` keep their original archival scope.
Their original ZIPs and executable packages remain in their existing archives,
outside this JSON/text selection.

Independent policy applicability is retained in
`reports/service-policy-applicability-review.json`; the separate actual workload
review is `reports/service-trial-independent-review.json`. The producer overview
beside this directory is not an independent final bundle review.

To replay document integrity, obtain the independently verified source-bound
`bonsai-tools.exe` from hosted preparation **41577882160**, source **90a62b9**:
6,289,920 bytes, SHA-256
`05370435a324f9eb927c2cddd4e5a6efb8ece30c74ec29e1bb4bc564f0e05a01`.
Verify the matching source receipt in
`reports/evaluator-package-receipt-90a62b9.json` and the exact input/report hashes.
Use a fresh copy of this data directory **outside Git**. Run only the native
offline commands:

```powershell
& $tool runtime-promotion --request (Join-Path $externalCopy 'plan-request.json')
# Preserve the historical request. Create a separate replay request changing only
# evidencePath to the absolute evidence.json path in this external copy.
& $tool runtime-promotion --request (Join-Path $externalCopy 'replay-file-score-request.json')
```

The original file-score request names its original external evidence path.
Archive new replay requests and outputs separately; do not overwrite historical
bytes. Matching policy and gate results establish offline document integrity,
without authenticating reviewer attestations or qualifying a new serving source.
Replay starts no model, device diagnostic, benchmark or test, and cannot reopen
the permanently closed owner session or change defaults.
