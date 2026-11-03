import base64, ctypes, hashlib, json, os, re, subprocess, sys, time
from pathlib import Path

LANE = Path(__file__).parent
REPO = Path(r"C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\ptq1-unblock-integration")
COMPILER = Path(r"C:\Users\symbi\.rustup\toolchains\nightly-2026-11-02-aarch64-pc-windows-msvc\bin")
TARGET = Path(r"C:\Users\symbi\AppData\Local\Temp\ptq1-local-validation-20261103\target")
GIT = Path(r"C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe\cmd\git.exe")
POWERSHELL = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
PLAN = json.loads((LANE / "plan.json").read_text(encoding="utf-8"))
START = time.monotonic()

class Power(ctypes.Structure):
    _fields_ = [("ac",ctypes.c_ubyte),("flags",ctypes.c_ubyte),("percent",ctypes.c_ubyte),("saver",ctypes.c_ubyte),
                ("life",ctypes.c_uint32),("full_life",ctypes.c_uint32)]

def power():
    result = dict(observedAtUtc=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),source="GetSystemPowerStatus",
                  acConnected=None,batterySaver=None,chargePercent=None)
    status=Power()
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        result.update(acConnected={0:False,1:True}.get(status.ac),batterySaver={0:False,1:True}.get(status.saver),
                      chargePercent=status.percent if status.percent<=100 else None,batteryFlags=int(status.flags))
    return result

def elapsed(): return time.monotonic()-START

receipt=dict(schemaVersion=1,kind=PLAN["kind"],sourceBaseRevision=PLAN["sourceRevision"],maximumTotalSeconds=300,
             workCutoffSeconds=255,checks=[],powerBefore=power(),modelExecutions=0,benchmarks=0,fullSuites=0,ciActions=0,
             pushes=0,performanceComparisonQualified=False,uninterruptedPowerStabilityEstablished=False,
             actualModelAcceptancePassed=False,defaultPromotionQualified=False)
environment=os.environ.copy()
environment.update(PATH=str(COMPILER)+os.pathsep+environment["PATH"],RUSTC=str(COMPILER/"rustc.exe"),
                   RUSTDOC=str(COMPILER/"rustdoc.exe"),CARGO_TARGET_DIR=str(TARGET),
                   CARGO_UNSTABLE_CHECKSUM_FRESHNESS="true",CARGO_BUILD_FINGERPRINT="content",
                   PYTHONPATH=str(REPO/"tools/native-reference"))

