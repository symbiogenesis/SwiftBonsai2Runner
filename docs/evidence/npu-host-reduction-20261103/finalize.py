"""Charge later review/document retention to the original closed trial clock."""
import datetime as dt
import hashlib
import json

from session_run import ROOT, now, power, write


if __name__ == '__main__':
    assert not (ROOT / 'final-archival.json').exists(), 'Final archival already retained.'
    clock = json.loads((ROOT / 'clock.json').read_text())
    observed = now()
    elapsed = (observed - dt.datetime.fromisoformat(clock['declarationUtc'])).total_seconds()
    charged = elapsed + clock['earlierReadOnlyPreparationChargeSeconds']
    write('final-archival.json', {
        'schemaVersion': 1, 'finalArchivalUtc': observed.isoformat(),
        'originalDeclarationUtc': clock['declarationUtc'], 'originalDeadlineUtc': clock['deadlineUtc'],
        'elapsedSinceDeclarationSeconds': elapsed,
        'earlierReadOnlyPreparationChargeSeconds': clock['earlierReadOnlyPreparationChargeSeconds'],
        'chargedThroughFinalArchivalSeconds': charged,
        'maximumAggregateSeconds': clock['maximumAggregateSeconds'],
        'withinOriginalAllowance': charged <= clock['maximumAggregateSeconds'],
        'power': power(), 'initialReadOnlyPreparationPower': 'unknown',
        'scope': 'Includes review, documentation edits, local Prism integration and evidence retention after the earlier device-workload close snapshot. No new device or model work occurred after that snapshot.',
        'historicalDeadlinesReopened': False, 'failedTrialsRetried': False,
        'ciDispatchedOrMonitored': False, 'pushed': False,
    })
    entries = []
    for path in sorted(ROOT.rglob('*')):
        if not path.is_file() or path.name == 'retained-input-index.json' or '__pycache__' in path.parts:
            continue
        with path.open('rb') as source:
            entries.append({'path': path.relative_to(ROOT).as_posix(), 'bytes': path.stat().st_size,
                            'sha256': hashlib.file_digest(source, 'sha256').hexdigest()})
    write('retained-input-index.json', {'schemaVersion': 1, 'files': entries})
    print(json.dumps({'retainedFiles': len(entries), 'chargedSeconds': charged,
                      'maximumAggregateSeconds': clock['maximumAggregateSeconds']}))
