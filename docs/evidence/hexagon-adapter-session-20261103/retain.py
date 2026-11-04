from pathlib import Path
import json
import shutil
import subprocess
from session import ROOT, now, power, pin, write, DECLARED

repo = Path(r'C:\Users\symbi\source\repos\SwiftBonsai2Runner')
destination = repo/'docs/evidence/hexagon-adapter-session-20261103'
git = Path.home()/'AppData/Local/Microsoft/WinGet/Packages/Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe/cmd/git.exe'

def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))

def revision(directory):
    return subprocess.check_output([str(git),'-C',str(directory),'rev-parse','HEAD'],text=True).strip()

write('power-after-retention.json', power())
report = read(ROOT/'standard-adapter-device/model-device.json')
events = report['ProbeEvents']
cpu = next(event for event in events if event['event']=='cpu-arm-finished')
assert report['Outcome']=='pending' and report['OwnedJobTeardown']=='confirmed-empty'
assert cpu['completed'] is False
assert not any(event['event'].startswith('native-') or event['event']=='result' for event in events)
assert 'NativeResult' not in report
checks = {}
for path in sorted(ROOT.glob('*.json')):
    value = read(path)
    if isinstance(value,dict) and 'exitCode' in value:
        checks[path.stem] = {key:value[key] for key in ['exitCode','timedOut','elapsedSeconds']}
clock = read(ROOT/'clock.json')
intake = read(ROOT/'intake.json')
for path in sorted(ROOT.iterdir()):
    if path.is_file() and path.suffix in ['.json','.stdout','.stderr','.ps1','.py']:
        shutil.copy2(path,destination/path.name)
raw_destination = destination/'standard-adapter-device'
raw_destination.mkdir(exist_ok=True)
for path in sorted((ROOT/'standard-adapter-device').iterdir()):
    assert path.is_file()
    shutil.copy2(path,raw_destination/path.name)
