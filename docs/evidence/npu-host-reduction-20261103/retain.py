"""Retain new controls and close this workload without altering older receipts."""
import datetime as dt
import hashlib
import json
from pathlib import Path
import shutil

from session_run import ROOT, now, power, write

EXTERNAL = Path(r'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Host-Reduction-20261103')


def digest(path):
    with path.open('rb') as source:
        return {'bytes': path.stat().st_size,
                'sha256': hashlib.file_digest(source, 'sha256').hexdigest()}


def retain_case(name):
    source = EXTERNAL / name
    for path in source.rglob('*'):
        if not path.is_file():
            continue
        assert path.stat().st_size < 2 * 1024 * 1024
        target = ROOT / name / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
    report = json.loads((ROOT / name / 'device/model-device.json').read_text(encoding='utf-8-sig'))
    result = report.get('NativeResult', {})
    return {
        'outcome': report.get('Outcome'), 'stage': report.get('Stage'),
        'probeExitCode': report.get('ProbeExitCode'), 'ownedJobTeardown': report.get('OwnedJobTeardown'),
        'sourceFilesVerified': report.get('LocalBuildSourceFilesVerified'),
        'operationsCompleted': result.get('nativeOperationsCompleted'),
        'integerValuesCompared': result.get('integerValuesCompared'),
        'integerAudit': result.get('integerAudit'),
        'logitsBitwiseEqual': result.get('logitsBitwiseEqual'),
        'stateBitwiseEqual': result.get('stateBitwiseEqual'),
        'nativeCleanup': result.get('nativeCleanup'), 'nativeResources': result.get('nativeResources'),
        'hostResidency': result.get('hostResidency'), 'timingScope': result.get('timingScope'),
        'tokenComparisons': result.get('tokenComparisons'),
        'power': {key: result.get(key) for key in ['cpuPowerBefore', 'cpuPowerAfter', 'npuPowerBefore', 'npuPowerAfter']},
        'supervisorElapsedMilliseconds': report.get('ElapsedMilliseconds'),
        'qualifiedBenefitComparison': False,
    }


if __name__ == '__main__':
    assert not (ROOT / 'workload-close.json').exists(), 'This workload is already closed.'
    shutil.copy2(EXTERNAL / 'local-model-build.json', ROOT / 'local-model-build.json')
    cases = {name: retain_case(name) for name in ['streaming', 'resident']}
    clock = json.loads((ROOT / 'clock.json').read_text())
    elapsed = (now() - dt.datetime.fromisoformat(clock['declarationUtc'])).total_seconds()
    charged = elapsed + clock['earlierReadOnlyPreparationChargeSeconds']
    write('summary.json', {
        'schemaVersion': 1, 'kind': 'local-x2-hmx-exact-weight-transpose-controls',
        'dateLocal': '2026-11-03', 'retainedUtc': now().isoformat(),
        'prismRevision': '16c5e53e426a7813401550ac3a3823c00dbdaa7c',
        'localChecks': {'format': 'passed', 'focusedArithmeticAndFailureControlsPassed': 14,
                        'strictReleaseLibraryClippy': 'passed', 'releaseModelProbeBuild': 'passed'},
        'cases': cases, 'elapsedSinceDeclarationSeconds': elapsed,
        'earlierReadOnlyPreparationChargeSeconds': clock['earlierReadOnlyPreparationChargeSeconds'],
        'chargedThroughRetentionSeconds': charged, 'maximumAggregateSeconds': clock['maximumAggregateSeconds'],
        'powerAtRetention': power(), 'initialReadOnlyPreparationPower': 'unknown',
        'historicalDeadlinesReopened': False, 'failedTrialsRetried': False,
        'deviceAttempts': 2, 'workloadClosed': True,
        'ciDispatchedOrMonitored': False, 'pushed': False,
        'productionAdmissionEnabled': False, 'installedApplicationChanged': False,
        'qualifiedBenefitComparison': False,
        'interpretation': 'Correctness covers one first-projection placement for tokens42,43,128 with standard adapter1.3 and NEON/four workers. Streaming checks every returned integer; resident mode uses complete graph parity. No broader serving or performance acceptance. Current resident CPU/native windows show no useful benefit. No before/after packing performance comparison is claimed; earlier runs are distinct workloads.',
    })
    write('workload-close.json', {
        'closedUtc': now().isoformat(), 'originalDeadlineUtc': clock['deadlineUtc'],
        'chargedThroughCloseSeconds': (now() - dt.datetime.fromisoformat(clock['declarationUtc'])).total_seconds()
                                      + clock['earlierReadOnlyPreparationChargeSeconds'],
        'maximumAggregateSeconds': clock['maximumAggregateSeconds'],
        'power': power(), 'historicalDeadlinesReopened': False,
        'stopReason': 'Both declared source-bound signed controls finished and cleaned up. Cached first-projection offload supplies no speed advantage or production serving path. Retain useful packing change and source-reviewed batch-four next implementation without another device run.',
    })
    files = [{'path': path.relative_to(ROOT).as_posix(), **digest(path)}
             for path in sorted(ROOT.rglob('*')) if path.is_file()
             and path.name != 'retained-input-index.json' and '__pycache__' not in path.parts]
    write('retained-input-index.json', {'schemaVersion': 1, 'files': files})
    print(json.dumps({'cases': {name: {key: value[key] for key in
        ['outcome', 'operationsCompleted', 'integerValuesCompared', 'logitsBitwiseEqual', 'stateBitwiseEqual', 'ownedJobTeardown']}
        for name, value in cases.items()}, 'chargedSeconds': charged}))
