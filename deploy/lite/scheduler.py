"""Call the existing scheduler; never treat an error as a successful cycle."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

def dispatch(project,worker=False):
    if str(uuid.UUID(project)) != project:
        raise ValueError('SCHEDULE_PROJECT')
    # Both processes use native_capture, never the proof-only pg_effect driver.
    command=['job-worker','--config','/work/operator/worker.json',
             'native-step' if worker else 'native-fire','--project',project]
    if worker:
        command+=['--worker','lite-worker']
    result=subprocess.run(command,capture_output=True)
    # Dispatch returning zero proves neither a task nor a completed cycle.
    # Authoritative job times and next_run_at live in the control database.
    return {'last_dispatch_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
            'dispatch_exit_code':result.returncode}

def main():
    if sys.argv[1:] not in ([],['--worker']):
        raise ValueError('SCHEDULE_ARGUMENT')
    os.umask(0o077)
    worker=bool(sys.argv[1:])
    config=json.loads(Path('/work/operator/schedule.json').read_text())
    path=Path('/work/worker-status.json' if worker else '/work/scheduler-status.json')
    while True:
        result=dispatch(config['project'],worker)
        temp=path.with_suffix('.tmp')
        temp.write_text(json.dumps(result))
        temp.replace(path)
        time.sleep(1 if worker else 30)

if __name__=='__main__':
    main()
