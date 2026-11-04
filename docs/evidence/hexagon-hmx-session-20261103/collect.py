import json
import shutil
from pathlib import Path
from session import ROOT,EVIDENCE,DECLARED,now,pin,power,write

EVIDENCE.mkdir(parents=True,exist_ok=True)
for path in ROOT.iterdir():
    if path.is_file() and path.suffix in ['.json','.stdout','.stderr','.py','.ps1']:
        shutil.copy2(path,EVIDENCE/path.name)
shutil.copytree(ROOT/'two-token-device',EVIDENCE/'two-token-device',dirs_exist_ok=True)
report=json.loads((ROOT/'two-token-device/model-device.json').read_text())
events=[json.loads(line) for line in (ROOT/'two-token-device/model-probe.txt').read_text().splitlines()]
result=events[-1]
assert result['event']=='result' and result['integerValuesCompared']==11141120
assert result['nativeOperationsCompleted']==result['nativeOperationsSubmitted']==80
assert all(row['stateBitwiseEqual'] and row['logitsBitwiseEqual'] for row in result['tokenComparisons'])
assert report['OwnedJobTeardown']=='confirmed-empty'
assert report['Failure']=='The model result has incomplete or contradictory native cleanup.'
summary={
 'schemaVersion':1,'kind':'scoped-live-X2-HMX-two-token-control-with-preserved-supervisor-rejection',
 'dateLocal':'2026-11-03',
 'outcome':'native-numerical-control-passed-original-supervisor-rejected-offline-review-pending',
 'authorization':'Owner session-specific aggregate duration extension; historical workloads remain closed.',
 'maximumAggregateSeconds':3600,'earlierReadOnlyPreparationChargeSeconds':600,
 'declarationUtc':DECLARED.isoformat(),'retainedUtc':now().isoformat(),
 'chargedThroughRetentionSeconds':600+(now()-DECLARED).total_seconds(),
 'deviceAttempts':1,'deviceRetried':False,
 'supervisorElapsedMilliseconds':report['ElapsedMilliseconds'],
 'elapsedIsWholeSupervisorNotKernelTiming':True,
 'originalSupervisorOutcome':report['Outcome'],'originalSupervisorFailure':report['Failure'],
 'originalSupervisorReceiptPreservedUnchanged':True,
 'rejectionCause':'New result validator expected ownerPinnedUntilExit=false. Native QueueComputeReport::completed_cycle requires true because driver/module/catalog owners stay pinned through process exit, independently of allocation free. The new validator was wrong; raw device computation and cleanup completed.',
 'offlineCorrection':'Correct validator to require true and replay the exact retained terminal event in a model-free regression; no new model or device run.',
 'probeBuildPrismRevision':'38ff4b974c9f406404a72939dee38e94b8c74939',
 'probeExecutableSha256':report['ExecutableSha256'],
 'sourceFilesVerified':report['LocalBuildSourceFilesVerified'],
 'modelSha256':result['modelSha256'],'modelBytes':result['modelBytes'],
 'tokens':result['tokenIds'],'projection':result['projection'],'executionBackend':result['executionBackend'],
 'nativeHmxBitplaneControl':result['nativeHmxBitplaneControl'],
 'nativeOperationsSubmitted':result['nativeOperationsSubmitted'],
 'nativeOperationsCompleted':result['nativeOperationsCompleted'],
 'integerValuesCompared':result['integerValuesCompared'],
 'tokenComparisons':result['tokenComparisons'],
 'logitsBitwiseEqual':result['logitsBitwiseEqual'],'stateBitwiseEqual':result['stateBitwiseEqual'],
 'nativeCleanup':result['nativeCleanup'],'nativeResources':result['nativeResources'],
 'nativeCompletedCycle':result['nativeCompletedCycle'],'ownedJobTeardown':report['OwnedJobTeardown'],
 'signedPackageVerified':report['SignedPackageVerified'],'runtimeFilesVerified':report['RuntimeFilesVerified'],
 'catalogMembersVerified':report['CatalogMembersVerified'],'driverIdentity':report['DriverIdentity'],
 'availablePhysicalBytesBefore':report['AvailablePhysicalBytesBefore'],
 'cpuPowerBefore':result['cpuPowerBefore'],'cpuPowerAfter':result['cpuPowerAfter'],
 'nativePowerBefore':result['npuPowerBefore'],'nativePowerAfter':result['npuPowerAfter'],
 'powerBeforePreparation':json.loads((ROOT/'power-before-preparation.json').read_text()),
 'powerAfterDevice':json.loads((ROOT/'device-supervisor.json').read_text())['powerAfter'],
 'earlierReadOnlyPreparationPower':'unknown',
 'powerInterpretation':'AC observed at both CPU and native arm endpoints; CPU charge62->62%, native62->63%, saver off. Preparation snapshot58%. Equal endpoints do not establish uninterrupted stability. No performance/energy comparison.',
 'preparationGuardFailure':'Before device work, a raw main/worktree equality assertion rejected six CRLF/LF differences. Independent exact closure review confirms only line endings differ; the supervisor still verified and held every built-worktree raw-byte hash. The guard and correction caused no device retry.',
 'validation':{'prismFocusedTestsPassed':9,'runnerFocusedTestsPassedBeforeCleanupCorrection':5,'runnerOfflineReplayTests':'pending','prismSelectedStrictClippyPassed':True,'runnerSelectedStrictClippyBeforeCorrectionPassed':True,'correctedRunnerClippy':'pending','releaseBuildsPassed':True,'completeSuitesExecuted':False,'initialBroadPrismClippy':'failed on existing feature-disabled prefill_dot::new missing_const_for_fn','initialSelectedPrismClippy':'failed on new unsafe as conversion; fixed with exact i16/f32 conversion; corrected selected target passed'},
 'qualificationLimits':['Only blk.0.ffn_up.weight for tokens42,43; every other projection and all transforms/recurrent state remain CPU-owned.','Scalar CPU control only; four-worker/NEON default and nonzero adapter are untested here.','HMX is the source-selected signed bitplane control; effective hardware placement and VTCM remain unverified.','The source-bound worker compares every plane/logit/state value; complete raw numerical arrays are not retained for independent offline numerical replay.','Explicit operation cleanup and empty Job do not establish complete vendor-session teardown or daily-service isolation.','This is a terminal two-token correctness control, not persistent serving or a performance comparison.'],
 'normalChatInference':'CPU','productionNpuAdmissionEnabled':False,'automaticBackendSelectionEnabled':False,
 'performanceMeasured':False,'measuredInferenceSpeedup':None,'adapterLoaded':False,
 'remainingPromotionGates':['Broader live activations and quantizer edges plus standard adapter at scale1.3.','Effective selected DSP/placement/VTCM/DMA resources and daily-service memory/isolation.','Wider placement, full prompt/output admission, prefix reuse/edit/cancellation/reset and persistent serving.','Efficient fused packed kernels and reusable device weights with a supported signing/distribution route.','Matched same-artifact complete-answer benefit and sustained quality/comfort.','Released explicit profile and installation/rollback acceptance.'],
 'ciDispatchedOrMonitored':False,'pushed':False,'independentReview':'pending'
}
for name,key in [('runner-offline-replay-tests','runnerOfflineReplayTests'),('runner-clippy-corrected','correctedRunnerClippy')]:
    path=ROOT/(name+'.json')
    if path.exists(): summary['validation'][key]='passed' if json.loads(path.read_text())['exitCode']==0 else 'failed'
(EVIDENCE/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'evidenceDirectory':str(EVIDENCE),'copiedFiles':sum(1 for p in EVIDENCE.rglob('*') if p.is_file()),'originalSupervisorOutcome':summary['originalSupervisorOutcome'],'numericalControl':'passed','offlineReplay':summary['validation']['runnerOfflineReplayTests']}))
