"""One explicitly requested, bounded local TTFT and generation capture."""
import base64
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer
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

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
CAP = 8
PROMPT = "Count from 1 to 100."

def utc():
    return datetime.now(timezone.utc).isoformat()

def save(name, value):
    destination = ROOT / name
    temporary = destination.with_suffix(destination.suffix + ".partial")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, destination)

def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac",ctypes.c_ubyte),("flags",ctypes.c_ubyte),
                    ("charge",ctypes.c_ubyte),("saver",ctypes.c_ubyte),
                    ("life",wintypes.DWORD),("full",wintypes.DWORD)]
    status = Status()
    ok = ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status))
    return dict(utc=utc(),acConnected={0:False,1:True}.get(status.ac) if ok else None,
                batterySaver=bool(status.saver) if ok else None,
                chargePercent=status.charge if ok and status.charge != 255 else None)

def process_identity():
    kernel = ctypes.WinDLL("kernel32",use_last_error=True)
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    created,exited,kernel_time,user_time = [wintypes.FILETIME() for _ in range(4)]
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)]*4
    if not kernel.GetProcessTimes(kernel.GetCurrentProcess(),ctypes.byref(created),
                                  ctypes.byref(exited),ctypes.byref(kernel_time),ctypes.byref(user_time)):
        raise RuntimeError("Unable to retain parent process identity")
    return os.getpid(), created.dwLowDateTime + (created.dwHighDateTime << 32)

def qpc():
    ticks,frequency = ctypes.c_longlong(),ctypes.c_longlong()
    if not ctypes.windll.kernel32.QueryPerformanceCounter(ctypes.byref(ticks)) or not ctypes.windll.kernel32.QueryPerformanceFrequency(ctypes.byref(frequency)):
        raise RuntimeError("Monotonic Windows clock unavailable")
    return ticks.value,frequency.value

def model_processes():
    command = "Get-Process | Where-Object { $_.ProcessName -match 'bonsai-|prism-inference|llama-|ollama|lm.?studio|qwen|canary|vllm|kobold' } | Select-Object Id,ProcessName | ConvertTo-Json -Compress"
    run = subprocess.run(["powershell.exe","-NoProfile","-Command",command],
                         capture_output=True,text=True,timeout=8,
                         creationflags=subprocess.CREATE_NO_WINDOW)
    if run.returncode:
        raise RuntimeError("Model process observation failed")
    value = json.loads(run.stdout) if run.stdout.strip() else []
    return value if isinstance(value,list) else [value]

class Events:
    def __init__(self):
        self.pending = b""
        self.events = []
        self.done = False
        self.received = []

    def feed(self, data, elapsed_ns):
        self.received.append(dict(elapsedNanoseconds=elapsed_ns,base64=base64.b64encode(data).decode("ascii")))
        self.pending = (self.pending+data).replace(b"\r\n",b"\n")
        while b"\n\n" in self.pending:
            record,self.pending = self.pending.split(b"\n\n",1)
            values = [line[6:] for line in record.split(b"\n") if line.startswith(b"data: ")]
            if not values:
                continue
            data = b"\n".join(values)
            if data == b"[DONE]":
                self.done = True
            else:
                self.events.append(dict(elapsedNanoseconds=elapsed_ns,event=json.loads(data)))

    def finish(self, total_ns, mode):
        usage = next((item["event"]["usage"] for item in reversed(self.events) if item["event"].get("usage")),{})
        count = usage.get("completion_tokens")
        content = []
        reasons = []
        reasoning = False
        tools = False
        for item in self.events:
            for choice in item["event"].get("choices",[]):
                delta = choice.get("delta",{})
                text = delta.get("content") or ""
                reasoning |= bool(delta.get("reasoning_content") or delta.get("reasoning"))
                tools |= bool(delta.get("tool_calls") or delta.get("function_call"))
                if text:
                    content.append(dict(text=text,elapsedNanoseconds=item["elapsedNanoseconds"]))
                if choice.get("finish_reason"):
                    reasons.append(choice["finish_reason"])
        if not self.done or self.pending.strip() or not content or len(reasons) != 1 or not isinstance(count,int) or not 1 <= count <= CAP:
            raise RuntimeError("Missing complete terminal SSE, nonempty content or reported token usage")
        first,last = content[0]["elapsedNanoseconds"],content[-1]["elapsedNanoseconds"]
        visible = "".join(item["text"] for item in content)
        eligible = (count == CAP and len(content) == count and reasons[-1] == "length"
                    and visible.isascii() and len(visible.encode()) < 1024
                    and re.fullmatch(r"[0-9,\s]+",visible) is not None
                    and not reasoning and not tools and last > first)
        return dict(timeToFirstTokenSeconds=first/1e9,
                    generationTokensPerSecond=(count-1)/((last-first)/1e9) if eligible else None,
                    generationIntervalSeconds=(last-first)/1e9,
                    generationIntervalTokens=count-1 if eligible else None,
                    firstContentNanoseconds=first,lastContentNanoseconds=last,
                    completionUsage=count,usage=usage,visibleContent=visible,
                    finishReason=reasons[-1],contentDeltaCount=len(content),
                    generationBoundaryEligible=eligible,
                    generationMetric="client-observed subsequent tokens / first-to-last token delivery interval",
                    generationEligibility="Reported cap8 and8nonemptyASCII token deltas, output<1024bytes, thinking off, length termination, positive interval; fixed-runtime source establishes one delta per displayed token.",
                    requestSeconds=total_ns/1e9,isolatedKernelDecodeTokensPerSecond=None,
                    events=self.events,receivedBodyChunks=self.received)

