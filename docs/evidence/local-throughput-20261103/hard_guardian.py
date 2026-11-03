"""Hard termination backstop for the same original trial, never a new attempt."""
import ctypes
from ctypes import wintypes
from datetime import datetime
import json
import time
from intake import ROOT, now, power, save

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel.OpenProcess.restype = wintypes.HANDLE
kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
kernel.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
events = []
limits = {"rust-pq2":125, "rust-ptq1":125, "publisher-pq2":80, "publisher-ptq1":80, "qwen":100}
while True:
    session = json.loads((ROOT / "session.json").read_text(encoding="utf-8"))
    original = session["startedMonotonic"]
    for sample in session.get("samples", []):
        pid = sample.get("brokerPid")
        if not pid or sample.get("brokerExitCode") is not None:
            continue
        offset = (datetime.fromisoformat(sample["startedUtc"]) - datetime.fromisoformat(session["startedUtc"])).total_seconds()
        cutoff = min(original+offset+limits[sample["id"]], original+session["maximumSeconds"]-20)
        if time.monotonic() < cutoff:
            continue
        handle = kernel.OpenProcess(0x1001, False, pid)
        if handle:
            try:
                image = ctypes.create_unicode_buffer(32768)
                size = wintypes.DWORD(len(image))
                matched = kernel.QueryFullProcessImageNameW(handle,0,image,ctypes.byref(size)) and image.value.casefold() == session["broker"]["path"].casefold()
                if matched:
                    terminated = bool(kernel.TerminateProcess(handle,124))
                    events.append(dict(utc=now(),elapsedSeconds=time.monotonic()-original,
                                       arm=sample["id"],brokerPid=pid,terminated=terminated))
                    save("hard-guardian-events.json",events)
            finally:
                kernel.CloseHandle(handle)
    if session["status"] in {"completed","stopped"} or time.monotonic() > original+session["maximumSeconds"]:
        break
    time.sleep(.2)
save("hard-guardian.json",dict(events=events,closedUtc=now(),powerAfter=power(),
                              elapsedSecondsThroughRetention=time.monotonic()-original,
                              withinDeclaredBudget=time.monotonic()-original <= session["maximumSeconds"]))
print(json.dumps(dict(events=events,status=session["status"],elapsedSeconds=time.monotonic()-original)))
