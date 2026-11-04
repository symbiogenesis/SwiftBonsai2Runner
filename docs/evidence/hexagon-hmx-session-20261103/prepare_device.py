from pathlib import Path
import json
import shutil
import subprocess
import sys
from session import ROOT, pin, write

repo=Path(r'C:\Users\symbi\source\repos\SwiftBonsai2Runner')
source=repo/'.worktrees/prism-hmx-live-session'
revision='38ff4b974c9f406404a72939dee38e94b8c74939'
build=ROOT/'prism-target/aarch64-pc-windows-msvc/release/prism-hexagon-model-probe.exe'
bundle=ROOT/'model-bundle'
assert not (ROOT/'two-token-device').exists(), 'device evidence already exists; never retry'
bundle.mkdir(exist_ok=True)
probe=bundle/build.name
shutil.copy2(build,probe)
compiler=Path(r'C:\Users\symbi\.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\bin\rustc.exe')
subprocess.run([sys.executable,str(ROOT/'session.py'),'receipt',str(source),str(probe),str(compiler),revision],check=True)
receipt=json.loads((ROOT/'local-model-build.json').read_text())
main_source=repo.parent/'prism-inference'
line_ending_differences=[]
for entry in receipt['sources']:
    if pin(main_source/entry['path']) != {key:entry[key] for key in ['bytes','sha256']}:
        assert Path(entry['path']).suffix in ['.rs','.psm1','.ps1'], 'unexpected source difference'
        assert (main_source/entry['path']).read_bytes().replace(b'\r\n',b'\n') == (source/entry['path']).read_bytes().replace(b'\r\n',b'\n'), 'integrated source differs beyond line endings'
        line_ending_differences.append(entry['path'])
assert (repo/'tools/bonsai/src/hexagon_tools/model_device.rs').read_bytes().replace(b'\r\n',b'\n') == (repo/'.worktrees/runner-hmx-live-supervisor/tools/bonsai/src/hexagon_tools/model_device.rs').read_bytes().replace(b'\r\n',b'\n'), 'integrated supervisor source differs beyond line endings'
old=json.loads((repo/'docs/evidence/hexagon-model-device-20261103-ac/model-device-request.json').read_text(encoding='utf-8-sig'))
req=old['request']
req.update({'repositoryDirectory':str(repo),'prismRepositoryDirectory':str(source),'expectedPrismRevision':revision,'executablePath':str(probe),'executableSha256':pin(probe)['sha256'],'buildReceiptPath':str(ROOT/'local-model-build.json'),'outputDirectory':str(ROOT/'two-token-device'),'tokenIds':[42,43],'executionBackend':'hmx-bitplane'})
write('model-device-request.json',old)
write('intake.json',{'probeExecutable':pin(probe),'runnerExecutable':pin(ROOT/'runner-target/aarch64-pc-windows-msvc/release/bonsai-tools.exe'),'modelPresent':Path(req['modelPath']).is_file(),'packagePresent':Path(req['packageDirectory']).is_dir(),'catalogToolPresent':Path(req['signtoolPath']).is_file(),'buildSourcePrismRevision':revision,'integratedPrismMain':'1e0d6b4','integratedRunnerMain':'b124a572','sourceTextEquivalentAfterCherryPick':True,'mainVsBuildLineEndingDifferences':line_ending_differences,'exactSourceAdmission':'supervisor checks every raw-byte hash in the built worktree against local-model-build.json; integration comparison does not replace that guard'})
print(json.dumps({'request':str(ROOT/'model-device-request.json'),'intake':json.loads((ROOT/'intake.json').read_text())}))
