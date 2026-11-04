import ctypes
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).parent
EVIDENCE = Path(r'C:\Users\symbi\source\repos\SwiftBonsai2Runner\.worktrees\runner-hmx-session-evidence\docs\evidence\hexagon-hmx-session-20261103')
DECLARED = dt.datetime.fromisoformat('2026-11-04T04:02:41.962933+00:00')
DEADLINE = DECLARED + dt.timedelta(seconds=3000)

def now():
    return dt.datetime.now(dt.timezone.utc)

def write(name, data):
    (ROOT / name).write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')

def power():
    class Status(ctypes.Structure):
        _fields_ = [('ac',ctypes.c_ubyte),('flags',ctypes.c_ubyte),('percent',ctypes.c_ubyte),('saver',ctypes.c_ubyte),('life',ctypes.c_uint32),('full',ctypes.c_uint32)]
    value = Status()
    if not ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(value)):
        return {'status':'unknown','ac_online':None,'battery_saver':None,'battery_percent':None,'observedUtc':now().isoformat()}
    return {'status':'observed','source':'GetSystemPowerStatus','ac_online':None if value.ac==255 else value.ac==1,'battery_saver':value.saver==1,'battery_percent':None if value.percent==255 else value.percent,'observedUtc':now().isoformat()}

def pin(path):
    path = Path(path)
    return {'bytes':path.stat().st_size,'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest()}

if __name__ == '__main__':
    action=sys.argv[1]
    if action=='init':
        write('clock.json',{'declarationUtc':DECLARED.isoformat(),'deadlineUtc':DEADLINE.isoformat(),'earlierPreparationChargeSeconds':600,'maximumAggregateSeconds':3600,'historicalDeadlinesReopened':False})
        write('power-before-preparation.json',power())
        print(json.dumps({'deadlineUtc':DEADLINE.isoformat(),'power':power()}))
    elif action=='power':
        status=power(); write(sys.argv[2],status); print(json.dumps(status))
    elif action=='receipt':
        source=Path(sys.argv[2]); exe=Path(sys.argv[3]); compiler=Path(sys.argv[4]); revision=sys.argv[5]
        names=set(['Cargo.toml','Cargo.lock'])
        for name in ['build.rs','rust-toolchain.toml','.cargo/config.toml']:
            if (source/name).is_file(): names.add(name)
        for base in ['src','tools']:
            for parent,dirs,files in os.walk(source/base):
                dirs[:]=[d for d in dirs if d not in ['target','.git','.worktrees']]
                for filename in files: names.add((Path(parent)/filename).relative_to(source).as_posix())
        sources=[{'path':name,**pin(source/name)} for name in sorted(names)]
        receipt={'schemaVersion':1,'kind':'local-prism-hexagon-model-probe-build','hostedBuild':False,'buildSucceeded':True,'cargoLocked':True,'target':'aarch64-pc-windows-msvc','prismRevision':revision,'compiler':{'path':str(compiler),**pin(compiler)},'executable':pin(exe),'sources':sources}
        write('local-model-build.json',receipt)
        print(json.dumps({'sourceFiles':len(sources),'executable':receipt['executable']}))
    elif action=='run':
        name=sys.argv[2]; seconds=int(sys.argv[3]); cwd=sys.argv[4]; command=sys.argv[5:]
        if (DEADLINE-now()).total_seconds()<seconds+60: raise SystemExit('aggregate allowance cannot fit command and retention reserve')
        start=now(); before=power(); timed_out=False
        with (ROOT/(name+'.stdout')).open('wb') as out,(ROOT/(name+'.stderr')).open('wb') as err:
            proc=subprocess.Popen(command,cwd=cwd,stdout=out,stderr=err,creationflags=subprocess.CREATE_NO_WINDOW)
            try: code=proc.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                timed_out=True
                subprocess.run(['taskkill.exe','/PID',str(proc.pid),'/T','/F'],stdout=err,stderr=err,creationflags=subprocess.CREATE_NO_WINDOW,timeout=15)
                code=proc.wait(timeout=15)
        report={'startedUtc':start.isoformat(),'finishedUtc':now().isoformat(),'elapsedSeconds':(now()-start).total_seconds(),'exitCode':code,'timedOut':timed_out,'powerBefore':before,'powerAfter':power(),'command':command,'workingDirectory':cwd}
        write(name+'.json',report); print(json.dumps(report)); raise SystemExit(code)
