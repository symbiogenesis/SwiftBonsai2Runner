"""One aggregate clock for the declared corrected-source workload; no retries."""
import ctypes
import datetime as dt
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def now():
    return dt.datetime.now(dt.timezone.utc)


def write(name, value):
    (ROOT / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("flags", ctypes.c_ubyte),
                    ("percent", ctypes.c_ubyte), ("saver", ctypes.c_ubyte),
                    ("life", ctypes.c_uint32), ("full", ctypes.c_uint32)]
    value = Status()
    observed = {"observedUtc": now().isoformat(), "source": "GetSystemPowerStatus",
                "ac_online": None, "battery_saver": None, "battery_percent": None}
    if ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(value)):
        observed.update(ac_online=None if value.ac == 255 else value.ac == 1,
                        battery_saver=value.saver == 1,
                        battery_percent=None if value.percent == 255 else value.percent)
    return observed


if __name__ == "__main__":
    action = sys.argv[1]
    if action == "init":
        assert not (ROOT / "clock.json").exists(), "Do not restart this clock."
        plan = json.loads((ROOT / "workload-plan.json").read_text())
        start = now()
        write("clock.json", {"declarationUtc": start.isoformat(),
              "deadlineUtc": (start + dt.timedelta(seconds=plan["remainingWorkAllowanceSeconds"])).isoformat(),
              "earlierReadOnlyPreparationChargeSeconds": plan["earlierReadOnlyPreparationChargeSeconds"],
              "maximumAggregateSeconds": plan["maximumAggregateSeconds"],
              "historicalDeadlinesReopened": False})
        write("power-before-preparation.json", power())
        print((ROOT / "clock.json").read_text())
    elif action == "power":
        value = power()
        write(sys.argv[2], value)
        print(json.dumps(value))
    elif action == "run":
        name, seconds, cwd = sys.argv[2], int(sys.argv[3]), sys.argv[4]
        command = sys.argv[5:]
        assert not (ROOT / (name + ".json")).exists(), "Do not retry this step."
        clock = json.loads((ROOT / "clock.json").read_text())
        deadline = dt.datetime.fromisoformat(clock["deadlineUtc"])
        assert (deadline - now()).total_seconds() >= seconds + 60, "No room for step and retention."
        start, before, timed_out = now(), power(), False
        with (ROOT / (name + ".stdout")).open("wb") as out, (ROOT / (name + ".stderr")).open("wb") as err:
            process = subprocess.Popen(command, cwd=cwd, stdout=out, stderr=err,
                                       creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                code = process.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                timed_out = True
                subprocess.run(["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
                               stdout=err, stderr=err, timeout=15,
                               creationflags=subprocess.CREATE_NO_WINDOW)
                code = process.wait(timeout=15)
        report = {"startedUtc": start.isoformat(), "finishedUtc": now().isoformat(),
                  "elapsedSeconds": (now() - start).total_seconds(), "exitCode": code,
                  "timedOut": timed_out, "powerBefore": before, "powerAfter": power(),
                  "command": command, "workingDirectory": cwd}
        write(name + ".json", report)
        print(json.dumps(report))
        raise SystemExit(code)