def stream_request(port,key,mode,deadline):
    native = mode == "native"
    # These are direct backend requests: use the broker's full canonical body,
    # rather than its smaller public request schema.
    body = dict(messages=[dict(role="user",content=PROMPT)],stream=True,max_completion_tokens=CAP,
                n=1,cache_prompt=False,n_cache_reuse=0,ignore_eos=False,
                temperature=1.0,top_p=.95,top_k=20,min_p=0.0,
                presence_penalty=0.0,frequency_penalty=0.0,repeat_penalty=1.0,
                chat_template_kwargs=dict(enable_thinking=False,reasoning_effort="xhigh",preserve_thinking=True))
    if mode != "qwen":
        body["lora"] = [dict(id=0,scale=1.3)]
    if not native:
        body.update(stream_options=dict(include_usage=True),seed=42)
    encoded = json.dumps(body)
    headers = {"Authorization":"Bearer "+key,"Content-Type":"application/json","Connection":"close"}
    started = time.perf_counter_ns()
    counter = http.client.HTTPConnection("127.0.0.1",port,timeout=max(.1,deadline-time.monotonic()))
    try:
        counter.request("POST","/v1/chat/completions/input_tokens",body=encoded,headers=headers)
        counted = counter.getresponse()
        count_payload = counted.read(32768)
        if counted.status != 200:
            raise RuntimeError("Count HTTP "+str(counted.status)+": "+count_payload.decode(errors="replace"))
        count_receipt = json.loads(count_payload)
        if count_receipt.get("object") != "response.input_tokens" or not isinstance(count_receipt.get("input_tokens"),int) or count_receipt["input_tokens"] <= 0:
            raise RuntimeError("Exact token admission did not return its required count")
    finally:
        counter.close()
    count_seconds = (time.perf_counter_ns()-started)/1e9
    connection = http.client.HTTPConnection("127.0.0.1",port,timeout=max(.1,deadline-time.monotonic()))
    try:
        connection.request("POST","/v1/chat/completions",body=encoded,headers=headers)
        response = connection.getresponse()
        if response.status != 200:
            raise RuntimeError("HTTP "+str(response.status)+": "+response.read(2048).decode(errors="replace"))
        parser = Events()
        size = 0
        while True:
            if time.monotonic() >= deadline:
                raise TimeoutError("Absolute request deadline reached")
            if connection.sock:
                connection.sock.settimeout(max(.1,deadline-time.monotonic()))
            chunk = response.read1(65536)
            observed = time.perf_counter_ns()-started
            if not chunk:
                break
            size += len(chunk)
            if size > 1024*1024:
                raise RuntimeError("Fixed response byte limit exceeded")
            parser.feed(chunk,observed)
        result = parser.finish(time.perf_counter_ns()-started,mode)
        if result["usage"].get("prompt_tokens") != count_receipt["input_tokens"]:
            raise RuntimeError("Generation prompt usage differs from the required admission count")
        result["requestBody"] = body
        result["countReceipt"] = count_receipt
        result["countAdmissionSeconds"] = count_seconds
        result["timeToFirstTokenScope"] = "Client request start before required exact count/admission through receipt of first token; includes count, template, prefill and HTTP delivery, excludes model startup."
        return result
    finally:
        connection.close()

