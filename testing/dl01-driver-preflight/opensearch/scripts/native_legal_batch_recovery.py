#!/usr/bin/env python3
"""Run a new legitimate C5/Loader generation after a closed batch-token refusal."""
import hashlib
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('resume', HERE / 'native_resume_jobs.py')
resume = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resume)
jobs = resume.jobs


def main():
    base = jobs.t.BASE / 'current-20261013/legal-batch'
    base.mkdir(parents=True, exist_ok=False)
    jobs.OUT = base
    jobs.t.OUT = base
    old = jobs.t.BASE / 'current-20261013/recovery/manifest-repaired.json'
    manifest = json.loads(old.read_text())
    producer = HERE / 'native_c5_product_producer.py'
    manifest['producer']['sha256'] = hashlib.sha256(producer.read_bytes()).hexdigest()
    new = base / 'manifest.json'
    new.write_text(json.dumps(manifest, indent=2) + '\n')
    jobs.worker('put', ['native-put', '--manifest', str(new)])
    jobs.worker('enable', ['native-enable', '--binding', manifest['binding'], '--enabled', 'true'])
    jobs.worker('fire', ['native-fire', '--project', manifest['project_id']])
    jobs.worker('step', ['native-step', '--project', manifest['project_id'], '--worker', 'opensearch-native-owner44'])
    jobs.worker('state', ['native-state', '--binding', manifest['binding']])
    jobs.worker('pause', ['native-enable', '--binding', manifest['binding'], '--enabled', 'false'])
    print('LEGAL_BATCH_NEW_REAL_C5_STEP_DONE')


if __name__ == '__main__':
    main()
