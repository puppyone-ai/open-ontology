#!/usr/bin/env python3
"""Lane E19 entry for the unchanged shared T1/T8 workload and matrix hook."""
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools/oo-test"))
# Import with an accepted legacy lane; the actual resources belong to E19.
os.environ["OO_TEST_LANE"] = "e"
import t1

if __name__ == "__main__":
    t1.LANE = "E19"
    t1.main()
