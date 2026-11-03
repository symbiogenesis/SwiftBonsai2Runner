"""Independent deadline enforcement sharing the already-running original clock."""
import ctypes
from datetime import datetime
import json
import subprocess
import sys
import time
from intake import ROOT, now, power, save

events = []
signalled = set()
limits = {"rust-pq2":125, "rust-ptq1":125, "publisher-pq2":80, "publisher-ptq1":80, "qwen":100}
code = """import ctypes,sys,time
k=ctypes.windll.kernel32
k.FreeConsole()
if not k.AttachConsole(int(sys.argv[1])): sys.exit(2)
k.SetConsoleCtrlHandler(None,True)
ok=k.GenerateConsoleCtrlEvent(0,0)
time.sleep(.3)
sys.exit(0 if ok else 3)
"""
while True:
    session = json.loads((ROOT / "session.json").read_text(encoding="utf-8"))
    original = session["startedMonotonic"]
    if session["status"] in {"completed", "stopped"}:
        break
    for sample in session.get("samples", []):
        pid = sample.get("brokerPid")
        if not pid or sample["status"] not in {"startup", "request"} or pid in signalled:
            continue
        offset = (datetime.fromisoformat(sample["startedUtc"]) -
                  datetime.fromisoformat(session["startedUtc"])).total_seconds()
        cutoff = min(original + offset + limits[sample["id"]] - 12,
                     original + session["maximumSeconds"] - 30)
        if time.monotonic() >= cutoff:
            result = subprocess.run([sys.executable, "-c", code, str(pid)], timeout=3,
                                    capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
            events.append(dict(utc=now(), elapsedSeconds=time.monotonic()-original,
                               arm=sample["id"], brokerPid=pid, consoleStopExitCode=result.returncode))
            signalled.add(pid)
            save("guardian-events.json", events)
    if time.monotonic() >= original + session["maximumSeconds"]:
        events.append(dict(utc=now(), reason="original-hard-deadline-reached"))
        break
    time.sleep(.25)
save("guardian.json", dict(events=events, closedUtc=now(), powerAfter=power(),
                           elapsedSecondsThroughRetention=time.monotonic()-original,
                           withinDeclaredBudget=time.monotonic()-original <= session["maximumSeconds"]))
print(json.dumps(dict(events=events, status=session["status"], elapsedSeconds=time.monotonic()-original)))
