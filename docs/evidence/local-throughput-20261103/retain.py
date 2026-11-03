"""Retain existing outputs and finish owned cleanup inside the original budget."""
import ctypes
from ctypes import wintypes
import hashlib
import json
from pathlib import Path
import time
import winreg
from intake import ROOT, now, power, processes, save

session = json.loads((ROOT / "session.json").read_text(encoding="utf-8"))
if (ROOT / "retention.json").exists():
    raise SystemExit("Retention has already closed; do not repeat this workload.")
original = session["startedMonotonic"]
scratch = Path(session["scratchDirectory"]).resolve()
if not scratch.name.startswith("Bonsai-Local-Throughput-20261103-"):
    raise SystemExit("Unexpected trial scratch root")
if time.monotonic() >= original + session["maximumSeconds"]:
    raise SystemExit("Original allowance exhausted; do not start further evidence work")
report = dict(startedUtc=now(), powerBeforeFinalCleanup=power(),
              modelAttemptsRepeated=0, stoppedBeforePublisher=True,
              qwenStatus="not-installed-in-selected-data-directory", retained=[], ownedTermination=[])
save("retention.json", report)
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.OpenProcess.argtypes = [wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
kernel.OpenProcess.restype = wintypes.HANDLE
kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE,wintypes.DWORD,wintypes.LPWSTR,ctypes.POINTER(wintypes.DWORD)]
kernel.TerminateProcess.argtypes = [wintypes.HANDLE,wintypes.UINT]
kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE,wintypes.DWORD]
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
recorded = {p["Id"] for sample in session["samples"] for p in sample.get("activeModelProcessesAfter", [])}
for pid in recorded:
    handle = kernel.OpenProcess(0x101001,False,pid)
    item = dict(pid=pid,opened=bool(handle))
    if handle:
        try:
            image = ctypes.create_unicode_buffer(32768)
            count = wintypes.DWORD(len(image))
            observed = bool(kernel.QueryFullProcessImageNameW(handle,0,image,ctypes.byref(count)))
            expected = Path(session["dataDirectory"]) / "inference" / "9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57" / "aarch64-pc-windows-msvc" / "prism-inference-server.exe"
            item["expectedImageMatched"] = observed and image.value.casefold() == str(expected).casefold()
            if item["expectedImageMatched"]:
                item["terminationRequested"] = bool(kernel.TerminateProcess(handle,124))
                item["waitResult"] = kernel.WaitForSingleObject(handle,3000)
        finally:
            kernel.CloseHandle(handle)
    report["ownedTermination"].append(item)
report["activeModelProcessesAfterFinalCleanup"] = processes()
for sample in session["samples"]:
    arm = scratch / sample["id"]
    for path in arm.rglob("*"):
        if path.is_file() and path.name in {"backend.stdout", "backend.stderr", "stdout.txt", "stderr.txt", "backend.stdout.log", "backend.stderr.log"}:
            contents = path.read_bytes()
            dest = ROOT / (sample["id"] + "-" + path.name)
            dest.write_bytes(contents)
            report["retained"].append(dict(path=dest.name,bytes=len(contents),sha256=hashlib.sha256(contents).hexdigest()))
    # Remove only this workload's fixed credential files after all owned models
    # are gone. No recursive delete or inference from later power snapshots.
    if not report["activeModelProcessesAfterFinalCleanup"]:
        for path in arm.rglob("*.key"):
            resolved = path.resolve()
            if scratch not in resolved.parents or resolved.name not in {"frontend.key","backend.key"}:
                raise RuntimeError("Unexpected credential deletion target")
            resolved.unlink()
report["remainingTrialCredentialFiles"] = len(list(scratch.rglob("*.key")))
repo = ROOT.parents[2]
lock = json.loads((repo / "config" / "prism-inference.lock.json").read_text(encoding="utf-8"))
runtime = Path(session["dataDirectory"]) / "inference" / lock["commit"] / lock["target"]
report["runtime"] = dict(version=lock["version"], sourceRevision=lock["commit"], files=[])
for expected in lock["files"]:
    path = runtime / expected["name"]
    contents = path.read_bytes()
    actual = dict(name=expected["name"],bytes=len(contents),sha256=hashlib.sha256(contents).hexdigest())
    actual["matchesPin"] = actual["bytes"] == expected["bytes"] and actual["sha256"] == expected["sha256"]
    report["runtime"]["files"].append(actual)
with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
    report["processorName"] = winreg.QueryValueEx(key,"ProcessorNameString")[0].strip()
report["powerAfterFinalRetention"] = power()
report["elapsedSecondsThroughRetention"] = time.monotonic()-original
report["withinDeclaredBudget"] = report["elapsedSecondsThroughRetention"] <= session["maximumSeconds"]
report["closedUtc"] = now()
save("retention.json",report)
print(json.dumps(report,indent=2))