def self_check():
    parser = Events()
    parser.feed(b'data: {"choices":[{"delta":{"role":"assistant"}}]}\n\n',100)
    for index in range(CAP):
        raw = ("data: "+json.dumps(dict(choices=[dict(delta=dict(content=str(index)),finish_reason=None)]))+"\n\n").encode()
        cut = len(raw)//2
        parser.feed(raw[:cut],(index+1)*1_000_000_000-10)
        parser.feed(raw[cut:],(index+1)*1_000_000_000)
    terminal = dict(choices=[dict(delta={},finish_reason="length")],usage=dict(completion_tokens=8,prompt_tokens=22,total_tokens=30))
    parser.feed(("data: "+json.dumps(terminal)+"\n\ndata: [DONE]\n\n").encode(),9_000_000_000)
    result = parser.finish(9_000_000_000,"native")
    assert result["timeToFirstTokenSeconds"] == 1 and result["generationTokensPerSecond"] == 1
    # An emitted event is never substituted for unknown or inconsistent usage.
    terminal["usage"]["completion_tokens"] = 7
    parser.events[-1]["event"] = terminal
    assert parser.finish(9_000_000_000,"native")["generationTokensPerSecond"] is None
    class Fixture(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        counted_body = None
        def log_message(self,*args):
            pass
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            if self.path.endswith("/input_tokens"):
                Fixture.counted_body = body
                payload = b'{"object":"response.input_tokens","input_tokens":22}'
                self.send_response(200); self.send_header("Content-Length",str(len(payload))); self.end_headers()
                self.wfile.write(payload); self.wfile.flush()
            else:
                assert body == Fixture.counted_body
                parts = [("data: "+json.dumps(dict(choices=[dict(delta=dict(content=str(index)),finish_reason=None)]))+"\n\n").encode() for index in range(CAP)]
                parts += [("data: "+json.dumps(dict(choices=[dict(delta={},finish_reason="length")],usage=dict(completion_tokens=8,prompt_tokens=22,total_tokens=30)))+"\n\ndata: [DONE]\n\n").encode()]
                self.send_response(200); self.send_header("Content-Length",str(sum(map(len,parts)))); self.end_headers()
                for part in parts:
                    time.sleep(.025)
                    self.wfile.write(part); self.wfile.flush()
            self.close_connection = True
    server = HTTPServer(("127.0.0.1",0),Fixture)
    worker = threading.Thread(target=server.serve_forever,daemon=True); worker.start()
    try:
        streamed = stream_request(server.server_port,"synthetic-fixture","native",time.monotonic()+5)
        assert 0 < streamed["timeToFirstTokenSeconds"] < streamed["requestSeconds"]
        assert streamed["generationIntervalSeconds"] > .15
        assert streamed["generationTokensPerSecond"] is not None
    finally:
        server.shutdown(); server.server_close(); worker.join(timeout=1)
    print("PASS split SSE records, role exclusion, usage-based arithmetic, mismatched usage rejection and delayed HTTP streaming",flush=True)

def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1",0))
        return sock.getsockname()[1]

def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))

