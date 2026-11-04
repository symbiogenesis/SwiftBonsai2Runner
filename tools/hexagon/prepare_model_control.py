"""Prepare one local Hexagon control after a recorded successful probe build.

This performs no device work, downloads, builds, or CI actions. The caller owns
the aggregate workload deadline and must supply its observed build receipt.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def pin(path):
    path = Path(path)
    with path.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return {"bytes": path.stat().st_size, "sha256": digest}


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def revision(git, source):
    prefix = [str(git), "-c", "safe.directory=" + source.as_posix(), "-C", str(source)]
    dirty = subprocess.check_output(prefix + ["status", "--porcelain"], text=True)
    if dirty.strip():
        raise ValueError("The built source must be clean: " + str(source))
    return subprocess.check_output(prefix + ["rev-parse", "HEAD"], text=True).strip()


def source_pins(source):
    names = {"Cargo.toml", "Cargo.lock"}
    for name in ["build.rs", "rust-toolchain.toml", ".cargo/config.toml"]:
        if (source / name).is_file():
            names.add(name)
    for base in ["src", "tools"]:
        for parent, dirs, files in os.walk(source / base):
            dirs[:] = [name for name in dirs if name not in ["target", ".git", ".worktrees"]]
            for name in files:
                names.add((Path(parent) / name).relative_to(source).as_posix())
    return [{"path": name, **pin(source / name)} for name in sorted(names)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["runner-repository", "prism-repository", "probe", "supervisor",
                 "compiler", "successful-build-receipt", "output-directory", "git"]:
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--adapter-case", choices=["none", "loaded-zero", "standard-scale-1.3"],
                        default="standard-scale-1.3")
    parser.add_argument("--token-ids", type=int, nargs="+", default=[42, 43, 128])
    args = parser.parse_args()
    if not 1 <= len(args.token_ids) <= 3 or min(args.token_ids) < 0:
        parser.error("The current snapshot budget permits one to three nonnegative token IDs.")
    # This is the real build runner's retained result, never a fabricated pass.
    built = json.loads(args.successful_build_receipt.read_text(encoding="utf-8-sig"))
    if built.get("exitCode") != 0 or built.get("timedOut", False):
        raise ValueError("The supplied build receipt does not record a successful build.")
    runner = args.runner_repository.resolve()
    prism = args.prism_repository.resolve()
    prism_revision = revision(args.git, prism)
    runner_revision = revision(args.git, runner)
    output = args.output_directory.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bundle = output / "model-bundle"
    bundle.mkdir()
    probe = bundle / "prism-hexagon-model-probe.exe"
    supervisor = bundle / "bonsai-tools.exe"
    shutil.copy2(args.probe, probe)
    shutil.copy2(args.supervisor, supervisor)
    shutil.copy2(args.successful_build_receipt, output / "observed-probe-build.json")
    receipt = {
        "schemaVersion": 1,
        "kind": "local-prism-hexagon-model-probe-build",
        "hostedBuild": False,
        "buildSucceeded": True,
        "cargoLocked": True,
        "target": "aarch64-pc-windows-msvc",
        "prismRevision": prism_revision,
        "compiler": {"path": str(args.compiler.resolve()), **pin(args.compiler)},
        "executable": pin(probe),
        "sources": source_pins(prism),
    }
    receipt_path = output / "local-model-build.json"
    write(receipt_path, receipt)
    template = runner / "docs/evidence/hexagon-adapter-session-20261103/model-device-request.json"
    envelope = json.loads(template.read_text(encoding="utf-8-sig"))
    request = envelope["request"]
    request.update({
        "repositoryDirectory": str(runner),
        "prismRepositoryDirectory": str(prism),
        "expectedPrismRevision": prism_revision,
        "executablePath": str(probe),
        "executableSha256": receipt["executable"]["sha256"],
        "buildReceiptPath": str(receipt_path),
        "outputDirectory": str(output / "device"),
        "tokenId": args.token_ids[0],
        "tokenIds": args.token_ids,
        "executionBackend": "hmx-bitplane",
        "adapterCase": args.adapter_case,
        "cpuBackend": "neon",
        "rowWorkers": 4,
        "maximumSeconds": 300,
    })
    if args.adapter_case == "none":
        request.pop("adapterPath", None)
    for name in ["modelPath", "packageDirectory", "signtoolPath"]:
        if not Path(request[name]).exists():
            raise ValueError("The retained control input is absent: " + request[name])
    if args.adapter_case != "none":
        expected = {"bytes": 9682464,
                    "sha256": "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"}
        if pin(request["adapterPath"]) != expected:
            raise ValueError("The approved standard adapter identity differs.")
    request_path = output / "model-device-request.json"
    write(request_path, envelope)
    intake = {
        "runnerRevision": runner_revision,
        "prismRevision": prism_revision,
        "supervisor": pin(supervisor),
        "probe": pin(probe),
        "sourceFiles": len(receipt["sources"]),
        "observedBuildReceipt": pin(output / "observed-probe-build.json"),
        "deviceCommand": [str(supervisor), "hexagon-tools", "--request", str(request_path)],
        "deviceAttempted": False,
        "performanceQualified": False,
    }
    write(output / "intake.json", intake)
    print(json.dumps(intake))


if __name__ == "__main__":
    main()
