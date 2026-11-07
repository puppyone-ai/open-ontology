#!/usr/bin/env python3
"""Run Python unittest suites of touched tools in the lane tree and the base tree.

Usage: pytests.py <lane-tree> <base-tree> <tools/dir> [...]
Exits 1 only when the lane tree fails a test the base tree does not fail, or a
suite errors at import only in the lane tree. Pre-existing failures are printed
and tolerated (plan §10, common card rule 9).
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import sys

RESULT = re.compile(r'^(ERROR|FAIL): (\S+) \((\S+)\)', re.M)


def suites(tree: Path, tool: str) -> list[Path]:
    root = tree / tool
    if not root.is_dir():
        return []
    dirs = sorted({p.parent for p in root.rglob('test_*.py') if 'target' not in p.parts and 'node_modules' not in p.parts})
    return dirs


def run(tree: Path, suite: Path) -> tuple[set[str], str]:
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
    (tree / 'evidence').mkdir(exist_ok=True)
    r = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(suite), '-p', 'test_*.py'],
                       cwd=tree, env=env, capture_output=True, text=True, timeout=1800)
    out = r.stdout + r.stderr
    failed = {f'{m[2]}.{m[1]}' for m in RESULT.findall(out)}
    tail = out.strip().splitlines()[-1] if out.strip() else ''
    if r.returncode != 0 and not failed:
        failed = {f'<suite {suite.name} rc={r.returncode}>'}
    return failed, tail


def main() -> int:
    lane, base, tools = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:]
    new_failures = []
    for tool in tools:
        for suite in suites(lane, tool):
            rel = suite.relative_to(lane)
            lf, ltail = run(lane, suite)
            bf, _ = run(base, base / rel) if (base / rel).is_dir() else (set(), '')
            fresh = sorted(lf - bf)
            print(f'{rel}: {ltail}; failing {len(lf)} (pre-existing {len(lf & bf)}, new {len(fresh)})')
            for t in sorted(lf & bf):
                print(f'  pre-existing: {t}')
            for t in fresh:
                print(f'  NEW: {t}')
            new_failures += fresh
    return 1 if new_failures else 0


if __name__ == '__main__':
    raise SystemExit(main())