def capture():
    if (ROOT/"session.json").exists():
        raise RuntimeError("This new timing workload already has a clock; no restart or retry")
    plan = read_json(ROOT/"workload-plan.json")
    ticks,frequency = qpc()
    started = time.monotonic()
    absolute = started+plan["maximumSeconds"]
    session = dict(schemaVersion=1,startedUtc=utc(),startedMonotonic=started,
                   originalStartedTicks=ticks,tickFrequency=frequency,
                   maximumSeconds=plan["maximumSeconds"],powerBeforeSetup=power(),
                   samples=[],status="setup",newUserRequestedWorkload=True,oldTrialClockReopened=False)
    save("session.json",session)
    scratch = Path(tempfile.mkdtemp(prefix="Bonsai-Token-Timing-20261103-"))
    session["scratchDirectory"] = str(scratch)
    data = Path(os.environ["LOCALAPPDATA"])/"SwiftBonsai2"
    parent_id,parent_created = process_identity()
    try:
        session["activeModelProcessesBeforeSetup"] = model_processes()
        if session["activeModelProcessesBeforeSetup"]:
            raise RuntimeError("Another model is active; no capture may start")
        self_check()
        target = Path(os.environ["LOCALAPPDATA"])/"SwiftBonsai2"/"build"/"chat"/"cd31d71ca8da53bd5d98bc6d"
        build_repo = REPO
        format_command = [str(Path(os.environ["USERPROFILE"])/".cargo"/"bin"/"rustfmt.exe"),
                          "+nightly-2026-11-02-aarch64-pc-windows-msvc","--edition","2024",
                          str(build_repo/"src"/"broker"/"examples"/"token_timing_owner.rs")]
        with (ROOT/"format.stdout.log").open("wb") as out,(ROOT/"format.stderr.log").open("wb") as err:
            formatted = subprocess.run(format_command,cwd=str(build_repo),stdout=out,stderr=err,
                                       timeout=10,creationflags=subprocess.CREATE_NO_WINDOW)
        session["formatterExitCode"] = formatted.returncode
        if formatted.returncode:
            raise RuntimeError("Measurement owner source formatter failed; no model was started")
        session["helperSourceSha256"] = hashlib.sha256((build_repo/"src"/"broker"/"examples"/"token_timing_owner.rs").read_bytes()).hexdigest()
        command = [str(Path(os.environ["USERPROFILE"])/".cargo"/"bin"/"cargo.exe"),
                   "+nightly-2026-11-02-aarch64-pc-windows-msvc","build","--offline","--locked","--release",
                   "--manifest-path",str(build_repo/"Cargo.toml"),"--package","bonsai-broker",
                   "--example","token_timing_owner","--target","aarch64-pc-windows-msvc",
                   "--target-dir",str(target)]
        with (ROOT/"build.stdout.log").open("wb") as out,(ROOT/"build.stderr.log").open("wb") as err:
            run = subprocess.run(command,cwd=str(scratch),stdout=out,stderr=err,
                                 timeout=plan["buildMaximumSeconds"],creationflags=subprocess.CREATE_NO_WINDOW)
        session["build"] = dict(arguments=command,exitCode=run.returncode)
        save("session.json",session)
        if run.returncode:
            raise RuntimeError("Measurement owner build failed; no model was started")
        helper = target/"aarch64-pc-windows-msvc"/"release"/"examples"/"token_timing_owner.exe"
        session["helperSha256"] = hashlib.sha256(helper.read_bytes()).hexdigest()
        pins = read_json(ROOT/"native-pins.json")
        for arm in plan["arms"]:
            sample = dict(id=arm["id"],status="pending",powerBefore=power())
            session["samples"].append(sample)
            save("session.json",session)
            if arm["mode"] == "qwen":
                model = read_json(REPO/"config"/"runtime.lock.json")["model"]
                expected = data/"models"/model["revision"]/model["filename"]
                if not expected.is_file():
                    sample.update(status="not-installed",reason="Pinned Qwen model absent; no download within this fixed workload",powerAfter=power())
                    save("session.json",session)
                    print(arm["id"]+": not installed",flush=True)
                    continue
            if time.monotonic()+arm["maximumSeconds"]+20 >= absolute:
                sample.update(status="not-run",reason="Insufficient original aggregate budget",powerAfter=power())
                save("session.json",session)
                break
            observed = model_processes()
            sample["activeModelProcessesBefore"] = observed
            if observed:
                raise RuntimeError("A model is already active; no next arm")
            directory,control = scratch/arm["id"],scratch/(arm["id"]+"-control")
            directory.mkdir(); control.mkdir()
            port = free_port()
            request = dict(dataDirectory=str(data),evidenceDirectory=str(directory),shutdownDirectory=str(control),
                           mode=arm["mode"],nativeVersion=arm.get("version"),
                           rowWorkers=arm.get("workers",1),useRepackedPtq1=arm.get("ptq1",False),port=port,
                           maximumSeconds=arm["maximumSeconds"],deadlineTicks=ticks+int(plan["maximumSeconds"]*frequency),
                           parentProcessId=parent_id,parentCreationTimeFileTime=parent_created,
                           runtime=None,nativeRuntimeFiles=[])
            if arm["mode"] == "native":
                pin = pins[arm["version"]]
                request["runtime"] = dict(archivePath=str(data/"downloads"/pin["asset"]["filename"]),
                                          runtimeDirectory=str(data/"inference"/pin["commit"] /"aarch64-pc-windows-msvc"),
                                          archiveBytes=pin["asset"]["bytes"],archiveSha256=pin["asset"]["sha256"])
                request["nativeRuntimeFiles"] = pin["files"]
            request_path = scratch/(arm["id"]+".json")
            request_path.write_text(json.dumps(request),encoding="utf-8")
            sample["ownerRequest"] = request
            arm_start = time.monotonic()
            arm_deadline = min(arm_start+arm["maximumSeconds"],absolute-20)
            snapshots = [sample["powerBefore"]]
            stopping = threading.Event()
            def observe():
                while not stopping.wait(1):
                    snapshots.append(power())
            monitor = threading.Thread(target=observe,daemon=True); monitor.start()
            process = None
            print(arm["id"]+": startup",flush=True)
            try:
                with (directory/"owner.stdout.log").open("wb") as out,(directory/"owner.stderr.log").open("wb") as err:
                    process = subprocess.Popen([str(helper),str(request_path)],cwd=str(scratch),
                                               stdout=out,stderr=err,creationflags=subprocess.CREATE_NO_WINDOW)
                sample["ownerPid"] = process.pid
                sample["status"] = "startup"
                save("session.json",session)
                while time.monotonic() < arm_deadline-12:
                    if process.poll() is not None:
                        raise RuntimeError("Native owner exited before readiness; see retained technical logs")
                    if (directory/"owner.json").is_file():
                        owner = read_json(directory/"owner.json")
                        key = (Path(owner["credentialDirectory"])/"backend.key").read_text(encoding="utf-8-sig").strip()
                        connection = http.client.HTTPConnection("127.0.0.1",port,timeout=.5)
                        try:
                            connection.request("GET","/health",headers={"Authorization":"Bearer "+key,"Connection":"close"})
                            response = connection.getresponse()
                            healthy = response.status == 200 and json.loads(response.read(32768)).get("status") == "ok"
                        except (OSError,http.client.HTTPException,ValueError):
                            healthy = False
                        finally:
                            connection.close()
                        if healthy:
                            sample["owner"] = owner
                            sample["startupSeconds"] = time.monotonic()-arm_start
                            sample["status"] = "request"
                            save("session.json",session)
                            print(arm["id"]+": streaming",flush=True)
                            sample["measurement"] = stream_request(port,key,arm["mode"],arm_deadline-12)
                            del key
                            sample["status"] = "measured"
                            break
                    time.sleep(.1)
                else:
                    raise TimeoutError("Owner startup did not fit its fixed arm")
            except Exception as error:
                sample.update(status="failed",failure=str(error))
            finally:
                (control/"shutdown.request").touch(exist_ok=False)
                if process is not None:
                    try:
                        process.wait(timeout=max(.1,min(12,absolute-time.monotonic()-5)))
                    except subprocess.TimeoutExpired:
                        sample["forcedOwnerTermination"] = True
                        process.kill(); process.wait(timeout=3)
                    sample["ownerExitCode"] = process.returncode
                stopping.set(); monitor.join(timeout=2)
                sample["powerAfter"] = power(); snapshots.append(sample["powerAfter"])
                sample["powerSamples"] = snapshots
                cleanup_path = directory/"cleanup.json"
                sample["cleanup"] = read_json(cleanup_path) if cleanup_path.is_file() else None
                sample["activeModelProcessesAfter"] = model_processes()
                with socket.socket() as sock:
                    sample["listenerReleased"] = sock.connect_ex(("127.0.0.1",port)) != 0
                sample["elapsedSeconds"] = time.monotonic()-arm_start
                if (sample.get("cleanup") or {}).get("ownedJobStopped") and not sample["activeModelProcessesAfter"]:
                    for path in directory.rglob("*.key"):
                        resolved = path.resolve()
                        if scratch.resolve() not in resolved.parents or resolved.name not in {"frontend.key","backend.key"}:
                            raise RuntimeError("Unexpected credential cleanup target")
                        resolved.unlink()
                sample["remainingCredentialFiles"] = len(list(directory.rglob("*.key")))
                # Fixed technical files only; credentials are never archived.
                for path in directory.rglob("*"):
                    if path.is_file() and path.name in {"owner.stdout.log","owner.stderr.log","backend.stdout","backend.stderr","owner.json","cleanup.json"}:
                        (ROOT/(arm["id"]+"-"+path.name)).write_bytes(path.read_bytes())
                save("session.json",session)
                metric = sample.get("measurement",{})
                print(arm["id"]+": "+sample["status"]+" TTFT="+str(metric.get("timeToFirstTokenSeconds"))+" generation="+str(metric.get("generationTokensPerSecond")),flush=True)
                if sample["activeModelProcessesAfter"] or not sample["listenerReleased"] or not (sample.get("cleanup") or {}).get("ownedJobStopped") or sample.get("ownerExitCode") != 0:
                    raise RuntimeError("Explicit owned cleanup failed; stop this workload")
                if sample["status"] == "failed":
                    # Other configurations are independent declared arms, not retries.
                    sample["additionalAttemptOfSameArm"] = False
        session["status"] = "completed"
    except Exception as error:
        session.update(status="stopped",failure=str(error))
    finally:
        session["powerAfterFinalRetention"] = power()
        session["elapsedSecondsThroughRetention"] = time.monotonic()-started
        session["withinDeclaredBudget"] = session["elapsedSecondsThroughRetention"] <= plan["maximumSeconds"]
        session["closedUtc"] = utc()
        save("session.json",session)
        print(json.dumps({key:session[key] for key in ["status","elapsedSecondsThroughRetention","withinDeclaredBudget"]}),flush=True)

if __name__ == "__main__":
    if sys.argv[1:] == ["--self-check"]:
        self_check()
    else:
        capture()
