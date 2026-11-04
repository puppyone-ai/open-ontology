"""Call the existing scheduler; never treat an error as a successful cycle."""
import json
import os
from pathlib import Path
import subprocess
import time

os.umask(0o077)
config = json.loads(Path('/work/operator/schedule.json').read_text())
# Fixed executable and argv. This file is not a general-purpose command runner.
project = config['project']
import uuid
if str(uuid.UUID(project)) != project:
    raise ValueError('SCHEDULE_PROJECT')
while True:
    result = subprocess.run(['job-worker','--config','/work/operator/worker.json',
                             'native-fire','--project',project],capture_output=True)
    path = Path('/work/scheduler-status.json')
    # Dispatch returning zero proves neither a task nor a completed cycle.
    # Authoritative job times and next_run_at live in the control database.
    previous = {'last_dispatch_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
                'dispatch_exit_code':result.returncode}
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(previous))
    temp.replace(path)
    time.sleep(30)
