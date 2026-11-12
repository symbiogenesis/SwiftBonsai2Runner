"""Check exact action-lock coverage before GitHub tries to start a workflow.

This dependency-free policy check accepts block-style `uses:` and the generated
v0.0.3 actions.lock subset used here. External actions are currently leaves;
adopting a remote composite requires extending traversal before its transitive
lock entries can pass. This is not a general YAML syntax validator or an upstream
provenance verifier; gh-actions-lock --verify owns provenance.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys


SHA_ACTION = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*@([0-9a-fA-F]{40})\Z")
USES = re.compile(r"^\s*(?:-\s+)?(?:uses|'uses'|\"uses\"):\s*(.*)$")
BLOCK = re.compile(r"^\s*(?:-\s+)?(?:[\w-]+|'[^']+'|\"[^\"]+\"):\s*[|>][1-9+-]*\s*(?:#.*)?$")
FLOW_USES = re.compile(r"^\s*(?:-\s*)?\{.*(?:\buses|['\"]uses['\"])\s*:")


@dataclass(frozen=True)
class Reference:
    source: Path
    line: int
    value: str


def scalar(value: str) -> str:
    """Read one simple YAML string, rejecting multiline/anchored/tagged values."""
    value = value.strip()
    if value.startswith("'"):
        match = re.fullmatch(r"'((?:[^']|'')*)'\s*(?:#.*)?", value)
        if match:
            return match[1].replace("''", "'")
    elif value.startswith('"'):
        match = re.fullmatch(r'("(?:[^"\\]|\\.)*")\s*(?:#.*)?', value)
        if match:
            return json.loads(match[1])
    elif re.fullmatch(r"[A-Za-z0-9_.$/@+-]+(?:\s+#.*)?", value):
        return re.split(r"\s+#", value, maxsplit=1)[0]
    raise ValueError("expected a single simple string scalar (no flow mapping, anchor or multiline value)")


def references(source: Path) -> list[Reference]:
    result = []
    block_indent = None
    for number, line in enumerate(source.read_text(encoding="utf-8-sig").splitlines(), 1):
        indent = len(line) - len(line.lstrip(" "))
        if block_indent is not None:
            if not line.strip() or indent > block_indent:
                continue
            block_indent = None
        if "\t" in line[:indent + 1]:
            raise ValueError(f"{source}:{number}: YAML indentation must use spaces")
        match = USES.match(line)
        if match:
            try:
                value = scalar(match[1])
            except (ValueError, json.JSONDecodeError) as error:
                raise ValueError(f"{source}:{number}: unsupported uses syntax: {error}") from error
            result.append(Reference(source, number, value))
        elif FLOW_USES.match(line):
            raise ValueError(f"{source}:{number}: write uses as a block mapping so the preflight can inspect it")
        if BLOCK.match(line):
            block_indent = indent
    return result


def lockfile(source: Path) -> tuple[dict[str, set[str]], dict[str, dict[str, str | int]]]:
    """Read the deliberately small generated v0.0.3 schema, failing closed."""
    workflows: dict[str, set[str]] = {}
    dependencies: dict[str, dict[str, str | int]] = {}
    sections: set[str] = set()
    section = None
    entry = None
    version = None
    for number, line in enumerate(source.read_text(encoding="utf-8-sig").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        text = line.strip()
        try:
            if "\t" in line:
                raise ValueError("tabs are outside the generated lockfile subset")
            if indent == 0:
                entry = None
                if text.startswith("version:") and version is None:
                    version = scalar(text.removeprefix("version:"))
                    section = None
                elif text in ("workflows:", "dependencies:"):
                    section = text[:-1]
                    if section in sections:
                        raise ValueError(f"duplicate {section} section")
                    sections.add(section)
                else:
                    raise ValueError("unsupported or duplicate top-level field")
            elif indent == 4 and section in ("workflows", "dependencies"):
                match = re.fullmatch(r"(.+):(?:\s+(\[\]|\{\}))?", text)
                if not match:
                    raise ValueError("expected a generated workflow or dependency key")
                entry = scalar(match[1])
                entries = workflows if section == "workflows" else dependencies
                if entry in entries:
                    raise ValueError(f"duplicate key {entry}")
                if match[2] and match[2] != ("[]" if section == "workflows" else "{}"):
                    raise ValueError("wrong empty collection type")
                entries[entry] = set() if section == "workflows" else {}
            elif indent == 8 and entry is not None:
                if section == "workflows" and text.startswith("- "):
                    value = scalar(text[2:])
                    if value in workflows[entry]:
                        raise ValueError(f"duplicate dependency {value}")
                    workflows[entry].add(value)
                elif section == "dependencies":
                    key, separator, value = text.partition(":")
                    if not separator or key not in ("ref", "commit", "owner_id", "repo_id"):
                        raise ValueError("unsupported dependency field")
                    if key in dependencies[entry]:
                        raise ValueError(f"duplicate dependency field {key}")
                    if key in ("owner_id", "repo_id"):
                        if not re.fullmatch(r"[1-9][0-9]*", value.strip()):
                            raise ValueError(f"{key} must be a positive integer")
                        dependencies[entry][key] = int(value)
                    else:
                        dependencies[entry][key] = scalar(value)
                else:
                    raise ValueError("expected a workflow dependency list item")
            else:
                raise ValueError("unsupported generated lockfile structure or indentation")
        except (ValueError, json.JSONDecodeError) as error:
            raise ValueError(f"{source}:{number}: {error}") from error
    if version != "v0.0.3" or sections != {"workflows", "dependencies"}:
        raise ValueError(f"{source}: expected complete generated lockfile schema v0.0.3")
    for key, value in dependencies.items():
        if set(value) != {"ref", "commit", "owner_id", "repo_id"} or not value["ref"]:
            raise ValueError(f"{source}: incomplete dependency metadata for {key}")
    return workflows, dependencies


def check(root: Path) -> list[str]:
    root = root.resolve()
    errors: list[str] = []
    workflow_dir = root / ".github/workflows"
    workflows = sorted(path for path in workflow_dir.glob("*") if path.suffix in (".yml", ".yaml"))
    if not workflows:
        return [f"{workflow_dir}: no workflows found"]
    manifests = set(workflows)
    for name in ("action.yml", "action.yaml"):
        manifests.update((root / ".github/actions").rglob(name))
    refs: dict[Path, list[Reference]] = {}
    local: dict[tuple[Path, int], Path] = {}
    pending = list(sorted(manifests))
    while pending:
        source = pending.pop()
        if source in refs:
            continue
        try:
            refs[source] = references(source)
        except (OSError, ValueError) as error:
            errors.append(str(error))
            refs[source] = []
        for reference in refs[source]:
            value = reference.value
            location = f"{source.relative_to(root).as_posix()}:{reference.line}"
            if value.startswith("$/"):
                relative = Path(value[2:])
                target = (root / relative).resolve()
                if "@" in value or "\\" in value or ".." in relative.parts or not target.is_relative_to(root):
                    errors.append(f"{location}: invalid same-repository action reference {value}")
                    continue
                if target.is_dir():
                    candidates = [target / name for name in ("action.yml", "action.yaml") if (target / name).is_file()]
                else:
                    candidates = [target] if target.is_file() and target.suffix in (".yml", ".yaml") else []
                if len(candidates) != 1:
                    errors.append(f"{location}: same-repository reference must identify exactly one action/workflow: {value}")
                    continue
                local[(source, reference.line)] = candidates[0]
                pending.append(candidates[0])
            elif not SHA_ACTION.fullmatch(value):
                errors.append(f"{location}: use $/ for same-repository actions or a full 40-character commit SHA for external actions: {value}")

    def closure(source: Path, ancestors: frozenset[Path] = frozenset()) -> set[str]:
        if source in ancestors:
            raise ValueError(f"{source.relative_to(root).as_posix()}: same-repository action dependency cycle")
        result = set()
        for reference in refs[source]:
            target = local.get((source, reference.line))
            if target is not None:
                result.update(closure(target, ancestors | {source}))
            elif SHA_ACTION.fullmatch(reference.value):
                result.add(reference.value)
        return result

    try:
        locked_workflows, dependencies = lockfile(workflow_dir / "actions.lock")
    except (OSError, ValueError) as error:
        return errors + [str(error)]
    expected_names = {path.relative_to(root).as_posix() for path in workflows}
    for name in sorted(expected_names - locked_workflows.keys()):
        errors.append(f"actions.lock: missing workflow {name}")
    for name in sorted(locked_workflows.keys() - expected_names):
        errors.append(f"actions.lock: stale workflow {name}")
    all_dependencies: set[str] = set()
    for source in workflows:
        name = source.relative_to(root).as_posix()
        try:
            expected = closure(source)
        except ValueError as error:
            errors.append(str(error))
            continue
        all_dependencies.update(expected)
        actual = locked_workflows.get(name, set())
        for value in sorted(expected - actual):
            errors.append(f"{name}: lockfile missing exact uses key {value} (matching commit metadata under a tag key is insufficient)")
        for value in sorted(actual - expected):
            errors.append(f"{name}: lockfile has stale uses key {value}")
    for value in sorted(all_dependencies - dependencies.keys()):
        errors.append(f"actions.lock: missing dependency metadata for exact uses key {value}")
    for value in sorted(dependencies.keys() - all_dependencies):
        errors.append(f"actions.lock: stale dependency metadata for {value}")
    for value in sorted(all_dependencies & dependencies.keys()):
        sha = SHA_ACTION.fullmatch(value)[1]
        if dependencies[value]["commit"] != "sha1-" + sha:
            errors.append(f"actions.lock: commit metadata disagrees with exact uses SHA for {value}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    arguments = parser.parse_args()
    errors = check(arguments.root)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print("Workflow startup policy passed: exact action-lock coverage and full-SHA external references.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
