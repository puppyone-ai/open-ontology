#!/usr/bin/env python3
"""Admit 19c to lane T's unmodified runner; do not alter its audit rules.

The current T7 runner only emits Oracle C1 verdicts for 23. Until lane T
integrates 19c C1 coverage, this entry cannot claim a 19c C1 audit pass.
"""
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools/oo-test"))
import t7

if __name__ == "__main__":
    t7.LANE = "E19"
    t7.VERSIONS["oracle"] = ["19"]
    raise SystemExit(t7.main())