def save():
    receipt["elapsedSeconds"]=elapsed()
    (LANE/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n",encoding="utf-8")

def run(name,arguments,maximum=90,minimum_remaining=3,exact_tests=None):
    allowance=min(maximum,255-elapsed())
    if allowance<minimum_remaining:
        raise RuntimeError("Required next check no longer fits original cutoff: "+name)
    record=dict(name=name,command=[str(v) for v in arguments],startedSeconds=elapsed(),powerBefore=power(),passed=False)
    receipt["checks"].append(record)
    stem=re.sub(r"[^a-zA-Z0-9_-]","_",name)
    stdout,stderr=LANE/(stem+".stdout.log"),LANE/(stem+".stderr.log")
    with stdout.open("wb") as output,stderr.open("wb") as errors:
        child=subprocess.Popen(record["command"],cwd=LANE,env=environment,stdout=output,stderr=errors,
                               creationflags=subprocess.CREATE_NO_WINDOW)
        deadline=time.monotonic()+allowance
        try:
            while child.poll() is None:
                if time.monotonic()>=deadline or max(stdout.stat().st_size,stderr.stat().st_size)>4*1024*1024:
                    record["failure"]="original-step-or-work-cutoff-or-output-limit"
                    killed=subprocess.run([str(Path(os.environ["SystemRoot"])/"System32/taskkill.exe"),
                                           "/PID",str(child.pid),"/T","/F"],stdout=output,stderr=errors,timeout=10,
                                          creationflags=subprocess.CREATE_NO_WINDOW,check=False)
                    record["treeKillExitCode"]=killed.returncode
                    child.wait(timeout=5)
                    record["rootReaped"]=True
                    raise RuntimeError("Selected check exceeded fixed allowance: "+name)
                time.sleep(0.05)
            record["exitCode"]=child.returncode
            if child.returncode!=0:
                raise RuntimeError("Selected check failed: "+name)
            if exact_tests is not None:
                summaries=re.findall(r"^test result: ok\. (\d+) passed; 0 failed; 0 ignored;",
                                     stdout.read_text(encoding="utf-8-sig"),re.MULTILINE)
                if summaries!=[str(exact_tests)]:
                    raise RuntimeError("Expected focused exact test count not executed: "+name)
                record["focusedTestsPassed"]=exact_tests
            record["passed"]=True
        finally:
            record.update(finishedSeconds=elapsed(),powerAfter=power())
            save()
    return stdout.read_text(encoding="utf-8-sig")

def cargo(name,arguments,maximum=90,exact_tests=None,minimum_remaining=5):
    return run(name,[COMPILER/"cargo.exe",arguments[0],"--locked","--offline","--release","--manifest-path",
                     REPO/"Cargo.toml",*arguments[1:]],maximum,minimum_remaining,exact_tests)

try:
    revision=run("source-revision",[GIT,"-C",REPO,"rev-parse","HEAD"],10).strip()
    if not revision.startswith(PLAN["sourceRevision"]): raise RuntimeError("Source changed after declaration")
    receipt["checkedSourceRevision"]=revision
    run("source-clean",[GIT,"-C",REPO,"diff","--exit-code"],10)
    sources=[".github/workflows/native-runtime-acceptance.yml",".github/workflows/ptq1-public-authoring-reference.yml",
             "tools/ci/src/native_acceptance_guard.rs","tools/ci/src/command.rs",
             "tools/native-reference/public_pack_reference.py","tools/native-reference/audit_public_reference.py",
             "tools/native-reference/test_audit_public_reference.py","src/broker/tests/native_model/response_capture.rs",
             "src/broker/tests/native_model/progress.rs","src/broker/tests/native_model/service_integration.rs"]
    receipt["sourceFiles"]=[dict(file=n,bytes=(REPO/n).stat().st_size,
                                sha256=hashlib.sha256((REPO/n).read_bytes()).hexdigest()) for n in sources]
    script=r"""
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$repo='REPO_LITERAL'
$counts=@()
foreach ($name in @('.github/workflows/native-runtime-acceptance.yml','.github/workflows/ptq1-public-authoring-reference.yml')) {
  $lines=[IO.File]::ReadAllLines((Join-Path $repo $name))
  $blocks=0
  for ($index=0; $index -lt $lines.Length; $index++) {
    if ($lines[$index] -notmatch '^(?<indent> +)run:\s*\|\s*$') { continue }
    $indent=$Matches.indent.Length+2
    $body=[Collections.Generic.List[string]]::new()
    for ($next=$index+1; $next -lt $lines.Length; $next++) {
      $line=$lines[$next]
      if ($line.Trim().Length -gt 0 -and ($line.Length -lt $indent -or $line.Substring(0,$indent).Trim().Length -gt 0)) { break }
      $body.Add($(if ($line.Length -ge $indent) { $line.Substring($indent) } else { '' }))
    }
    $tokens=$null; $parseErrors=$null
    [Management.Automation.Language.Parser]::ParseInput(($body -join [char]10),[ref]$tokens,[ref]$parseErrors) | Out-Null
    if ($parseErrors.Count -ne 0) { throw ('Workflow parse failed: '+$name) }
    $blocks++
  }
  $counts += [ordered]@{ File=$name; ParsedBlocks=$blocks }
}
if ($counts[0].ParsedBlocks -ne 9) { throw 'Native workflow block inventory differs.' }
$tokens=$null; $parseErrors=$null
[Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'scripts/Start-Chat.ps1'),[ref]$tokens,[ref]$parseErrors) | Out-Null
if ($parseErrors.Count -ne 0) { throw 'Launcher parse failed.' }
[ordered]@{ Workflows=$counts; LauncherParsed=$true; WorkflowCommandsExecuted=$false } | ConvertTo-Json -Depth 5 -Compress
""".replace("REPO_LITERAL",str(REPO).replace("'","''"))
    (LANE/"parser-source.txt").write_text(script,encoding="utf-8")
    receipt["workflowParsing"]=json.loads(run("workflow-native-parser",[POWERSHELL,"-NoProfile","-NonInteractive",
        "-EncodedCommand",base64.b64encode(script.encode("utf-16le")).decode()],10))
    tests=["test_sealed_source_replays_with_different_checkout_bytes_without_executing_producer",
           "test_sealed_source_rejects_missing_extra_changed_and_wrong_type_entries",
           "test_sealed_source_rejects_manifest_tampering_paths_and_approved_lock_substitution",
           "test_sealed_source_preserves_a_verified_losing_result",
           "test_sealed_source_rejects_recipe_substitution_despite_consistent_report_and_manifest",
           "test_complete_scalar_replay_retains_qualification_boundaries"]
    run("retained-source-replay-controls",[sys.executable,"-B","-m","unittest",
        *["test_audit_public_reference.PublicReplayTests."+n for n in tests]],30)
    receipt["focusedPythonCasesPassed"]=len(tests)
    for name in ["negative_fixtures_cover_receipts_and_test_output","workflow_receipt_requires_source_bound_complete_controls"]:
        cargo("guard-"+name,["test","-p","bonsai-ci","--lib","command::native_acceptance_guard::tests::"+name,
                            "--","--exact"],90,1,15)
    cases=["response_capture::native_response_capture_hashes_fragmented_errors_without_retaining_payloads",
           "response_capture::native_response_capture_cancelled_read_retains_prefix_and_original_deadline",
           "response_capture::native_response_capture_caps_header_storage_and_classifies_connection_failure",
           "progress::native_progress_read_error_retains_partial_response_identity_before_cleanup",
           "service_integration::native_service_integration_cancellation_failure_retains_response_identity"]
    for index,name in enumerate(cases):
        cargo("service-control-"+str(index+1),["test","-p","bonsai-broker","--test","native_model",name,"--","--exact"],
              90,1,15 if index==0 else 5)
    cargo("guard-production-build",["build","-p","bonsai-ci","--bin","bonsai-ci"],35,minimum_remaining=15)
    binary=TARGET/"release/bonsai-ci.exe"
    receipt["guardExecutable"]=dict(path=str(binary),bytes=binary.stat().st_size,
                                    sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    result=json.loads(run("guard-executable-self-check",[binary,"native-acceptance-guard","--json",
                      json.dumps(dict(operation="self-check",revision=revision))],5))
    if result.get("Success") is not True or (result.get("ReceiptFixtures"),result.get("OutputFixtures"))!=(23,13):
        raise RuntimeError("Guard executable self-check receipt incomplete")
    receipt["guardSelfCheck"]=result
    cargo("guard-release-library-clippy",["clippy","-p","bonsai-ci","--lib","--","-D","warnings"],45,minimum_remaining=20)
    receipt["allRequestedChecksPassed"]=True
except Exception as error:
    receipt.update(allRequestedChecksPassed=False,failureClass=type(error).__name__,failure=str(error)[:512])
finally:
    receipt["powerAfterWork"]=power()
    receipt["clockClosed"]=True
    save()
    (LANE/"driver-source.txt").write_bytes((LANE/"driver.py").read_bytes())
    records=[]
    for path in sorted(LANE.iterdir()):
        if path.is_file() and path.name not in ("file-manifest.json","retention.json"):
            raw=path.read_bytes()
            records.append(dict(file=path.name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()))
    (LANE/"file-manifest.json").write_text(json.dumps(dict(files=records),indent=2)+"\n",encoding="utf-8")
    retention=dict(chargedSecondsThroughRetention=elapsed(),maximumTotalSeconds=300,
                   retainedWithinDeclaredTotal=elapsed()<=300,powerAfterRetention=power(),
                   rawReceiptSha256=hashlib.sha256((LANE/"receipt.json").read_bytes()).hexdigest(),
                   rawManifestSha256=hashlib.sha256((LANE/"file-manifest.json").read_bytes()).hexdigest(),
                   originalClockRestarted=False,noAdditionalModelAttempts=True)
    (LANE/"retention.json").write_text(json.dumps(retention,indent=2)+"\n",encoding="utf-8")
    print(json.dumps(dict(allRequestedChecksPassed=receipt["allRequestedChecksPassed"],failure=receipt.get("failure"),
                         checks=len(receipt["checks"]),chargedSecondsThroughRetention=retention["chargedSecondsThroughRetention"],
                         evidenceDirectory=str(LANE),modelExecutions=0,ciActions=0)))
sys.exit(0 if receipt["allRequestedChecksPassed"] and retention["retainedWithinDeclaredTotal"] else 1)

