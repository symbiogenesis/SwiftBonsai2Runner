"""Hosted native Windows ARM64 public queue/packet ABI and generated-wire control.

The queue test retains the real Windows conditionals. The unchanged generated
start/stop test selects the public non-Windows branch with /U_WIN32 (preserving
_WIN64 and _M_ARM64). It proves native data layout and frame bytes only; it does
not supply the absent remote_ext.h/qtest_stdlib.h or qualify SDK equivalence.
No vendor executable, SDK, runtime DLL or driver is loaded.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import urllib.request

from public_qaic_wire import equal_typed, unique_object


ROOT = Path(__file__).resolve().parents[2]
LOCAL = Path(__file__).resolve().parent
RETAINED = ROOT / "docs/evidence/hexagon-public-qaic-41171859264"
MAX_BYTES = 1024 * 1024


def identity(path):
    data = path.read_bytes()
    return {"name": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def assert_identity(path, expected):
    actual = identity(path)
    if any(actual[key] != expected[key] for key in ("bytes", "sha256")):
        raise ValueError("input identity changed: " + path.name)
    return actual


def arm64_pe(path):
    with path.open("rb") as source:
        header = source.read(64)
        if len(header) < 64 or header[:2] != b"MZ":
            raise ValueError("expected native ARM64 PE: " + path.name)
        source.seek(struct.unpack_from("<I", header, 60)[0])
        signature = source.read(6)
    if signature != b"PE\0\0\x64\xaa":
        raise ValueError("expected native ARM64 PE: " + path.name)


class Control:
    def __init__(self, output, report):
        self.output = output
        self.review = output / "review"
        self.review.mkdir()
        self.report = report
        self.env = dict(os.environ)
        self.commands = []

    def save(self):
        self.report["commands"] = self.commands
        (self.review / "provenance.json").write_text(json.dumps(self.report, indent=2) + "\n", encoding="utf-8")

    def run(self, name, arguments, *, expected_failure=False, seconds=30):
        command = {"name": name, "arguments": list(map(str, arguments)), "completed": False}
        self.commands.append(command)
        self.save()
        result = subprocess.run(command["arguments"], cwd=self.output, env=self.env,
                                capture_output=True, timeout=seconds, check=False)
        if len(result.stdout) + len(result.stderr) > MAX_BYTES:
            raise ValueError("bounded command output exceeded: " + name)
        (self.review / (name + ".stdout.log")).write_bytes(result.stdout)
        (self.review / (name + ".stderr.log")).write_bytes(result.stderr)
        command.update(completed=True, exitCode=result.returncode)
        self.save()
        if (result.returncode == 0) == expected_failure:
            raise ValueError("command outcome differed: " + name)
        return result.stdout.decode("utf-8", errors="strict")


def select_compiler(control):
    installer = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    text = control.run("vswhere", [installer, "-latest", "-products", "*", "-requires",
                                  "Microsoft.VisualStudio.Component.VC.Tools.ARM64", "-property", "installationPath"])
    installation = Path(text.strip()).resolve(strict=True)
    version = (installation / "VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt").read_text().strip()
    tools = installation / "VC/Tools/MSVC" / version
    binary = tools / "bin/Hostarm64/arm64"
    compiler = binary / "cl.exe"
    linker = binary / "link.exe"
    arm64_pe(compiler)
    arm64_pe(linker)
    sdk = Path(os.environ["ProgramFiles(x86)"]) / "Windows Kits/10"
    versions = sorted((path for path in (sdk / "Include").iterdir()
                       if path.is_dir() and (sdk / "Lib" / path.name / "ucrt/arm64/ucrt.lib").is_file()
                       and (sdk / "Lib" / path.name / "um/arm64/kernel32.lib").is_file()),
                      key=lambda path: tuple(map(int, path.name.split("."))), reverse=True)
    if not versions:
        raise ValueError("native ARM64 Windows SDK libraries are required")
    sdk_version = versions[0].name
    control.env["INCLUDE"] = ";".join(map(str, [tools / "include", sdk / "Include" / sdk_version / "ucrt",
                                              sdk / "Include" / sdk_version / "shared",
                                              sdk / "Include" / sdk_version / "um"]))
    control.env["LIB"] = ";".join(map(str, [tools / "lib/arm64", sdk / "Lib" / sdk_version / "ucrt/arm64",
                                          sdk / "Lib" / sdk_version / "um/arm64"]))
    control.env["PATH"] = str(binary) + ";" + control.env["PATH"]
    components = [compiler, linker, binary / "c1.dll", binary / "c2.dll"]
    selected = [{**identity(path), "path": str(path)} for path in components]
    control.report["compiler"] = {"selectedFiles": selected, "toolsVersion": version,
                                  "windowsSdkVersion": sdk_version, "target": "aarch64-pc-windows-msvc",
                                  "scope": "selected native compiler/linker components; not-complete-OS-closure"}
    return compiler, selected


def intake(control, pin):
    inputs = control.output / "inputs"
    inputs.mkdir()
    retained_inputs = control.review / "inputs"
    retained_inputs.mkdir()
    entries = []
    for entry in pin["publicInputs"]:
        if not entry["url"].startswith("https://raw.githubusercontent.com/"):
            raise ValueError("public input URL is outside the reviewed source host")
        with urllib.request.urlopen(entry["url"], timeout=30) as response:
            data = response.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise ValueError("public input exceeded its byte bound")
        path = inputs / entry["name"]
        path.write_bytes(data)
        entries.append({**assert_identity(path, entry), "url": entry["url"]})
        shutil.copyfile(path, retained_inputs / (path.name + ".txt"))
    public_pin = json.loads((LOCAL / "public-qaic.lock.json").read_bytes(), object_pairs_hook=unique_object)
    for entry in public_pin["wireCapture"]["headers"]:
        source = RETAINED / "wire-public-inputs" / (entry["name"] + ".txt")
        assert_identity(source, entry)
        shutil.copyfile(source, inputs / entry["name"])
        shutil.copyfile(source, retained_inputs / (entry["name"] + ".txt"))
        entries.append({**identity(source), "origin": str(source.relative_to(ROOT)), "url": entry["url"]})
    for entry in public_pin["wireCapture"]["reviewedGeneratedFiles"]:
        if entry["name"] == "htp_iface_skel.c":
            continue
        source = RETAINED / "generated-public-source" / (entry["name"] + ".txt")
        assert_identity(source, entry)
        shutil.copyfile(source, inputs / entry["name"])
        shutil.copyfile(source, retained_inputs / (entry["name"] + ".txt"))
        entries.append({**identity(source), "origin": str(source.relative_to(ROOT)), "generatedBy": public_pin["qaicCommit"]})
    notices = control.review / "notices"
    shutil.copytree(RETAINED / "inputs-and-notices", notices)
    shutil.copyfile(RETAINED / "NOTICES.txt", notices / "generation-NOTICES.txt")
    for source in (LOCAL / "windows_queue_abi.c", LOCAL / "windows_queue_abi.rs",
                   LOCAL / "public_qaic_wire.c", LOCAL / "public_idl_control.rs", LOCAL / "public-qaic-wire.expected.json"):
        shutil.copyfile(source, retained_inputs / (source.name + ".txt"))
    control.report["inputs"] = entries
    control.report["localSources"] = [identity(path) for path in retained_inputs.iterdir()]
    return inputs


def build_c(control, compiler, inputs, name, sources, *, branch=False):
    executable = control.output / (name + ".exe")
    arguments = [compiler, "/nologo", "/std:c11", "/TC", "/Od", "/GS", "/guard:cf", "/I" + str(inputs),
                 "/Fo" + str(control.output) + "\\", "/Fe" + str(executable)]
    if branch:
        # Preserve _WIN64 for the real Windows CRT pointer widths. Only the
        # Qualcomm public-header/generator branch switches to its non-Windows form.
        arguments += ["/W1", "/wd4068", "/U_WIN32", "/D_CRT_SECURE_NO_WARNINGS",
                      "/D_ATTRIBUTE_UNUSED=", "/D_ATTRIBUTE_VISIBILITY=", "/FI" + str(inputs / "AEEStdDef.h")]
    else:
        arguments += ["/W4", "/WX"]
    control.run(name + "-compile", [*arguments, *sources, "/link", "/machine:arm64", "/MAP:" + str(control.review / (name + ".map"))])
    arm64_pe(executable)
    return executable


def compare_layout(control, compiler, inputs, rustc):
    executable = build_c(control, compiler, inputs, "queue-abi", [LOCAL / "windows_queue_abi.c"])
    observed = json.loads(control.run("queue-abi-capture", [executable], seconds=5), object_pairs_hook=unique_object)
    expected = {"schemaVersion": 1, "kind": "windows-arm64-public-queue-abi-control",
                "queueBuffer": {"bytes": 24, "alignment": 8, "fdOffset": 0, "sizeOffset": 4,
                                "offsetOffset": 8, "flagsOffset": 12, "addressOffset": 16},
                "htp": {"tensorBytes": 56, "bufferBytes": 24, "operationBytes": 232,
                        "requestBytes": 24, "responseBytes": 88, "profileBytes": 48},
                "queueWrites": {"timed": {"arguments": 7, "timeoutMicroseconds": 100000, "calls": 1},
                                "nonblocking": {"arguments": 6, "calls": 1}},
                "infiniteTimeout": 4294967295, "mockCalls": 6, "unexpectedCalls": 0, "returnCode": 0,
                "bufferHex": "98badcfe403020100403020194000000efcdab8967452381",
                "driverLoaded": False, "deviceExecuted": False, "profileEligible": False}
    if not equal_typed(observed, expected):
        raise ValueError("public native queue/packet ABI control differs from the independent expectation")
    rust_executable = control.output / "queue-abi-rust.exe"
    control.run("queue-abi-rust-build", [rustc, "--edition=2024", "--target", "aarch64-pc-windows-msvc", "-D", "warnings",
                                       "-C", "linker=" + str(compiler.parent / "link.exe"),
                                       LOCAL / "windows_queue_abi.rs", "-o", rust_executable])
    arm64_pe(rust_executable)
    rust = json.loads(control.run("queue-abi-rust-capture", [rust_executable], seconds=5), object_pairs_hook=unique_object)
    keys = {"schemaVersion", "queueBuffer", "htp", "infiniteTimeout", "bufferHex",
            "driverLoaded", "deviceExecuted", "profileEligible"}
    if rust.keys() != keys | {"kind"} or rust["kind"] != "windows-arm64-Rust-queue-abi-control" or \
            any(not equal_typed(rust[key], observed[key]) for key in keys):
        raise ValueError("native Rust queue layout differs from the independently compiled public C headers")
    control.report["queueAbi"] = {"passed": True, "nativeWindowsConditionals": True,
                                   "observed": observed, "rustObserved": rust,
                                   "executable": identity(executable), "rustExecutable": identity(rust_executable)}
    negatives = {
        "narrow-queue-pointer": '#include "dspqueue.h"\ntypedef int (*bad_close)(uint32_t);\nbad_close narrowed = dspqueue_close;\n',
        "timed-write-without-timeout": '#include "dspqueue.h"\ntypedef AEEResult (*bad_write)(dspqueue_t, uint32_t, uint32_t, struct dspqueue_buffer *, uint32_t, const uint8_t *);\nbad_write missing_timeout = dspqueue_write;\n',
        "noblock-write-with-timeout": '#include "dspqueue.h"\ntypedef AEEResult (*bad_noblock)(dspqueue_t, uint32_t, uint32_t, struct dspqueue_buffer *, uint32_t, const uint8_t *, uint32_t);\nbad_noblock extra_timeout = dspqueue_write_noblock;\n',
        "wrong-buffer-layout": '#include "dspqueue.h"\n_Static_assert(sizeof(struct dspqueue_buffer) == 20, "deliberate wrong buffer ABI");\n',
        "wrong-operation-layout": '#include <stdint.h>\n#include "htp-ops.h"\n_Static_assert(sizeof(struct htp_op_desc) == 224, "deliberate wrong operation ABI");\n',
    }
    for name, source in negatives.items():
        path = control.output / (name + ".c")
        path.write_text(source, encoding="utf-8")
        shutil.copyfile(path, control.review / (name + ".c.txt"))
        control.run(name, [compiler, "/nologo", "/std:c11", "/TC", "/W4", "/WX", "/c", "/I" + str(inputs),
                           "/Fo" + str(control.output / (name + ".obj")), path], expected_failure=True)
        diagnostics = b"\n".join((control.review / (name + suffix)).read_bytes()
                                   for suffix in (".stdout.log", ".stderr.log"))
        if name in {"narrow-queue-pointer", "timed-write-without-timeout", "noblock-write-with-timeout"}:
            if not re.search(rb"\bC(?:4113|4133|4047)\b", diagnostics):
                raise ValueError("incompatible queue signature failed for an unrelated reason: " + name)
        elif ("deliberate wrong " + ("buffer" if name == "wrong-buffer-layout" else "operation") +
              " ABI").encode("ascii") not in diagnostics:
            raise ValueError("wrong layout control failed for an unrelated reason")
    control.report["queueAbi"]["negativeControlsRejected"] = list(negatives)


def compare_wire(control, compiler, inputs, rustc):
    executable = build_c(control, compiler, inputs, "start-stop-wire",
                         [inputs / "htp_iface_stub.c", LOCAL / "public_qaic_wire.c"], branch=True)
    observed = json.loads(control.run("start-stop-wire-capture", [executable], seconds=5), object_pairs_hook=unique_object)
    expected = json.loads((LOCAL / "public-qaic-wire.expected.json").read_bytes(), object_pairs_hook=unique_object)
    shared = ("schemaVersion", "testVectorId", "handleHex", "start", "stop")
    if any(not equal_typed(observed[name], expected[name]) for name in shared):
        raise ValueError("native generated C start/stop differs from the independent golden frame")
    expected_layout = {"charBits": 8, "pointerBytes": 8, "sizeTBytes": 8, "uint32Bytes": 4,
                       "uint64Bytes": 8, "remoteHandle64Bytes": 8, "remoteBufferBytes": 16,
                       "remoteBufferAlignment": 8, "remoteBufferPointerOffset": 0, "remoteBufferLengthOffset": 8,
                       "remoteArgBytes": 16, "remoteArgAlignment": 8, "littleEndian": True}
    if not equal_typed(observed["layout"], expected_layout) or observed["callCount"] != 2 or \
            observed["unexpectedCalls"] != 0 or observed["returnCodes"] != {"start": 0, "stop": 0}:
        raise ValueError("native generated C wire control failed its call/layout checks")
    rust_executable = control.output / "start-stop-rust.exe"
    control.run("start-stop-rust-build", [rustc, "--edition=2024", "--target", "aarch64-pc-windows-msvc", "-D", "warnings",
                                        "-C", "linker=" + str(compiler.parent / "link.exe"),
                                        LOCAL / "public_idl_control.rs", "-o", rust_executable])
    arm64_pe(rust_executable)
    rust = json.loads(control.run("start-stop-rust-capture", [rust_executable], seconds=5), object_pairs_hook=unique_object)
    if any(not equal_typed(rust[name], observed[name]) for name in shared):
        raise ValueError("native independent Rust encoder differs from unchanged generated C")
    control.report["startStopWire"] = {
        "passed": True, "generatedFilesUnchanged": True, "nativeCompilerTarget": "aarch64-pc-windows-msvc",
        "publicHeaderConditionals": "non-Windows public branch: /U_WIN32; _WIN64 and _M_ARM64 preserved",
        "generatedSourceAllocatorBranch": "standard malloc/free; no qtest_stdlib replacement",
        "generatedAttributeConfiguration": "documented #ifndef overrides _ATTRIBUTE_UNUSED/_ATTRIBUTE_VISIBILITY empty for MSVC",
        "wholeWindowsHeaderClosureQualified": False, "sdkEquivalenceQualified": False,
        "observed": observed, "rustObserved": rust,
        "executable": identity(executable), "rustExecutable": identity(rust_executable),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rustc", required=True, type=Path)
    arguments = parser.parse_args()
    if sys.platform != "win32" or platform.machine().upper() != "ARM64" or \
            os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise ValueError("this control runs only on hosted native Windows ARM64; no owner-device workload")
    output = arguments.output.resolve()
    if not output.is_absolute() or output.exists() or not output.is_relative_to(Path(os.environ["RUNNER_TEMP"]).resolve()):
        raise ValueError("select a new empty hosted RUNNER_TEMP directory")
    output.mkdir()
    report = {"schemaVersion": 1, "kind": "hosted-windows-arm64-public-queue-wire-control", "passed": False,
              "sourceCommit": os.environ.get("GITHUB_SHA"), "runId": os.environ.get("GITHUB_RUN_ID"),
              "runAttempt": os.environ.get("GITHUB_RUN_ATTEMPT"), "controller": identity(Path(__file__)),
              "driverLoaded": False, "sdkUsed": False, "deviceExecuted": False, "profileEligible": False,
              "scope": "public queue/packet ABI and explicitly selected non-Windows public start/stop wire branch",
              "wholeWindowsHeaderClosureQualified": False, "sdkEquivalenceQualified": False}
    control = Control(output, report)
    try:
        pin = json.loads((LOCAL / "windows-queue-abi.lock.json").read_bytes(), object_pairs_hook=unique_object)
        report["pin"] = identity(LOCAL / "windows-queue-abi.lock.json")
        inputs = intake(control, pin)
        compiler, selected = select_compiler(control)
        rustc = arguments.rustc.resolve(strict=True)
        arm64_pe(rustc)
        report["rustCompiler"] = identity(rustc)
        banner = control.run("rustc-identity", [rustc, "-vV"])
        if "host: aarch64-pc-windows-msvc" not in banner.splitlines():
            raise ValueError("the independent Rust compiler must be native Windows ARM64")
        compare_layout(control, compiler, inputs, rustc)
        compare_wire(control, compiler, inputs, rustc)
        for component in selected:
            assert_identity(Path(component["path"]), component)
        assert_identity(rustc, report["rustCompiler"])
        report["compilerIdentitiesRechecked"] = True
        report["passed"] = True
    finally:
        control.save()
    print(json.dumps({"passed": report["passed"], "receipt": str(control.review / "provenance.json")}))


if __name__ == "__main__":
    main()
