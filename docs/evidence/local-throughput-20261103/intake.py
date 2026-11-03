"""One local owner-authorized capture; all later work uses this original clock."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent

def now():
    return datetime.now(timezone.utc).isoformat()

def save(name, value):
    (ROOT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")

def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                    ("percent", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                    ("life", ctypes.c_ulong), ("full", ctypes.c_ulong)]
    status = Status()
    if not ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        return dict(utc=now(), acConnected=None, batterySaver=None, chargePercent=None)
    return dict(utc=now(), acConnected={0: False, 1: True}.get(status.ac),
                batterySaver=bool(status.saver),
                chargePercent=None if status.percent == 255 else status.percent)

def processes():
    command = "Get-Process | Where-Object { $_.ProcessName -match 'bonsai|prism|llama|ollama|lm.?studio|qwen|canary|vllm|kobold' } | Select-Object Id,ProcessName | ConvertTo-Json -Compress"
    result = subprocess.run(["powershell.exe", "-NoProfile", "-Command", command],
                            capture_output=True, text=True, timeout=12,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        raise RuntimeError("Active-model process observation failed")
    value = json.loads(result.stdout) if result.stdout.strip() else []
    return value if isinstance(value, list) else [value]

if __name__ == "__main__":
    if (ROOT / "session.json").exists():
        raise SystemExit("This trial already has an original clock; never restart it.")
    session = dict(schemaVersion=1, kind="owner-authorized-local-request-throughput",
                   startedUtc=now(), startedMonotonic=time.monotonic(), maximumSeconds=720,
                   conservativeEstimateSeconds=690,
                   estimate=dict(setup=120, rustOwners=250, publisherOwners=160,
                                 qwenOwner=100, cleanup=40, retention=20),
                   reason="Five fresh model owners and complete setup/cleanup/retention do not conservatively fit five or ten minutes.",
                   authorization="User explicitly requested measurements on this laptop; no CI dispatch or monitoring.",
                   fixedOrder=["rust-pq2", "rust-ptq1", "publisher-pq2", "publisher-ptq1", "qwen"],
                   prompt="Count from 1 to 100.", completionCap=8, thinking=False, seed=42,
                   noRetries=True, noBuildsOrDownloads=True, status="intake")
    save("session.json", session)
    session["powerBefore"] = power()
    session["activeModelProcessesBefore"] = processes()
    data = Path(os.environ["LOCALAPPDATA"]) / "SwiftBonsai2"
    session["dataDirectory"] = str(data)
    inventory = []
    for directory, children, files in os.walk(data):
        # Exclude prior qualification archives and source trees; inspect installed delivery only.
        children[:] = [name for name in children if name not in
                       {"qualification", "source", "sources", "src", ".git", "deps", "incremental", ".fingerprint", "build-script-build"}]
        for name in files:
            if name.endswith((".gguf", ".zip")) or name in {"bonsai-tools.exe", "bonsai-broker.exe", "prism-inference-server.exe", "llama-server.exe"} or (name.endswith(".json") and "receipt" in name):
                path = Path(directory) / name
                item = dict(path=str(path), bytes=path.stat().st_size)
                if name == "bonsai-broker.exe":
                    contents = path.read_bytes()
                    item.update(sha256=hashlib.sha256(contents).hexdigest(),
                                privateShutdownSupported=b"--owner-shutdown-directory" in contents,
                                publisherSelectionSupported=b"--publisher-engine" in contents)
                inventory.append(item)
    save("installed-inventory.json", inventory)
    session["intakeSeconds"] = time.monotonic() - session["startedMonotonic"]
    save("session.json", session)
    print(json.dumps(dict(session=session, inventory=inventory), indent=2))
