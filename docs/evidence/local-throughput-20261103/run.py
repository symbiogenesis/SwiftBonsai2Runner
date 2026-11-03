"""Execute each installed option once, retaining the intake's single deadline."""
import ctypes
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time

from intake import ROOT, now, power, processes, save

session = json.loads((ROOT / "session.json").read_text(encoding="utf-8"))
if session["status"] != "intake":
    raise SystemExit("A run has already been attempted; this clock cannot be retried.")
deadline = session["startedMonotonic"] + session["maximumSeconds"]
inventory = json.loads((ROOT / "installed-inventory.json").read_text(encoding="utf-8"))
broker_item = next(item for item in inventory if item.get("publisherSelectionSupported"))
broker = Path(broker_item["path"])
data = Path(session["dataDirectory"])
scratch = Path(tempfile.mkdtemp(prefix="Bonsai-Local-Throughput-20261103-"))
session.update(status="running", broker=broker_item, scratchDirectory=str(scratch), samples=[])
save("session.json", session)

def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]

def interrupt(pid):
    # Give only the broker's hidden console a normal Ctrl-C. Its owned model
    # process uses its own native containment; no unrelated console is targeted.
    code = """import ctypes,sys,time
k=ctypes.windll.kernel32
k.FreeConsole()
if not k.AttachConsole(int(sys.argv[1])): sys.exit(2)
k.SetConsoleCtrlHandler(None, True)
ok=k.GenerateConsoleCtrlEvent(0,0)
time.sleep(.3)
sys.exit(0 if ok else 3)
"""
    result = subprocess.run([sys.executable, "-c", code, str(pid)],
                            capture_output=True, timeout=3,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    return result.returncode

def measure(port, key, limit, swift):
    body = dict(messages=[dict(role="user", content=session["prompt"])],
                stream=True, max_completion_tokens=session["completionCap"],
                enable_thinking=False)
    if swift:
        body["abliteration_scale"] = 1.3
    encoded = json.dumps(body).encode()
    started = time.monotonic()
    conn = http.client.HTTPConnection("127.0.0.1", port,
                                      timeout=max(.1, limit-time.monotonic()))
    try:
        conn.request("POST", "/v1/chat/completions", body=encoded,
                     headers={"Authorization": "Bearer " + key,
                              "Content-Type": "application/json", "Connection": "close"})
        response = conn.getresponse()
        payload = response.read(2 * 1024 * 1024)
        elapsed = time.monotonic() - started
        if response.status != 200:
            raise RuntimeError("HTTP status " + str(response.status) + ": " + payload.decode(errors="replace")[:800])
    finally:
        conn.close()
    records = [line[6:] for line in payload.decode("utf-8").splitlines() if line.startswith("data: ")]
    events = [json.loads(record) for record in records if record != "[DONE]"]
    usage = next((event["usage"] for event in reversed(events) if event.get("usage")), {})
    tokens = usage.get("completion_tokens")
    finish = next((choice["finish_reason"] for event in reversed(events)
                   for choice in event.get("choices", []) if choice.get("finish_reason")), None)
    content = "".join(choice.get("delta", {}).get("content") or ""
                      for event in events for choice in event.get("choices", []))
    if "[DONE]" not in records or finish not in {"stop", "length"} or not isinstance(tokens, int) or not 1 <= tokens <= 8:
        raise RuntimeError("Missing complete SSE terminal framing or valid completion usage")
    return dict(requestSeconds=elapsed, reportedCompletionTokens=tokens,
                requestTokensPerSecond=tokens/elapsed, finishReason=finish,
                visibleContent=content, usage=usage, requestBody=body,
                responseBodySha256=hashlib.sha256(payload).hexdigest(),
                events=events, decodeTokensPerSecond=None,
                metricScope="HTTP connection/request through complete framed response read; includes prefill, generation, transport and client framing; excludes model startup")

def run_arm(name, seconds, native, ptq1, reference=False):
    sample = dict(id=name, status="pending", powerBefore=power(), startedUtc=now())
    session["samples"].append(sample)
    save("session.json", session)
    if time.monotonic() + seconds + 30 >= deadline:
        sample["reason"] = "insufficient-original-budget-for-arm-and-retention"
        sample["powerAfter"] = power()
        save("session.json", session)
        return
    if reference and not any(Path(item["path"]).name == "Qwen3.8-27B-UD-Q3_K_XL.gguf" for item in inventory):
        sample.update(status="not-installed", reason="Pinned Qwen model and runtime absent from installed data; no download attempted")
        sample["powerAfter"] = power()
        save("session.json", session)
        print(name + ": not installed", flush=True)
        return
    observed = processes()
    sample["activeModelProcessesBefore"] = observed
    if observed:
        sample["reason"] = "another-model-process-observed"
        sample["powerAfter"] = power()
        save("session.json", session)
        raise RuntimeError("Another model process is active; stop without running more arms")
    frontend, backend = free_port(), free_port()
    while backend == frontend:
        backend = free_port()
    arm = scratch / name
    arm.mkdir()
    args = [str(broker), "--launch-development", "--require-native-sandbox",
            "--data-directory", str(data), "--evidence-parent", str(arm),
            "--frontend-port", str(frontend), "--backend-port", str(backend),
            "--max-waiting", "0", "--profile", "reference-cpu" if reference else "swift-pq2-assessment"]
    if not reference:
        args += ["--native-engine" if native else "--publisher-engine",
                 "--native-row-workers", "4" if native else "1", "--use-abliterate-lora",
                 "--use-repacked-ptq1" if ptq1 else "--use-original-pq2"]
    sample.update(arguments=args, status="startup", nativeRowWorkers=4 if native else None)
    start = time.monotonic()
    arm_deadline = min(start + seconds, deadline - 30)
    startupinfo = subprocess.STARTUPINFO()
    startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startupinfo.wShowWindow = 0
    out_path, err_path = arm / "broker.stdout.log", arm / "broker.stderr.log"
    process = None
    snapshots = [sample["powerBefore"]]
    stopped = threading.Event()
    def observe():
        while not stopped.wait(1):
            snapshots.append(power())
    observer = threading.Thread(target=observe, daemon=True)
    observer.start()
    print(name + ": startup", flush=True)
    try:
        with out_path.open("wb") as out, err_path.open("wb") as err:
            process = subprocess.Popen(args, cwd=str(scratch), stdout=out, stderr=err,
                                       startupinfo=startupinfo,
                                       creationflags=subprocess.CREATE_NEW_CONSOLE)
        sample["brokerPid"] = process.pid
        save("session.json", session)
        while time.monotonic() < arm_deadline - 12:
            if process.poll() is not None:
                raise RuntimeError("Broker exited before readiness: " + err_path.read_text(errors="replace")[:1200])
            notice = out_path.read_text(errors="replace")
            match = re.search(r"Protected credentials and technical evidence: ([^\r\n]+)", notice)
            if match:
                private = Path(match.group(1))
                sample["startupSeconds"] = time.monotonic() - start
                key = (private / "frontend.key").read_text(encoding="utf-8-sig").strip()
                sample["status"] = "request"
                save("session.json", session)
                print(name + ": request", flush=True)
                sample["measurement"] = measure(frontend, key, arm_deadline-12, not reference)
                del key
                sample["status"] = "measured"
                break
            time.sleep(.1)
        else:
            raise TimeoutError("Startup consumed its declared arm allowance")
    except Exception as error:
        sample.update(status="failed", failure=str(error))
    finally:
        if process is not None:
            if process.poll() is None:
                sample["consoleStopExitCode"] = interrupt(process.pid)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    sample["forcedBrokerTermination"] = True
                    process.kill()
                    process.wait(timeout=3)
            sample["brokerExitCode"] = process.returncode
        stopped.set()
        observer.join(timeout=2)
        sample["powerAfter"] = power()
        snapshots.append(sample["powerAfter"])
        sample["powerObservations"] = snapshots
        sample["allObservedPowerConditionsMatch"] = all(
            item["acConnected"] is not None and
            (item["acConnected"], item["batterySaver"]) ==
            (snapshots[0]["acConnected"], snapshots[0]["batterySaver"]) for item in snapshots)
        sample["activeModelProcessesAfter"] = processes()
        sample["cleanupObserved"] = not sample["activeModelProcessesAfter"] and sample.get("brokerExitCode") == 0
        sample["listenerReleased"] = []
        for port in (frontend, backend):
            with socket.socket() as sock:
                sample["listenerReleased"].append(sock.connect_ex(("127.0.0.1", port)) != 0)
        sample["elapsedSeconds"] = time.monotonic() - start
        # Retain only technical model logs. Never copy generated credential files.
        selected = []
        for path in arm.rglob("*"):
            if path.is_file() and path.suffix in {".log", ".json"}:
                dest = ROOT / (name + "-" + path.name)
                if dest.exists():
                    continue
                content = path.read_bytes()
                dest.write_bytes(content)
                selected.append(dict(path=dest.name, bytes=len(content), sha256=hashlib.sha256(content).hexdigest()))
        sample["retainedTechnicalFiles"] = selected
        save("session.json", session)
        print(name + ": " + sample["status"] + " " + str(sample.get("measurement", {}).get("requestTokensPerSecond", sample.get("failure", ""))), flush=True)
        if sample["activeModelProcessesAfter"] or not all(sample["listenerReleased"]):
            raise RuntimeError("Owned cleanup not observed; no next model may start")

try:
    run_arm("rust-pq2", 125, True, False)
    run_arm("rust-ptq1", 125, True, True)
    run_arm("publisher-pq2", 80, False, False)
    run_arm("publisher-ptq1", 80, False, True)
    run_arm("qwen", 100, False, False, reference=True)
    session["status"] = "completed"
except Exception as error:
    session.update(status="stopped", failure=str(error))
finally:
    session["powerAfter"] = power()
    session["elapsedSecondsThroughRetention"] = time.monotonic() - session["startedMonotonic"]
    session["withinDeclaredBudget"] = session["elapsedSecondsThroughRetention"] <= session["maximumSeconds"]
    session["closedUtc"] = now()
    save("session.json", session)
    print(json.dumps({key: session[key] for key in ["status", "elapsedSecondsThroughRetention", "withinDeclaredBudget", "powerAfter"]}), flush=True)
