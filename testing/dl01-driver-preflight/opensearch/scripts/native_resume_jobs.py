#!/usr/bin/env python3
"""Resume the original C5 source through the fixed native worker and Loader."""
import importlib.util
from pathlib import Path

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('jobs', here / 'native_fresh_jobs.py')
jobs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(jobs)

jobs.ELF = Path('/tmp/oo-native-capture-worker-20261012-r12')
jobs.SHA = '87de41e5073cbd7cc8711466c3b92bd20e8ee74cec76423d92fbde5ed5b97723'
jobs.PIPE = Path('/tmp/oo-native-loader-minimal-20261012-linux194')
jobs.PIPE_SHA = '26855c12b869388b1a9a4590fe6f9521ae7f62685817428078bf51be40e67c18'
jobs.ADM = Path('/tmp/oo-dl01w1-02-native-admission-minimal')
jobs.ADM_SHA = '0b584453877a4c5ec3e7c311c199ade8af723229ea2ab56745858e37f7a1e274'
jobs.OUT = jobs.t.BASE / 'current-20261013/recovery'
jobs.t.OUT = jobs.OUT

if __name__ == '__main__':
    jobs.main()
