import json
import shutil
from session import ROOT,EVIDENCE,DECLARED,now,pin

for name in ['power-after-retention.json']:
    shutil.copy2(ROOT/name,EVIDENCE/name)
shutil.copy2(ROOT/'runner-target/aarch64-pc-windows-msvc/release/bonsai-tools.exe',ROOT/'model-bundle/bonsai-tools-corrected.exe')
summary=json.loads((EVIDENCE/'summary.json').read_text())
assert summary['validation']['runnerOfflineReplayTests']=='passed'
assert summary['validation']['correctedRunnerClippy']=='passed'
assert json.loads((ROOT/'runner-corrected-release-build.json').read_text())['exitCode']==0
summary.update({
 'outcome':'scoped-native-numerical-control-and-corrected-offline-validation-passed',
 'originalSupervisorPassed':False,
 'correctedOfflineValidatorPassed':True,
 'correctedSupervisorNewDeviceRunPerformed':False,
 'localPrismMainAfterIntegration':'1e0d6b4',
 'localRunnerMainAfterImplementationIntegration':'fb573b79',
 'correctedRunnerBuildRevision':'44a2acef4b470423d2d44eb722a69d7d28b1b27e',
 'correctedRunnerExecutable':pin(ROOT/'model-bundle/bonsai-tools-corrected.exe'),
 'originalCaptureRunnerExecutable':pin(ROOT/'model-bundle/bonsai-tools-original-capture.exe'),
 'retainedExecutableDirectory':str(ROOT/'model-bundle'),
 'powerAfterRetention':json.loads((ROOT/'power-after-retention.json').read_text()),
 'powerInterpretation':'AC observed at CPU/native arm endpoints, saver off; CPU charge62->62%, native62->63%. First preparation observation was AC58%. A later post-device archival snapshot observed battery operation65%, saver off. Thus the observed power source changed after the device capture; endpoint snapshots do not establish uninterrupted stability. No performance/energy comparison is qualified.',
 'retainedUtc':now().isoformat(),
 'chargedThroughRetentionSeconds':600+(now()-DECLARED).total_seconds()
})
summary['validation']['correctedRunnerReleaseBuildPassed']=True
(EVIDENCE/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
failure={'schemaVersion':1,'kind':'pre-device-integration-source-guard-rejection','deviceStarted':False,'observedExactTimestamp':'unknown','reason':'The preparer initially required raw byte equality between built worktree and cherry-picked main, rejecting six CRLF/LF differences. All were independently confirmed to differ only in line endings. The exact built-worktree closure and supervisor raw-byte guard were never changed. No model/driver workload had begun.','affectedFiles':json.loads((ROOT/'intake.json').read_text())['mainVsBuildLineEndingDifferences']}
(EVIDENCE/'preparation-guard-rejection.json').write_text(json.dumps(failure,indent=2)+'\n')
print(json.dumps({'outcome':summary['outcome'],'chargeSeconds':summary['chargedThroughRetentionSeconds'],'powerAfterRetention':summary['powerAfterRetention'],'correctedRunnerExecutable':summary['correctedRunnerExecutable']}))
