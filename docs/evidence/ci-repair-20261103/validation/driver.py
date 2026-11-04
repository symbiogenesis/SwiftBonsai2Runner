import ctypes,datetime,hashlib,json,os,re,subprocess,time,zipfile
from pathlib import Path
LANE=Path(__file__).parent
REPO=LANE.parent/'source'
PRIMARY=Path(r'C:\Users\symbi\source\repos\SwiftBonsai2Runner')
COMPILER=Path(os.environ['USERPROFILE'])/'.rustup/toolchains/nightly-2026-11-02-aarch64-pc-windows-msvc/bin'
GH=Path(os.environ['LOCALAPPDATA'])/'Microsoft/WinGet/Packages/GitHub.cli_Microsoft.Winget.Source_8wekyb3d8bbwe/bin/gh.exe'
START=time.monotonic()
class Power(ctypes.Structure):
    _fields_=[('ac',ctypes.c_ubyte),('flags',ctypes.c_ubyte),('charge',ctypes.c_ubyte),('saver',ctypes.c_ubyte),('life',ctypes.c_uint32),('full',ctypes.c_uint32)]
def power():
    p=Power(); r=dict(observedAtUtc=datetime.datetime.now(datetime.UTC).isoformat(),acConnected=None,batterySaver=None,chargePercent=None)
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(p)):
        r.update(acConnected={0:False,1:True}.get(p.ac),batterySaver={0:False,1:True}.get(p.saver),chargePercent=p.charge if p.charge<=100 else None,rawAcLineStatus=p.ac,rawBatteryFlag=p.flags)
    return r