closed = now()
close_power = power()
close_record = {'schemaVersion':1,'closedUtc':closed.isoformat(),'maximumAggregateSeconds':3600,'earlierPreparationChargeSeconds':600,'chargedSeconds':600+(closed-DECLARED).total_seconds(),'deviceControlAttempts':1,'deviceRetried':False,'nativeArmObserved':False,'ownedJobTeardown':report['OwnedJobTeardown'],'powerAtClose':close_power,'historicalDeadlinesReopened':False}
write('workload-close.json',close_record)
shutil.copy2(ROOT/'workload-close.json',destination/'workload-close.json')
summary = {
    'schemaVersion':1,'kind':'npu-adapter-preparation-with-failed-cpu-reference-control',
    'dateLocal':'2026-11-03','outcome':'preparation-passed-model-control-failed-before-native-arm',
    'authorization':'Owner session-specific aggregate duration extension; historical workloads remain closed.',
    **clock,'retainedUtc':closed.isoformat(),
    'chargedThroughRetentionSeconds':600+(closed-DECLARED).total_seconds(),
    'powerAtWorkloadClose':close_power,
    'deviceControlAttempts':1,'deviceRetried':False,
    'supervisorElapsedMilliseconds':report['ElapsedMilliseconds'],
    'supervisorProcessExitCode':read(ROOT/'device-supervisor.json')['exitCode'],
    'supervisorOutcome':report['Outcome'],'supervisorFailure':report['Failure'],
    'originalWorkerExitCode':None,
    'originalWorkerExitCodeObservation':'Not serialized by the original supervisor. The child emitted an error event; outer exit zero is not acceptance.',
    'observedEventSequence':[event['event'] for event in events],
    'cpuReferenceCompleted':False,'nativeArmObserved':False,'nativeOperationsObserved':None,
    'adapterDeviceAcceptancePassed':False,'nativeParityPassed':False,
    'ownedJobTeardown':report['OwnedJobTeardown'],
    'intendedControl':{'tokenIds':[42,43,128,256],'adapterCase':'standard-scale-1.3','adapterScale':1.3,'cpuBackend':'neon','rowWorkers':4,'executionBackend':'hmx-bitplane','projection':'blk.0.ffn_up.weight'},
    'staticPacketInfeasibility':{
        'kind':'pinned-geometry-source-derivation-not-an-observed-error-code',
        'recurrentAndConvolutionBytesPerSnapshot':156893184,
        'logicalF32KvBytesPerCommittedToken':131072,
        'snapshotBytesFormula':'n * 156893184 + 131072 * n * (n + 1) / 2',
        'fourSnapshotBytesRequired':628883456,'fixedSnapshotBudgetBytes':536870912,
        'excessBytes':92012544,'threeSnapshotBytesRequired':471465984,
        'interpretation':'Four mandatory snapshots cannot fit. The generic original error does not prove which earlier graph, adapter, evaluation or capture step actually threw. No retry or error receipt rewrite was performed.'
    },
    'capturedSource':{'prismRevision':intake['buildSourcePrismRevision'],'runnerRevision':intake['buildSourceRunnerRevision'],'probeExecutable':intake['probeExecutable'],'runnerExecutable':intake['runnerExecutable'],'sourceFilesVerified':report['LocalBuildSourceFilesVerified']},
    'localMainAfterSourceIntegration':{'runner':revision(repo),'prism':revision(repo.parent/'prism-inference')},
    'signedIntake':{'runtimeFilesVerified':report['RuntimeFilesVerified'],'catalogMembersVerified':report['CatalogMembersVerified'],'driverIdentity':report['DriverIdentity'],'adapterIdentity':report['AdapterIdentity'],'availablePhysicalBytesBefore':report['AvailablePhysicalBytesBefore']},
    'validation':{'capturedSourceFocusedRunnerTestsPassed':9,'capturedSourceFocusedPrismTestsPassed':11,'postFailureSourceFocusedRunnerTestsPassed':10,'postFailureSourceFocusedPrismTestsPassed':13,'postFailureSelectedStrictClippyPassed':True,'postFailureProductionReleaseBuildPerformed':False,'postFailureModelRunPerformed':False,'initialParallelFeatureCompileFailed':True,'correctedSelectedChecksPassed':True,'malformedIntermediateBuildWrapperFailed':True,'finalCapturedReleaseBuildsPassed':True,'receipts':checks,'completeSuitesExecuted':False},
    'power':{'beforePreparation':read(ROOT/'power-before-preparation.json'),'beforeDevice':read(ROOT/'power-before-device.json'),'cpuBefore':cpu['powerBefore'],'cpuAfter':cpu['powerAfter'],'nativeBefore':None,'nativeAfter':None,'afterDevice':read(ROOT/'power-after-device.json'),'afterRetention':read(ROOT/'power-after-retention.json'),'earlierReadOnlyPreparation':'unknown','interpretation':'Battery operation observed at workload and CPU endpoints, saver off. No native arm was observed, so its power remains unknown. Equal endpoints do not establish uninterrupted stability. No performance or energy comparison is qualified.'},
    'sourceOnlyCorrection':'Pre-execution cumulative-state admission, precise CPU stage/token events and serialized child exit status are separate post-failure changes. Focused checks do not supply another model or NPU verdict.',
    'remainingPromotionGates':['Feasible exact-source loaded-zero and standard-adapter device controls plus broader numerical inputs and independent raw numerical replay.','Supported native serving lifetime, full-request reservation, prefixes, cancellation and failure recovery.','Observed selected DSP and effective VTCM/DMA/storage, accounting beyond process Job and complete vendor ownership.','Useful same-artifact end-to-end benefit and quality/comfort with matched power.','Released explicit supported application profile, installation/isolation and CPU rollback.'],
    'normalChatInference':'CPU','productionNpuAdmissionEnabled':False,'automaticBackendSelectionEnabled':False,
    'performanceMeasured':False,'measuredInferenceSpeedup':None,
    'ciDispatchedOrMonitored':False,'pushed':False,
    'retainedExecutableDirectory':str(ROOT/'model-bundle'),
    'independentReview':'independent-review.json'
}
(destination/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
index=[]
for path in sorted(destination.rglob('*')):
    if path.is_file() and path.name!='retained-input-index.json':
        index.append({'path':path.relative_to(destination).as_posix(),**pin(path)})
(destination/'retained-input-index.json').write_text(json.dumps({'schemaVersion':1,'files':index},indent=2)+'\n',encoding='utf-8')
print(json.dumps({'outcome':summary['outcome'],'chargedThroughRetentionSeconds':summary['chargedThroughRetentionSeconds'],'filesRetained':len(index),'powerAfterRetention':summary['power']['afterRetention']}))
