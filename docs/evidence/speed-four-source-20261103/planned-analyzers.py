"""Collect planned analyzer findings while the original source session stays open."""
import ctypes
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys
import time

out = Path(__file__).resolve().parent
original = json.loads((out / "receipt.json").read_text())
steps = json.loads((out / "checks.json").read_text())
start = datetime.datetime.fromisoformat(original["startedAtUtc"])

class Power(ctypes.Structure):
    _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                ("charge", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                ("life", ctypes.c_uint32), ("full", ctypes.c_uint32)]

def power():
    status = Power()
    result = dict(observedAtUtc=datetime.datetime.now(datetime.UTC).isoformat(),
                  acConnected=None, batterySaver=None, chargePercent=None)
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        result.update(acConnected={0: False, 1: True}.get(status.ac),
                      batterySaver={0: False, 1: True}.get(status.saver),
                      chargePercent=status.charge if status.charge <= 100 else None)
    return result

records = []
for step in steps:
    if not step["name"].endswith("clippy"):
        continue
    if len(sys.argv) > 1 and step["name"] != sys.argv[1]:
        continue
    elapsed = (datetime.datetime.now(datetime.UTC) - start).total_seconds()
    if original.get("clockClosed") or elapsed > 470:
        break
    env = os.environ.copy()
    env.update(step.get("environment", {}))
    if len(sys.argv) > 2:
        env["CARGO_TARGET_DIR"] = sys.argv[2]
    compiler = step["compiler"]
    env.update(PATH=compiler + os.pathsep + env["PATH"], RUSTC=str(Path(compiler) / "rustc.exe"),
               RUSTDOC=str(Path(compiler) / "rustdoc.exe"), CARGO_BUILD_JOBS="4",
               CARGO_NET_OFFLINE="true", CARGO_BUILD_FINGERPRINT="content",
               CARGO_UNSTABLE_CHECKSUM_FRESHNESS="true", CARGO_TERM_COLOR="never")
    entry = dict(name=step["name"], powerBefore=power(), originalSessionElapsedBefore=elapsed)
    began = time.monotonic()
    with (out / (step["name"] + "-diagnostic.log")).open("wb") as log:
        process = subprocess.Popen(step["arguments"], cwd=step["cwd"], env=env,
                                   stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            entry["exitCode"] = process.wait(timeout=min(180 if len(sys.argv) > 1 else 100, 550 - elapsed))
        except subprocess.TimeoutExpired:
            subprocess.run([r"C:\Windows\System32\taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                           stdout=log, stderr=log, timeout=10, check=False,
                           creationflags=subprocess.CREATE_NO_WINDOW)
            process.wait(timeout=5)
            entry["status"] = "pending-timeout"
    entry.update(powerAfter=power(), seconds=time.monotonic()-began)
    records.append(entry)
    receipt_name = f"planned-{sys.argv[1]}.json" if len(sys.argv) > 1 else "planned-analyzers.json"
    (out / receipt_name).write_text(json.dumps(records, indent=2)+"\n")
    print(json.dumps(entry), flush=True)
