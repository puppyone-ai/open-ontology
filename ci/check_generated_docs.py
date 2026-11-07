#!/usr/bin/env python3
"""Local/CI entry point: rebuild oo, test generators, reject generated-doc drift."""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli"], cwd=ROOT, check=True)
subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", "tools/docs"],
               cwd=ROOT, check=True)
for script in ("gen_cli_reference.py", "gen_dba_grants.py", "gen_error_reference.py"):
    subprocess.run([sys.executable, str(ROOT / "tools/docs" / script), "--check"],
                   cwd=ROOT, check=True)
