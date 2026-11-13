"""Bind a reusable CI bootstrap executable to exact hosted build inputs.

This verifies bytes before execution; a cache hit is never a check/test verdict.
Only the CI command and its receipt belong in this cache, never acceptance output.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import sys


TOOLCHAIN = "nightly-2026-11-12-aarch64-pc-windows-msvc"
HOST = "aarch64-pc-windows-msvc"
FIELDS = {"schemaVersion", "sourceId", "hostId", "toolchain", "compiler", "sha256"}


def identity(source: str, host: str) -> None:
    """Reject incomplete caller identities before creating or trusting a receipt."""
    if not re.fullmatch(r"[a-f0-9]{64}", source):
        raise ValueError("An exact SHA-256 source identity is required.")
    if not host or host.strip() != host or any(c in host for c in "\r\n\0"):
        raise ValueError("An exact hosted image/OS identity is required.")


def executable_digest(path: Path) -> str:
    """Check a native ARM64 PE executable without running the cached program."""
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("The CI bootstrap is not a PE executable.")
        offset = struct.unpack_from("<I", header, 60)[0]
        if offset < 64 or offset > path.stat().st_size - 26:
            raise ValueError("The CI bootstrap has an invalid PE header offset.")
        stream.seek(offset)
        pe = stream.read(26)
        if (pe[:4] != b"PE\0\0" or struct.unpack_from("<H", pe, 4)[0] != 0xAA64
                or struct.unpack_from("<H", pe, 24)[0] != 0x20B):
            raise ValueError("The CI bootstrap must be a native Windows ARM64 PE32+ image.")
        characteristics = struct.unpack_from("<H", pe, 22)[0]
        if not characteristics & 0x0002 or characteristics & 0x2000:
            raise ValueError("The CI bootstrap must be an executable, not a DLL.")
        stream.seek(0)
        digest = hashlib.sha256()
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
        return digest.hexdigest()


def unique_fields(pairs):
    """Reject duplicate JSON fields rather than silently accepting a replacement."""
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError(f"Duplicate receipt field: {name}")
        result[name] = value
    return result


def verify(package: Path, source: str, host: str) -> Path:
    """Verify exact inputs and the executable digest before selecting the image."""
    identity(source, host)
    receipt = json.loads((package / "receipt.json").read_text(encoding="utf-8"),
                         object_pairs_hook=unique_fields)
    if not isinstance(receipt, dict) or set(receipt) != FIELDS:
        raise ValueError("The CI bootstrap receipt has unexpected fields.")
    if (type(receipt["schemaVersion"]) is not int or receipt["schemaVersion"] != 1
            or receipt["sourceId"] != source or receipt["hostId"] != host
            or receipt["toolchain"] != TOOLCHAIN):
        raise ValueError("The CI bootstrap receipt has different build inputs.")
    compiler = receipt["compiler"]
    if not isinstance(compiler, str) or f"host: {HOST}" not in compiler.splitlines():
        raise ValueError("The CI bootstrap compiler identity has a different native host.")
    if not re.fullmatch(r"[a-f0-9]{64}", str(receipt["sha256"])):
        raise ValueError("The CI bootstrap receipt has an invalid executable digest.")
    executable = package / "bonsai-ci.exe"
    if executable_digest(executable) != receipt["sha256"]:
        raise ValueError("The CI bootstrap executable digest differs from its receipt.")
    return executable


def create(package: Path, source: str, host: str, compiler: str) -> Path:
    """Create the immutable package receipt only after a successful native build."""
    identity(source, host)
    receipt = {
        "schemaVersion": 1,
        "sourceId": source,
        "hostId": host,
        "toolchain": TOOLCHAIN,
        "compiler": compiler,
        "sha256": executable_digest(package / "bonsai-ci.exe"),
    }
    (package / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return verify(package, source, host)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("create", "verify"))
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--source", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--compiler", type=Path)
    options = parser.parse_args()
    try:
        if options.operation == "create":
            if options.compiler is None:
                raise ValueError("Receipt creation requires the native compiler identity.")
            executable = create(options.package, options.source, options.host,
                                options.compiler.read_text(encoding="utf-8-sig"))
        else:
            executable = verify(options.package, options.source, options.host)
        print(executable)
        return 0
    except (OSError, ValueError, TypeError, KeyError) as error:
        print(f"CI bootstrap verification failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
