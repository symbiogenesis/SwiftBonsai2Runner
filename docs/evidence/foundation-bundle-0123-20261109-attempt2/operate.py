"""Prepared entrypoint for one separately authorized source-ready Foundation attempt."""

import importlib.util
from pathlib import Path
import sys
import time


def main():
    started = time.monotonic()
    entrypoint = Path(__file__).resolve()
    base_path = entrypoint.parent.parent / "foundation-bundle-0123-20261109/operate.py"
    spec = importlib.util.spec_from_file_location("source_ready_foundation_operator", base_path)
    operator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(operator)  # Imports definitions only; its main guard does not run.
    sys.argv.extend(["--source-ready", "--declaration", str(entrypoint.with_name("workload-plan.json")),
                     "--entrypoint", str(entrypoint)])
    return operator.main(started=started)


if __name__ == "__main__":
    raise SystemExit(main())
