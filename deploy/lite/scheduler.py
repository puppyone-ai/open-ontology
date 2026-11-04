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
    previous = json.loads(path.read_text()) if path.exists() else {'last_success':None,'last_failure':None}
    previous['last_success' if result.returncode==0 else 'last_failure'] = time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    previous['next_due'] = time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime(time.time()+30))
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(previous))
    temp.replace(path)
    time.sleep(30)
