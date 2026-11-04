"""Bind the successful local build to immutable inputs for two new controls."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from session_run import ROOT, now, power, write

REPO = ROOT.parents[2]
PRISM = REPO / '.worktrees/x2-npu-host-cost'
EXTERNAL = Path(r'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Host-Reduction-20261103')
OLD = Path(r'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Progress-20261103/resident/model-bundle')
BUILT = Path(r'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103/prism-target/aarch64-pc-windows-msvc/release/prism-hexagon-model-probe.exe')
COMPILER = Path(r'C:\Users\symbi\.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc/bin/rustc.exe')


def digest(path):
    with path.open('rb') as source:
        return {'bytes': path.stat().st_size,
                'sha256': hashlib.file_digest(source, 'sha256').hexdigest()}


if __name__ == '__main__':
    assert not EXTERNAL.exists(), 'Never overwrite a prior control directory.'
    checks = json.loads((ROOT / 'prism-build-checks.json').read_text())
    assert checks['exitCode'] == 0 and not checks['timedOut']
    assert not subprocess.check_output(['git', '-C', str(PRISM), 'status', '--porcelain'], text=True).strip()
    revision = subprocess.check_output(['git', '-C', str(PRISM), 'rev-parse', 'HEAD'], text=True).strip()
    assert revision == '16c5e53e426a7813401550ac3a3823c00dbdaa7c'
    source_paths = []
    for folder in ['src', 'tools']:
        source_paths.extend(path for path in (PRISM / folder).rglob('*')
                            if path.is_file() and not {'target', '.git', '.worktrees'}.intersection(path.relative_to(PRISM).parts))
    source_paths.extend(PRISM / name for name in ['Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain.toml', '.cargo/config.toml']
                        if (PRISM / name).is_file())
    sources = [{'path': path.relative_to(PRISM).as_posix(), **digest(path)}
               for path in sorted(source_paths)]
    external_bundle = EXTERNAL / 'model-bundle'
    external_bundle.mkdir(parents=True)
    for path in OLD.iterdir():
        if path.is_file() and path.name != BUILT.name:
            shutil.copy2(path, external_bundle / path.name)
    shutil.copy2(BUILT, external_bundle / BUILT.name)
    receipt = {
        'schemaVersion': 1, 'kind': 'local-prism-hexagon-model-probe-build',
        'hostedBuild': False, 'buildSucceeded': True, 'cargoLocked': True,
        'target': 'aarch64-pc-windows-msvc', 'prismRevision': revision,
        'compiler': {'path': str(COMPILER), **digest(COMPILER)},
        'executable': digest(external_bundle / BUILT.name), 'sources': sources,
    }
    receipt_path = EXTERNAL / 'local-model-build.json'
    receipt_path.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    baseline = json.loads((REPO / 'docs/evidence/npu-adapter-progress-20261103/resident/model-device-request.json').read_text())
    requests = {}
    for name, resident in [('streaming', False), ('resident', True)]:
        request = json.loads(json.dumps(baseline))
        request['request'].update(
            repositoryDirectory=str(REPO), prismRepositoryDirectory=str(PRISM),
            expectedPrismRevision=revision, executablePath=str(external_bundle / BUILT.name),
            executableSha256=receipt['executable']['sha256'], buildReceiptPath=str(receipt_path),
            outputDirectory=str(EXTERNAL / name / 'device'), residentHostWeights=resident)
        destination = EXTERNAL / name / 'model-device-request.json'
        destination.parent.mkdir()
        destination.write_text(json.dumps(request, indent=2) + '\n', encoding='utf-8')
        requests[name] = [str(external_bundle / 'bonsai-tools.exe'), 'hexagon-tools', '--request', str(destination)]
    write('preparation.json', {
        'preparedUtc': now().isoformat(), 'power': power(),
        'runnerRevision': subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', 'HEAD'], text=True).strip(),
        'prismRevision': revision, 'probe': receipt['executable'],
        'reusedSupervisor': digest(external_bundle / 'bonsai-tools.exe'),
        'supervisorProvenance': 'Unmodified supervisor from retained successful resident build; runtime locks match current Runner and the supervisor validates all new probe sources and executable.',
        'buildReceipt': digest(receipt_path), 'sources': len(sources),
        'deviceCommands': requests, 'deviceAttempted': False,
    })
    print(json.dumps({'sources': len(sources), 'probe': receipt['executable'], 'requests': requests}))
