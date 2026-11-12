"""Check staged workflow startup policy without modifying the index or using the network.

Only staged YAML manifests and actions.lock are copied to temporary storage. This
keeps partially staged repairs in the working tree from hiding a broken commit.
"""

from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import time

from check_workflow_startup import check


LOCK = ".github/workflows/actions.lock"
GUARDS = {
    ".githooks/pre-commit",
    "tools/ci/check_staged_workflows.py",
    "tools/ci/check_workflow_startup.py",
}


def manifest(path: str) -> bool:
    return PurePosixPath(path).suffix in (".yml", ".yaml")


def relevant(path: str) -> bool:
    # Same-repository references can point outside .github/actions. Conservatively
    # include YAML changes anywhere, while ordinary Rust/docs commits stay cheap.
    return manifest(path) or path == LOCK or path in GUARDS


class Git:
    def __init__(self, root: Path, executable: str):
        self.root = root
        self.executable = executable
        self.deadline = time.monotonic() + 30

    def run(self, *arguments: str, data: bytes | None = None) -> bytes:
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError("staged startup check exceeded its 30-second Git allowance")
        result = subprocess.run(
            [self.executable, *arguments], cwd=self.root, input=data,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=remaining,
            check=False,
        )
        if result.returncode:
            detail = result.stderr.decode("utf-8", errors="replace").strip()
            raise ValueError(f"git {arguments[0]} failed ({result.returncode}): {detail[:2000]}")
        return result.stdout


def check_staged(root: Path, git_executable: str = "git") -> tuple[bool, list[str]]:
    """Return whether CI inputs changed and any errors in their staged snapshot."""
    selected = True
    try:
        git = Git(root.resolve(), git_executable)
        changed = git.run("diff", "--cached", "--name-only", "--no-renames", "-z", "--")
        paths = changed.decode("utf-8").split("\0")
        selected = any(relevant(path) for path in paths if path)
        if not selected:
            return False, []

        entries: dict[str, str] = {}
        index = git.run("ls-files", "--cached", "--stage", "--full-name", "-z")
        for record in index.split(b"\0"):
            if not record:
                continue
            metadata, raw_path = record.split(b"\t", 1)
            path = raw_path.decode("utf-8")
            if not (manifest(path) or path == LOCK):
                continue
            mode, oid, stage = metadata.decode("ascii").split()
            if stage != "0":
                raise ValueError(f"unmerged staged workflow input: {path}")
            if mode not in ("100644", "100755"):
                raise ValueError(f"staged workflow input must be a regular file: {path}")
            if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", oid):
                raise ValueError(f"invalid staged object identity: {path}")
            entries[path] = oid

        # Read immutable object IDs, rather than worktree paths or checkout-index;
        # the guard neither changes staged bytes nor writes Git metadata.
        identities = list(dict.fromkeys(entries.values()))
        objects: dict[str, bytes] = {}
        if identities:
            raw = git.run("cat-file", "--batch", data=("\n".join(identities) + "\n").encode("ascii"))
            offset = 0
            for oid in identities:
                end = raw.find(b"\n", offset)
                if end < 0:
                    raise ValueError("incomplete staged object response")
                header_oid, kind, size = raw[offset:end].decode("ascii").split()
                if header_oid != oid or kind != "blob" or not size.isdecimal():
                    raise ValueError(f"invalid staged object response for {oid}")
                start = end + 1
                offset = start + int(size)
                if raw[offset:offset + 1] != b"\n":
                    raise ValueError(f"incomplete staged blob for {oid}")
                objects[oid] = raw[start:offset]
                offset += 1
            if offset != len(raw):
                raise ValueError("unexpected trailing staged object response")

        with tempfile.TemporaryDirectory(prefix="bonsai-staged-workflows-") as scratch:
            snapshot = Path(scratch).resolve()
            for path, oid in entries.items():
                relative = PurePosixPath(path)
                target = (snapshot / path).resolve()
                if relative.is_absolute() or ".." in relative.parts or "\\" in path or not target.is_relative_to(snapshot):
                    raise ValueError(f"unsafe staged workflow path: {path}")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(objects[oid])
            errors = check(snapshot)
            return True, [error.replace(str(snapshot), "staged index") for error in errors]
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        return selected, [str(error)]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    arguments = parser.parse_args()
    selected, errors = check_staged(arguments.root)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        print("Commit blocked: repair and stage the workflow/action and matching actions.lock together.", file=sys.stderr)
        return 1
    if selected:
        print("Staged workflow startup policy passed (offline; upstream provenance and hosted CI remain separate).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
