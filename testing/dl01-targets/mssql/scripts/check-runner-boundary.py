#!/usr/bin/env python3
"""No Docker mutation or EULA acceptance: check authorization before side effects."""
import os
from pathlib import Path
import subprocess
import tempfile

with tempfile.TemporaryDirectory(prefix='oo-mssql-runner-check-') as scratch:
    secret_dir = Path(scratch) / 'secrets'
    env = os.environ.copy()
    env.pop('OO_DL01W1_09_ACCEPT_EULA', None)
    env['OO_DL01W1_09_SECRET_DIR'] = str(secret_dir)
    runner = Path(__file__).with_name('run-amd64.sh')
    result = subprocess.run(['bash', str(runner)], env=env, capture_output=True)
    assert result.returncode == 43, result.returncode
    assert b'explicit operator EULA authorization required' in result.stderr
    assert not secret_dir.exists(), 'unauthorized runner created credentials'
print('runner authorization refusal: exit43, no credential creation, no Docker action')
