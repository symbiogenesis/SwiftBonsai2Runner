from pathlib import Path
import json
import shutil
import subprocess
import sys
from session import ROOT, pin, write, now, DEADLINE

repo = Path(r'C:\Users\symbi\source\repos\SwiftBonsai2Runner')
source = repo / '.worktrees/npu-prism-qualification'
runner_source = repo / '.worktrees/npu-runner-qualification'
git = Path.home() / 'AppData/Local/Microsoft/WinGet/Packages/Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe/cmd/git.exe'
old_root = ROOT.parent / 'SwiftBonsai-Hmx-Session-20261103'
output = ROOT / 'standard-adapter-device'
assert not output.exists(), 'New device evidence already exists; do not retry.'
assert (DEADLINE-now()).total_seconds() > 420, 'No room for intake, device and retention.'

def revision(directory):
    assert not subprocess.check_output([str(git), '-C', str(directory), 'status', '--porcelain'], text=True).strip(), 'Build source must be clean.'
    return subprocess.check_output([str(git), '-C', str(directory), 'rev-parse', 'HEAD'], text=True).strip()

prism_revision = revision(source)
runner_revision = revision(runner_source)
bundle = ROOT / 'model-bundle'
bundle.mkdir(exist_ok=False)
probe = bundle / 'prism-hexagon-model-probe.exe'
supervisor = bundle / 'bonsai-tools.exe'
shutil.copy2(old_root/'prism-target/aarch64-pc-windows-msvc/release'/probe.name, probe)
shutil.copy2(old_root/'runner-target/release'/supervisor.name, supervisor)
compiler = Path.home()/'.rustup/toolchains/nightly-2026-10-22-aarch64-pc-windows-msvc/bin/rustc.exe'
subprocess.run([sys.executable, str(ROOT/'session.py'), 'receipt', str(source), str(probe), str(compiler), prism_revision], check=True)
receipt = json.loads((ROOT/'local-model-build.json').read_text())

line_endings = []
for entry in receipt['sources']:
    main_path = repo.parent/'prism-inference'/entry['path']
    if pin(main_path) != {key:entry[key] for key in ['bytes','sha256']}:
        assert main_path.read_bytes().replace(b'\r\n',b'\n') == (source/entry['path']).read_bytes().replace(b'\r\n',b'\n'), 'Integrated Prism source differs beyond line endings.'
        line_endings.append(entry['path'])
supervisor_path = 'tools/bonsai/src/hexagon_tools/model_device.rs'
assert (repo/supervisor_path).read_bytes().replace(b'\r\n',b'\n') == (runner_source/supervisor_path).read_bytes().replace(b'\r\n',b'\n'), 'Integrated supervisor differs.'

envelope = json.loads((repo/'docs/evidence/hexagon-hmx-session-20261103/model-device-request.json').read_text(encoding='utf-8-sig'))
request = envelope['request']
adapter = Path.home()/'AppData/Local/SwiftBonsai2/adapters/703028bb7fba03496e796b144839b161cf0f5409/bonsai-abliterate-lora.gguf'
assert pin(adapter) == {'bytes':9682464,'sha256':'f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'}, 'Installed standard adapter differs.'
request.update({
    'repositoryDirectory':str(repo), 'prismRepositoryDirectory':str(source),
    'expectedPrismRevision':prism_revision, 'executablePath':str(probe),
    'executableSha256':pin(probe)['sha256'], 'buildReceiptPath':str(ROOT/'local-model-build.json'),
    'outputDirectory':str(output), 'tokenIds':[42,43,128,256],
    'executionBackend':'hmx-bitplane','adapterCase':'standard-scale-1.3',
    'adapterPath':str(adapter),'cpuBackend':'neon','rowWorkers':4
})
write('model-device-request.json', envelope)
write('intake.json', {
    'probeExecutable':pin(probe),'runnerExecutable':pin(supervisor),
    'buildSourcePrismRevision':prism_revision,'buildSourceRunnerRevision':runner_revision,
    'modelPresent':Path(request['modelPath']).is_file(),
    'adapter':{'path':str(adapter),**pin(adapter)},
    'packagePresent':Path(request['packageDirectory']).is_dir(),
    'catalogToolPresent':Path(request['signtoolPath']).is_file(),
    'integratedSourceTextEquivalent':True,'prismLineEndingDifferences':line_endings,
    'exactSourceAdmission':'Supervisor independently hashes and holds every raw built-worktree source entry, executable, adapter, model, driver and runtime input.'
})
print(json.dumps({'request':str(ROOT/'model-device-request.json'), 'prismRevision':prism_revision,'runnerRevision':runner_revision,'probeExecutable':pin(probe),'runnerExecutable':pin(supervisor)}))
