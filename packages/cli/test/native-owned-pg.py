#!/usr/bin/env python3
"""Reuse the owned PG runner unchanged, selecting the standalone CLI entrypoint.

Only token approve remains on the npm surface; all enroll/device/login/query
and prompt assertions are the existing runner's assertions and gold values.
"""
from pathlib import Path
import json
import runpy
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
TESTS = ROOT / "crates/oo-gateway/tests"
original_popen = subprocess.Popen
native_calls = {}


class NativePopen(original_popen):
    def __init__(self, args, *positional, **keywords):
        if (isinstance(args, list) and len(args) >= 3
                and args[0] == "node"
                and str(args[1]).endswith("npm-cli/bin/oo.mjs")
                and args[2] in ("login", "whoami", "share-prompt", "query")):
            command = args[2]
            native_calls[command] = native_calls.get(command, 0) + 1
            args = [str(ROOT / "target/debug/oo"), *args[2:]]
        super().__init__(args, *positional, **keywords)


subprocess.Popen = NativePopen
sys.path.insert(0, str(TESTS))
try:
    runpy.run_path(str(TESTS / "token_owned_pg.py"), run_name="__main__")
finally:
    subprocess.Popen = original_popen
assert all(native_calls.get(command, 0) > 0
           for command in ("login", "whoami", "share-prompt", "query"))
(Path(sys.argv[1]) / "native-entrypoint-summary.json").write_text(
    json.dumps({"native_commands": native_calls, "assertions": "unchanged owned PG runner"}, indent=2)
)