receipt=dict(kind='corrected-source-ci-regression-validation',sourceRevision='a21b8caa',startedAtUtc=datetime.datetime.now(datetime.UTC).isoformat(),maximumTotalSeconds=300,workCutoffSeconds=265,powerBefore=power(),checks=[],clockClosed=False,performanceComparisonQualified=False,uninterruptedPowerStabilityEstablished=False,modelExecutions=0,benchmarks=0,fullSuites=0,ciDispatches=0,pushes=0)
SOURCES=['.github/workflows/ptq1-public-authoring-reference.yml','tools/native-reference/pins.json','config/prism-inference.lock.json','tools/bonsai/src/native_qualification/tests.rs','src/artifacts/tests/contained.rs','src/artifacts/tests/lifetime.rs','Cargo.toml','Cargo.lock','rust-toolchain.toml']
receipt['sourceFiles']={n:hashlib.sha256((REPO/n).read_bytes()).hexdigest() for n in SOURCES}
def elapsed(): return time.monotonic()-START
def save():
    receipt['elapsedSeconds']=elapsed()
    (LANE/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
def run(name,args,maximum=90,expected=0,tests=None,contains=None):
    allowance=min(maximum,265-elapsed())
    if allowance<5: raise RuntimeError('Next required work cannot fit original cutoff: '+name)
    env=os.environ.copy()
    env.update(PATH=str(COMPILER)+os.pathsep+env['PATH'],RUSTC=str(COMPILER/'rustc.exe'),RUSTDOC=str(COMPILER/'rustdoc.exe'),CARGO_TARGET_DIR=str(LANE.parent/'target'),CARGO_BUILD_JOBS='4',CARGO_NET_OFFLINE='true',CARGO_TERM_COLOR='never',CARGO_BUILD_FINGERPRINT='content',CARGO_UNSTABLE_CHECKSUM_FRESHNESS='true')
    entry=dict(name=name,command=[str(v) for v in args],powerBefore=power(),startedSeconds=elapsed(),passed=False)
    receipt['checks'].append(entry); save()
    logpath=LANE/(name+'.log')
    with logpath.open('wb') as log:
        child=subprocess.Popen(entry['command'],cwd=LANE,env=env,stdout=log,stderr=log,stdin=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
        entry['processId']=child.pid; save()
        try:
            entry['exitCode']=child.wait(timeout=allowance)
        except subprocess.TimeoutExpired:
            result=subprocess.run([str(Path(os.environ['SystemRoot'])/'System32/taskkill.exe'),'/PID',str(child.pid),'/T','/F'],stdout=log,stderr=log,timeout=10,creationflags=subprocess.CREATE_NO_WINDOW,check=False)
            entry.update(failure='original-command-or-aggregate-cutoff',treeKillExitCode=result.returncode)
            child.wait(timeout=5)
        finally:
            entry.update(powerAfter=power(),finishedSeconds=elapsed()); save()
    output=logpath.read_text(encoding='utf-8-sig',errors='replace')
    if entry.get('exitCode')!=expected: raise RuntimeError('Required command failed: '+name)
    if tests is not None:
        summaries=re.findall(r'^test result: ok\. (\d+) passed; 0 failed; 0 ignored;',output,re.MULTILINE)
        if summaries!=[str(tests)]: raise RuntimeError('Required exact test count absent: '+name+': '+repr(summaries))
        entry['focusedTestsPassed']=tests
    if contains and contains not in output: raise RuntimeError('Required diagnostic absent: '+name)
    entry['passed']=True; save()
    print(json.dumps({k:v for k,v in entry.items() if k not in ['command']}),flush=True)
    return output
def cargo(name,args,maximum=90,tests=None):
    return run(name,[COMPILER/'cargo.exe',args[0],'--manifest-path',REPO/'Cargo.toml',*args[1:]],maximum,tests=tests)
try:
    run('compiler-identity',[COMPILER/'rustc.exe','-vV'],10)
    cargo('workspace-format',['fmt','--all','--','--check'],15)
    download=LANE/'actionlint-download'; download.mkdir()
    archive='actionlint_1.7.12_windows_arm64.zip'
    run('actionlint-intake',[GH,'release','download','v1.7.12','--repo','rhysd/actionlint','--pattern',archive,'--pattern','actionlint_1.7.12_checksums.txt','--dir',download],25)
    sums=(download/'actionlint_1.7.12_checksums.txt').read_text()
    digest=hashlib.sha256((download/archive).read_bytes()).hexdigest()
    if not any(line.split()==[digest,archive] for line in sums.splitlines()): raise RuntimeError('Official actionlint archive checksum mismatch')
    receipt['actionlint']=dict(version='1.7.12',archive=archive,sha256=digest,source='https://github.com/rhysd/actionlint/releases/tag/v1.7.12')
    tool=LANE/'actionlint'; tool.mkdir()
    with zipfile.ZipFile(download/archive) as packed:
        for member in packed.infolist():
            name=Path(member.filename)
            if name.is_absolute() or '..' in name.parts or ':' in member.filename: raise RuntimeError('Unsafe tool archive path')
        packed.extractall(tool)
    baseline=LANE/'original-public-reference.yml'
    baseline.write_bytes((PRIMARY/'.github/workflows/ptq1-public-authoring-reference.yml').read_bytes())
    actionlint=tool/'actionlint.exe'
    run('original-workflow-context-negative',[actionlint,'-shellcheck=','-pyflakes=',baseline],10,expected=1,contains='context "runner" is not allowed here')
    run('fixed-workflow-context',[actionlint,'-shellcheck=','-pyflakes=',REPO/'.github/workflows/ptq1-public-authoring-reference.yml'],10)
    tools_tests=['native_qualification::request_timing::tests::timing_mode_is_explicit_one_standard_adapter_rust_request','native_qualification::tests::adapter_conditions_select_one_bound_pair_and_cannot_change_the_row_control','native_qualification::tests::capture_source_cannot_substitute_an_internally_consistent_stale_checkout','native_qualification::tests::current_runtime_source_and_bounded_atomic_receipts_fail_closed','native_qualification::tests::plans_keep_one_selected_ptq1_pair_and_the_fixed_row_pair','native_qualification::tests::missing_or_expired_owner_allowances_reject_before_preparation','native_qualification::tests::checked_in_ptq1_capture_source_matches_released_runtime']
    cargo('tools-source-and-allowance-controls',['test','--locked','--offline','--release','-p','bonsai-tools','--lib','--',*tools_tests,'--exact'],150,7)
    cargo('contained-ancestor-release-control',['test','--locked','--offline','--release','-p','bonsai-artifacts','--test','contained','host_verification_denies_nested_ancestor_rename_and_delete_until_drop','--','--exact'],40,1)
    lifetime_tests=['runtime_guard_holds_archive_installation_and_ancestors_until_drop','cancelled_runtime_verification_releases_open_handles','runtime_guard_release_ignores_unrelated_shared_temp_ancestor_guards']
    cargo('runtime-release-and-sibling-controls',['test','--locked','--offline','--release','-p','bonsai-artifacts','--test','lifetime','--',*lifetime_tests,'--exact'],30,3)
    cargo('touched-strict-release-all-targets',['clippy','--locked','--offline','--release','-p','bonsai-artifacts','-p','bonsai-tools','--all-features','--all-targets','--','-D','warnings'],75)
    receipt['allRequestedChecksPassed']=True
except Exception as error:
    receipt.update(allRequestedChecksPassed=False,failure=str(error)); print(str(error),flush=True)
finally:
    receipt.update(powerAfter=power(),clockClosed=True); save()
    retained={path.name:hashlib.sha256(path.read_bytes()).hexdigest() for path in LANE.iterdir() if path.is_file() and path.name!='file-manifest.json'}
    (LANE/'file-manifest.json').write_text(json.dumps(retained,indent=2)+'\n')
    receipt['elapsedSecondsThroughLogManifestRetention']=elapsed(); save()
    print(json.dumps({k:v for k,v in receipt.items() if k not in ['checks','sourceFiles']}),flush=True)
