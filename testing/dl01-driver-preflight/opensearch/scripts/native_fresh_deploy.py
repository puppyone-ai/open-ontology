#!/usr/bin/env python3
"""Deploy frozen native Loader and real jobs migrations to own fresh target."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tarfile
import io

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('target', HERE / 'native_fresh_target.py')
t = importlib.util.module_from_spec(spec)
spec.loader.exec_module(t)
FIXED = 'b593e053e93da2c767a4d0bf19f1fa7d94332fdc'
ELF = Path('/tmp/oo-native-loader-20261012-linux194')
SHA = 'daca92824e5dde50d58f1bc81fadddbe49b303f0847f016e24301d2f06fb6903'
ARCHIVE = Path('/tmp/oo-dl01w1-44-native-loader-fixed')
OWNER = '5d3f04f6-15de-4cf5-af60-62f24974cc76'


def pipeline(name, command, body):
    request = t.OUT / (name + '-request.json')
    request.write_text(json.dumps(body) + '\n')
    t.phase(name, ['docker', 'run', '--rm', '-i', '--pull', 'never',
        '--network', 'container:' + t.NAME, '--user', '0:0', '--security-opt', 'no-new-privileges',
        '--cap-drop', 'ALL', '--entrypoint', '/opt/oo-pipeline',
        '-v', str(ELF) + ':/opt/oo-pipeline:ro', '-v', str(t.PRIVATE) + ':/private:ro',
        'oo-a395-selfbuild:local', command], request.read_bytes())


def main():
    if hashlib.sha256(ELF.read_bytes()).hexdigest() != SHA:
        raise SystemExit('fixed Loader ELF drift')
    ARCHIVE.mkdir(mode=0o700, exist_ok=False)
    raw = subprocess.check_output(['git', 'archive', FIXED, 'crates/oo-jobs'])
    with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
        archive.extractall(ARCHIVE, filter='data')
    target = {'host': '127.0.0.1', 'port': 5432, 'database': t.DB,
        'runtime_principal': 'postgres', 'password_file': '/private/native-admin.pw',
        'ca_file': '/private/certs/ca.pem', 'ssl': 'verify-full',
        'owner': OWNER, 'shard': 'shard_oo_dl01w1_44'}
    pipeline('install-native-update', 'install-native-update', {'protocol': 'oo.pipeline.install-native-update/v1', 'target': target})
    admin = {'host': '127.0.0.1', 'port': 5432, 'database': t.DB,
        'administrator': 'postgres', 'password_file': '/private/native-admin.pw', 'ca_file': '/private/certs/ca.pem'}
    pipeline('install-sealed-catalog', 'install-sealed-catalog', {'protocol': 'oo.pipeline.install-sealed-catalog/v1', 'target': admin})
    pipeline('install-native-sealed-catalog', 'install-native-sealed-catalog', {'protocol': 'oo.pipeline.install-native-sealed-catalog/v1', 'target': admin})
    t.phase('runtime-pipeline-membership', ['docker', 'exec', '-i', t.NAME, 'psql', '-X', '-U', 'postgres', '-d', t.DB, '-v', 'ON_ERROR_STOP=1'],
        b'GRANT oo_pipeline_runtime TO oo_dl01w1_44_runtime WITH INHERIT TRUE, SET FALSE;\n')
    t.phase('jobs-deploy', ['python3', str(ARCHIVE / 'crates/oo-jobs/tools/deploy.py'),
        '--container', t.NAME, '--database', t.DB, '--admin', 'postgres', '--loader-role', 'oo_dl01w1_44_runtime'])
    print('PRODUCT_SCHEMAS_AND_REAL_JOBS_DEPLOYED: no source import or lease claim synthesized')


if __name__ == '__main__':
    main()
