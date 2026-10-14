#!/usr/bin/env python3
"""Owned MariaDB product close. One build, then Gateway, dialogue, and cancel."""
import hashlib, json, os, pathlib, shutil, socket, ssl, subprocess, sys, time, urllib.request

FLOW = pathlib.Path('/Users/redux/puppy-one/open-ontology/tools/source-onboarding/product_flow.py')
DIALOGUE = pathlib.Path('/Users/redux/puppy-one/open-ontology/tools/agent-product-verifier/dialogue.py')
ROOT = pathlib.Path('/Users/redux/puppy-one/open-ontology')
CACHE = pathlib.Path('/Users/redux/.cache/oo-dl01w1-71')
BEARER = pathlib.Path('/tmp/oo-product-model-20261012/bearer')
API_ELF = pathlib.Path('/tmp/oo-pg46-runtime-finish/selfbuild-control-api-observation-fixed')
PG_BUILDER = pathlib.Path('/tmp/oo-pg46-runtime-finish/continuous/pg-builder-current')
PIPELINE = pathlib.Path('/tmp/oo-pg46-runtime-finish/oo-pipeline')
GW_ELF = pathlib.Path('/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-gateway-51f007a8b3463086dc3fde8093689137ea8282327382a4cd245bf2d7cac8dcee')
CLI_ELF = pathlib.Path('/Users/redux/.cache/oo-dl01w1-73/fixed-elf/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703')
ISSUE = pathlib.Path('/tmp/oo-pg46-runtime-finish/issue-token')
PINS = {
    API_ELF: '025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae',
    PG_BUILDER: '58e5eed3d88f22c2b27c758b7fb064258188eb09ce379e74eac66c5af741a91d',
    PIPELINE: '2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4',
    GW_ELF: '51f007a8b3463086dc3fde8093689137ea8282327382a4cd245bf2d7cac8dcee',
    CLI_ELF: 'fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703',
    ISSUE: '0554085cb2e872373ecfdb136821197e5c0aa30536aa6725855b5e9127e57052',
}
SOURCE_COMMIT = 'd34813ee44ec558460ee571ef9f37f1ecdd1e026'

def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

def write(path, value):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')
    os.chmod(path, 0o600)

def secret(path):
    path = pathlib.Path(path)
    if path.exists():
        os.chmod(path, 0o600)
        return
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.write(fd, (os.urandom(24).hex() + '\n').encode())
    os.close(fd)

def kept(proc, directory):
    directory = pathlib.Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / 'stdout').write_bytes(proc.stdout or b'')
    (directory / 'stderr').write_bytes(proc.stderr or b'')
    (directory / 'exit').write_text(str(proc.returncode) + '\n')
    for child in directory.iterdir():
        os.chmod(child, 0o600)

def run(argv, sink, timeout=120):
    proc = subprocess.run(argv, capture_output=True, timeout=timeout)
    kept(proc, sink)
    if proc.returncode:
        raise RuntimeError(pathlib.Path(sink).name + ' refused')
    return proc

def state(name):
    proc = subprocess.run(['docker', 'inspect', '-f', '{{.State.Status}}', name], capture_output=True, text=True, timeout=20)
    return proc.stdout.strip() if proc.returncode == 0 else 'absent'

def psql(pg, sql, database='postgres'):
    proc = subprocess.run(['docker', 'exec', '-i', pg, 'psql', '-U', 'postgres', '-d', database, '-v', 'ON_ERROR_STOP=1', '-X', '-q', '-t', '-A'], input=sql.encode(), capture_output=True, timeout=60)
    if proc.returncode:
        err = '\n'.join(line for line in proc.stderr.decode('utf-8', 'replace').splitlines() if 'PASSWORD' not in line.upper())
        raise RuntimeError('postgres refused: ' + err[:400])
    return proc.stdout.decode()

def identity_status():
    raw = socket.create_connection(('127.0.0.1', 18441), 3)
    try:
        context = ssl.create_default_context()
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        secured = context.wrap_socket(raw)
        secured.settimeout(3)
        secured.send(b'GET /control/v1/identity/current HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n')
        return secured.recv(80).split(b'\r\n', 1)[0].decode('ascii', 'replace')
    finally:
        raw.close()

def pins():
    for path, expect in PINS.items():
        if sha(path) != expect:
            raise RuntimeError('elf pin mismatch')

def role_sql(role, path):
    secret(path)
    password = path.read_text().strip().replace("'", "''")
    extra = 'NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION'
    return ("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='%s') THEN "
            "CREATE ROLE %s LOGIN PASSWORD '%s' %s; ELSE ALTER ROLE %s WITH LOGIN PASSWORD '%s' %s; END IF; END $$;") % (
            role, role, password, extra, role, password, extra)

def ensure_db(pg, name, owner):
    found = psql(pg, "SELECT 1 FROM pg_database WHERE datname='%s'" % name).strip()
    if found != '1':
        psql(pg, 'CREATE DATABASE %s OWNER %s' % (name, owner))

def mint_api_tls(private, openssl):
    if (private / 'server.crt').exists():
        return
    # ponytail: one owned CA for this API listen; the product client pins this file.
    # Host Python rejects a CA that has no keyUsage.
    run([openssl, 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(private / 'ca.key'), '-out', str(private / 'ca.crt'), '-days', '30', '-subj', '/CN=Maria connection API', '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'], private / 'tls-ca')
    run([openssl, 'req', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(private / 'server.key'), '-out', str(private / 'server.csr'), '-subj', '/CN=127.0.0.1'], private / 'tls-req')
    (private / 'server.ext').write_text('basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n')
    os.chmod(private / 'server.ext', 0o600)
    run([openssl, 'x509', '-req', '-in', str(private / 'server.csr'), '-CA', str(private / 'ca.crt'), '-CAkey', str(private / 'ca.key'), '-CAcreateserial', '-out', str(private / 'server.crt'), '-days', '30', '-extfile', str(private / 'server.ext')], private / 'tls-sign')

def place(src, dest, mode):
    dest = pathlib.Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if not dest.exists() or sha(dest) != sha(src):
        shutil.copyfile(src, dest)
    os.chmod(dest, mode)

def runtime_doc(root, pg, bundle, columns):
    commit = sha(bundle / 'manifest.json')
    private = root / 'api-private'
    base = str(root)
    principals = []
    for subject, role, principal, label, cred in (
        ('fixture-reviewer', 'reviewer', 'native-reviewer', 'control_plane_test', str(private / 'reviewer.credential')),
        ('fixture-owner', 'owner', 'native-owner', 'fixture-owner', str(private / 'owner.credential')),
        ('fixture-auditor', 'auditor', 'native-auditor', 'fixture-auditor', str(private / 'auditor.credential')),
    ):
        principals.append({'subject': subject, 'ledger_role': role, 'principal_id': principal, 'reviewer_label': label, 'credential_file': cred, 'object_scope': columns})
    pgca = str(root / 'private/ca.crt')
    doc = {
        'contract': 'oo.selfbuild-control-api.config/v1',
        'deployment_origins': ['https://127.0.0.1:18441'],
        'listen': {'bind': '0.0.0.0:18454', 'cert_file': str(private / 'server.crt'), 'key_file': str(private / 'server.key')},
        'identity': {'origin': 'https://127.0.0.1:18441', 'ca_file': str(private / 'identity-ca.pem')},
        'postgres': {'host': pg, 'port': 5432, 'database': 'oo_maria_conn_selfbuild', 'user': 'oo_maria_conn_selfbuild_api_admin', 'password_file': str(private / 'api-admin.password'), 'ca_file': pgca, 'server_name': pg},
        'ledger': {'host': pg, 'port': 5432, 'database': 'oo_maria_conn_selfbuild', 'user': 'oo_maria_conn_selfbuild_api_admin', 'password_file': str(private / 'api-admin.password'), 'ca_file': pgca, 'server_name': pg},
        'work_dir': str(root / 'work'), 'model_cache_dir': str(root / 'model-cache'),
        'binaries': {'catalog': base + '/bin/oo-pipeline', 'pg_builder': base + '/bin/pg-builder', 'rules': base + '/bin/pg-builder', 'suggestions': base + '/bin/pg-builder', 'review_ledger': base + '/bin/pg-builder'},
        'projects': [{
            'project_id': 'mariadb', 'config_commit': commit, 'source_ref': 'mariadb_connection_whole_src',
            'allowance_id': 'mariadb_connection_whole_allow', 'llm_secret_ref': 'secret://llm/mariadb-connection-whole',
            'source_secret_ref': 'secret://source/mariadb-connection-whole',
            'source': {'host': 'unused.invalid', 'port': 5432, 'database': 'unused', 'username': 'unused', 'password_file': str(private / 'api-admin.password'), 'ca_file': pgca,
                       'tables': [{'schema': name.split('.')[0], 'table': name.split('.')[1]} for name in ('logistics.hubs', 'logistics.shipments', 'logistics.tracking_events', 'cust_east.k_note', 'cust_west.k_meter')]},
            'sample_policy': 'none', 'rules_auto_accept': 'none',
            # gpt-6.1-sol returned usage_limit_reached with reset_seconds 518945 on build b_aee6dce94b73dbd6b8369bc0a5c0dc74.
            'llm': {'bearer_file': str(private / 'llm-bearer'), 'origin': 'http://127.0.0.1:8317', 'model_id': 'grok-4.7'},
            'role_policy_file': str(bundle / 'roles.json'), 'hidden_columns': [], 'visible_columns': columns,
            'sealed_bundle': str(bundle),
            'source_protocol': {'catalog': 'oo.pipeline.sealed-import-collection/v1', 'profile': 'oo.source.mariadb.profile/v1', 'semantics': 'oo.source.mariadb.audit/v1', 'source_commit': SOURCE_COMMIT, 'semantics_commit': SOURCE_COMMIT, 'type_basis': [], 'schema_audit': []},
        }],
        'principals': principals,
    }
    return doc, commit

def api_accepts(port, cafile):
    context = ssl.create_default_context(cafile=cafile)
    try:
        urllib.request.urlopen('https://127.0.0.1:%s/control/v1/builds' % port, context=context, timeout=5)
        return True
    except Exception as exc:
        return bool(getattr(exc, 'code', None))

def ensure_api(cfg, root, pg, runtime, bundle, columns):
    private = root / 'api-private'
    private.mkdir(mode=0o700, exist_ok=True)
    os.chmod(private, 0o700)
    if (private / 'ca.crt').exists() and api_accepts(cfg['api_port'], str(private / 'ca.crt')):
        return runtime
    pins()
    mint_api_tls(private, cfg['openssl'])
    psql(pg, role_sql('oo_maria_conn_selfbuild_api_admin', private / 'api-admin.password'))
    psql(pg, role_sql('oo_token_admin', private / 'token-admin.password'))
    psql(pg, role_sql('oo_token_runtime', private / 'token-runtime.password'))
    psql(pg, role_sql('oo_maria_conn_data', private / 'data.password'))
    ensure_db(pg, 'oo_maria_conn_selfbuild', 'oo_maria_conn_selfbuild_api_admin')
    ensure_db(pg, 'oo_maria_conn_token', 'oo_token_admin')
    place(API_ELF, root / 'bin/api', 0o755)
    place(PG_BUILDER, root / 'bin/pg-builder', 0o755)
    place(PIPELINE, root / 'bin/oo-pipeline', 0o755)
    place(CACHE / 'native-generic/identity_forward.py', root / 'bin/identity_forward.py', 0o700)
    place(CACHE / 'native-generic/model_loopback_forward.py', root / 'bin/model_forward.py', 0o700)
    place(BEARER, private / 'llm-bearer', 0o600)
    place(CACHE / 'native-identity-private/ca.pem', private / 'identity-ca.pem', 0o600)
    place(CACHE / 'native-ledger-reviewer.credential', private / 'reviewer.credential', 0o600)
    place(CACHE / 'native-ledger-owner.credential', private / 'owner.credential', 0o600)
    place(CACHE / 'native-ledger-auditor.credential', private / 'auditor.credential', 0o600)
    doc, _commit = runtime_doc(root, pg, bundle, columns)
    if not (private / 'schema-installed').exists():
        write(private / 'runtime.json', doc)
    (root / 'work').mkdir(mode=0o700, exist_ok=True)
    (root / 'model-cache').mkdir(mode=0o700, exist_ok=True)
    start = private / 'start.sh'
    base = str(root)
    # The fixed API exits without a message. Record the code. A second forward bind must not skip install.
    start.write_text('#!/bin/sh\nset -eu\n(python3 %s/bin/identity_forward.py >>%s/api-private/identity-forward.log 2>&1 || true) &\n(python3 %s/bin/model_forward.py >>%s/api-private/model-forward.log 2>&1 || true) &\nsleep 1\nif [ ! -f %s/api-private/schema-installed ]; then\n  set +e\n  %s/bin/api install --config %s/api-private/runtime.json >%s/api-private/install.stdout 2>%s/api-private/install.stderr\n  code=$?\n  set -e\n  if [ "$code" -ne 0 ]; then echo "exit:$code" >>%s/api-private/install.stderr; exit "$code"; fi\n  touch %s/api-private/schema-installed\nfi\nset +e\n%s/bin/api serve --config %s/api-private/runtime.json >%s/api-private/serve.stdout 2>%s/api-private/serve.stderr\ncode=$?\necho "exit:$code" >>%s/api-private/serve.stderr\nexit "$code"\n' % (base, base, base, base, base, base, base, base, base, base, base, base, base, base, base, base))
    os.chmod(start, 0o700)
    # The owned runtime already shares the target netns and resolves host.docker.internal.
    # A second container cannot add that host while joining the netns.
    proc = subprocess.run(['docker', 'exec', '-d', '-u', '%s:%s' % (os.getuid(), os.getgid()), runtime, '/bin/sh', str(start)], capture_output=True, timeout=30)
    kept(proc, private / 'api-exec')
    if proc.returncode:
        raise RuntimeError('api exec refused')
    last = None
    for _ in range(40):
        if api_accepts(cfg['api_port'], str(private / 'ca.crt')):
            return runtime
        time.sleep(1)
    err = ''
    stderr = private / 'install.stderr'
    if stderr.exists():
        err = '\n'.join(line for line in stderr.read_text(errors='replace').splitlines() if 'password' not in line.lower())[-400:]
    raise RuntimeError('api not accepting: ' + (err or 'no install error'))

def product_config(cfg, root, bundle, columns):
    path = root / 'product-config.json'
    if path.exists():
        return
    write(path, {
        'endpoint': 'https://127.0.0.1:%s' % cfg['api_port'], 'origin': 'https://127.0.0.1:18441', 'ca': str(root / 'api-private/ca.crt'),
        'identity_origin': 'https://127.0.0.1:18441', 'identity_ca': str(CACHE / 'native-identity-private/ca.pem'),
        'cookies': {'admin': str(CACHE / 'native-identity-private/fixture-admin.cookie'), 'reviewer': str(CACHE / 'native-identity-private/fixture-reviewer.cookie'), 'owner': str(CACHE / 'native-identity-private/fixture-owner.cookie')},
        'operator_sha256': sha(ROOT / 'tools/selfbuild-control-api/recipes/operator.py'),
        'export_oos_sha256': sha(ROOT / 'tools/selfbuild-control-api/recipes/export_oos.py'),
        'fixture_authorized': True,
        'create': {'allowance_id': 'mariadb_connection_whole_allow', 'config_commit': sha(bundle / 'manifest.json'), 'llm_secret_ref': 'secret://llm/mariadb-connection-whole', 'project_id': 'mariadb', 'source_ref': 'mariadb_connection_whole_src'},
        'work_dir_container': str(root / 'work'), 'work_dir_host': str(root / 'work'), 'catalog_sha256': sha(bundle / 'catalog.json'), 'model_deadline_seconds': 2400,
    })

def run_product(root):
    product = root / 'product'
    receipt = product / 'product-receipt.json'
    if receipt.exists():
        return
    config = root / 'product-config.json'
    if not (product / 'prepared.json').exists():
        proc = subprocess.run([sys.executable, str(FLOW), '--config', str(config), '--out', str(product), '--prepare-only'], capture_output=True, timeout=2600)
        kept(proc, root / 'product-prepare')
        if proc.returncode:
            raise RuntimeError('product prepare failed')
    overlay = json.loads((product / 'prepared-overlay.json').read_text())
    columns, reviews = [], []
    for column in overlay['columns']:
        name = column['dataset'] + '.' + column['name']
        columns.append(name)
        if column.get('classification_state') == 'pending':
            reviews.append({'name': name, 'binds_fact': column['fact_sha256'], 'decision': 'confirmed'})
    if len(columns) != 36:
        raise RuntimeError('overlay column count')
    decisions = root / 'decisions.json'
    if not decisions.exists():
        write(decisions, {'reviews': reviews, 'confirm_columns': columns, 'role': 'oo_r_mariadb_full', 'fixture_authorized': True})
    proc = subprocess.run([sys.executable, str(FLOW), '--config', str(config), '--out', str(product), '--decisions', str(decisions)], capture_output=True, timeout=600)
    kept(proc, root / 'product-decide')
    if proc.returncode:
        raise RuntimeError('product decisions failed')

def pack_identity(role_pack):
    manifest = json.loads(next(item['text'] for item in role_pack['files'] if item['path'] == 'manifest.json'))
    readme = next(item['text'] for item in role_pack['files'] if item['path'] == 'README.md')
    role = next(line.split('`')[1] for line in readme.splitlines() if line.startswith('role:'))
    return {'project': manifest['project'], 'role': role, 'policy_version': manifest['policy']['version'], 'knowledge_version': manifest['knowledge']['version'], 'policy_id': manifest['policy']['id'], 'knowledge_id': manifest['knowledge']['id'], 'ontology_version': manifest['ontology_version'], 'catalog_version': manifest['catalog_version']}

def install_token(cfg, root, pg):
    marker = root / 'api-private' / 'token-installed'
    if marker.exists():
        return
    folder = root / 'gateway'
    folder.mkdir(mode=0o700, exist_ok=True)
    write(folder / 'endpoint.json', {'host': pg, 'port': 5432, 'database': 'oo_maria_conn_token'})
    (folder / 'ca.crt').write_bytes((root / 'private/ca.crt').read_bytes())
    os.chmod(folder / 'ca.crt', 0o600)
    (folder / 'admin.pass').write_bytes((root / 'api-private/token-admin.password').read_bytes())
    os.chmod(folder / 'admin.pass', 0o600)
    run(['docker', 'run', '--rm', '--pull', 'never', '--platform', 'linux/arm64', '--user', '%s:%s' % (os.getuid(), os.getgid()),
         '--network', 'container:' + pg, '--entrypoint', '/fixed/issue-token',
         '--mount', 'type=bind,source=%s,target=/fixed/issue-token,readonly' % ISSUE,
         '--mount', 'type=bind,source=%s,target=/run-current' % root,
         cfg['runtime_image'], 'install', '/run-current/gateway/endpoint.json', '/run-current/gateway/admin.pass', '/run-current/gateway/ca.crt'],
        root / 'api-private' / 'token-install', timeout=120)
    marker.write_text('1\n')
    os.chmod(marker, 0o600)

def gateway(cfg, root, pg):
    ready = root / 'gateway-ready.json'
    name = pg.removesuffix('-pg') + '-gw'
    if ready.exists() and state(name) == 'running':
        return name, json.loads(ready.read_text())['listen']
    pack = json.loads((root / 'product/product-receipt.json').read_text())['pack']
    remote = pathlib.Path(pack['native_maintenance']['artifact_directory'])
    product_cfg = json.loads((root / 'product-config.json').read_text())
    artifact = pathlib.Path(product_cfg['work_dir_host']) / remote.relative_to(product_cfg['work_dir_container'])
    command = json.loads((artifact / 'command.json').read_text())
    reviewed = json.loads((artifact / 'reviewed.json').read_text())
    identity = pack_identity(json.loads((artifact / 'role-pack.json').read_text()))
    vector = command['catalog']['binding_vector']
    if len(vector) != 5:
        raise RuntimeError('gateway vector refused')
    folder = root / 'gateway'
    reviewed_dir = folder / 'reviewed' / 'mariadb_full'
    reviewed_dir.mkdir(parents=True, exist_ok=True)
    os.chmod(folder, 0o700)
    os.chmod(folder / 'reviewed', 0o700)
    (folder / 'packs').mkdir(exist_ok=True)
    (folder / 'wal').mkdir(exist_ok=True)
    (reviewed_dir / 'native-input.json').write_bytes((artifact / 'command.json').read_bytes())
    (reviewed_dir / 'command.json').write_bytes((artifact / 'reviewed.json').read_bytes())
    (folder / 'packs' / 'mariadb_full.json').write_bytes((artifact / 'role-pack.json').read_bytes())
    catalog = command['catalog']
    artifact_sha = hashlib.sha256(json.dumps(catalog, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
    allow = []
    grants = ["DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='oo_r_mariadb_full') THEN CREATE ROLE oo_r_mariadb_full NOLOGIN NOINHERIT; END IF; END $$",
              'GRANT CONNECT ON DATABASE oo_a390_control TO oo_maria_conn_data',
              'GRANT USAGE ON SCHEMA _oo TO oo_maria_conn_data, oo_r_mariadb_full',
              'GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO oo_maria_conn_data, oo_r_mariadb_full',
              'GRANT oo_r_mariadb_full TO oo_maria_conn_data WITH INHERIT FALSE, SET TRUE']
    meta = ['physical_mappings', 'table_freshness', 'source_watermark', 'table_quality', 'apply_receipts', 'full_import_run', 'full_import_table', 'full_import_quality']
    grants.append('GRANT SELECT ON ' + ','.join('_oo.' + item for item in meta) + ' TO oo_maria_conn_data, oo_r_mariadb_full')
    for entry in catalog['entries']:
        table = entry['tables'][0]
        dataset = table['logical_schema'] + '.' + table['logical_table']
        names = []
        for column in table['columns']:
            allow.append({'column': column['logical_name'], 'dataset': dataset, 'explicit_allow': False})
            names.append(column['stored_physical'])
        grants.append('GRANT SELECT (' + ','.join('"' + column + '"' for column in names) + ') ON _oo."' + table['stored_physical'] + '" TO oo_r_mariadb_full')
    if len(allow) != 36:
        raise RuntimeError('gateway column count')
    binding = {'allow': allow, 'artifact_sha256': artifact_sha, 'binding_vector': vector, 'catalog_version': identity['catalog_version'],
               'config_digest': '', 'generation': max(item['source_generation'] for item in vector), 'knowledge_id': identity['knowledge_id'],
               'knowledge_version': identity['knowledge_version'], 'login': 'oo_maria_conn_data', 'ontology_version': identity['ontology_version'],
               'policy_id': identity['policy_id'], 'policy_version': identity['policy_version'], 'project': identity['project'],
               'role': identity['role'].removeprefix('oo_r_'), 'schema': '', 'schema_hash': reviewed['provenance']['collection_sha256'], 'source_id': ''}
    write(reviewed_dir / 'binding.json', binding)
    (folder / 'ca.crt').write_bytes((root / 'private/ca.crt').read_bytes())
    (folder / 'runtime.pass').write_bytes((root / 'api-private/token-runtime.password').read_bytes())
    (folder / 'data.pass').write_bytes((root / 'api-private/data.password').read_bytes())
    for item in ('runtime.pass', 'data.pass', 'ca.crt'):
        os.chmod(folder / item, 0o600)
    write(folder / 'data-endpoint.json', {'host': pg, 'port': 5432, 'database': 'oo_a390_control'})
    (folder / 'data-ca.crt').write_bytes((folder / 'ca.crt').read_bytes())
    os.chmod(folder / 'data-ca.crt', 0o600)
    write(folder / 'config.json', {'listen': '127.0.0.1:18570', 'audience': 'https://127.0.0.1:9/', 'runtime_user': 'oo_token_runtime',
                                   'password_file': 'runtime.pass', 'ca_file': 'ca.crt', 'endpoint_file': 'endpoint.json', 'pack_dir': 'packs',
                                   'body_limit': 1048576, 'request_timeout_ms': 15000, 'global_concurrency': 4,
                                   'data_endpoint_file': 'data-endpoint.json', 'data_password_file': 'data.pass', 'data_ca_file': 'data-ca.crt',
                                   'data_user': 'oo_maria_conn_data', 'wal_dir': 'wal', 'reviewed_dir': 'reviewed'})
    write(folder / 'grant.json', {'kind': 'client', 'project_id': identity['project'], 'principal_id': 'fixture-owner', 'role': identity['role'],
                                  'policy_version': identity['policy_version'], 'knowledge_version': identity['knowledge_version'],
                                  'audience_url': 'https://127.0.0.1:9/', 'claims': [], 'client_ttl_ms': 3600000})
    psql(pg, ';\n'.join(grants) + ';', 'oo_a390_control')
    install_token(cfg, root, pg)
    if state(name) == 'absent':
        run(['docker', 'run', '-d', '--name', name, '--pull', 'never', '--platform', 'linux/arm64', '--user', '%s:%s' % (os.getuid(), os.getgid()),
             '--network', 'container:' + pg, '--label', 'oo.owner=mariadb-connection-whole', '--label', 'oo.kind=owned-gateway',
             '--mount', 'type=bind,source=%s,target=/run-current' % root,
             '--mount', 'type=bind,source=%s,target=/fixed/gateway,readonly' % GW_ELF,
             '--mount', 'type=bind,source=%s,target=/fixed/oo,readonly' % CLI_ELF,
             '--mount', 'type=bind,source=%s,target=/fixed/issue-token,readonly' % ISSUE,
             '--entrypoint', '/fixed/gateway', cfg['runtime_image'], '/run-current/gateway'], root / 'api-private' / 'gateway-run', timeout=60)
    elif state(name) != 'running':
        run(['docker', 'start', name], root / 'api-private' / 'gateway-start', timeout=60)
    for _ in range(30):
        if (folder / 'listen.addr').exists() and (folder / 'listen.addr').stat().st_size:
            break
        time.sleep(1)
    else:
        raise RuntimeError('gateway did not listen')
    token = folder / 'client.token'
    if not token.exists():
        run(['docker', 'exec', '-w', '/run-current/gateway', name, '/fixed/issue-token', 'issue', 'endpoint.json', 'admin.pass', 'ca.crt', 'grant.json', 'client.token'], root / 'api-private' / 'issue', timeout=60)
    os.chmod(token, 0o600)
    listen = (folder / 'listen.addr').read_text().strip()
    return name, listen

def cli_wrapper(root, gateway_name):
    path = root / 'oo-cli'
    path.write_text('#!/usr/bin/env python3\nimport os, pathlib, sys\nargs=sys.argv[1:]\ni=args.index("--token-file")\ntoken=pathlib.Path(args[i+1])\nif not token.is_absolute(): token=pathlib.Path.cwd()/token\ntext=str(token)\nif text.startswith("/private/tmp/"):\n    text="/tmp/"+text[len("/private/tmp/"):]\n    token=pathlib.Path(text)\nroot=pathlib.Path(%r)\nif token != root and root not in token.parents: raise SystemExit("unexpected token path")\nargs[i+1]="/run-current/"+str(token.relative_to(root))\nos.execvp("docker", ["docker", "exec", %r, "/fixed/oo", *args])\n' % (str(root), gateway_name))
    os.chmod(path, 0o700)
    return path

def cell(value):
    if isinstance(value, dict) and 'value' in value:
        value = value['value']
    return str(value)

def source_query(cfg, sql):
    user = pathlib.Path(cfg['reader_user_file']).read_text().strip()
    password = pathlib.Path(cfg['password_file']).read_text().strip()
    defaults = '[client]\nuser=%s\npassword=%s\n' % (user, password)
    proc = subprocess.run(['docker', 'exec', '-i', cfg['fixture_container'], 'mariadb', '--defaults-extra-file=/dev/stdin', '-N', '-e', sql], input=defaults.encode(), capture_output=True, timeout=40)
    if proc.returncode:
        raise RuntimeError('source read refused')
    return proc.stdout.decode().strip()

def gold(cfg, root, listen):
    token = str(root / 'gateway/client.token')
    common = ['--json', '--gateway', 'http://' + listen, '--transport', 'local-development', '--auth', 'external', '--token-file', token]
    expected = {}
    for table in ('logistics.hubs', 'logistics.shipments', 'logistics.tracking_events', 'cust_east.k_note', 'cust_west.k_meter'):
        expected[table] = source_query(cfg, 'SELECT COUNT(*) FROM %s' % table)
    expected['hub_code'] = source_query(cfg, 'SELECT hub_code FROM logistics.hubs WHERE hub_id=8')
    expected['reading'] = source_query(cfg, 'SELECT reading FROM cust_west.k_meter WHERE id=18446744073709551615')
    queries = [(table, 'SELECT COUNT(*) AS n FROM %s' % table, expected[table]) for table in expected if '.' in table]
    queries.append(('hub_code', 'SELECT hub_code FROM logistics.hubs WHERE hub_id = 8', expected['hub_code']))
    queries.append(('reading', 'SELECT reading FROM cust_west.k_meter WHERE id = 18446744073709551615', expected['reading']))
    compared = []
    for name, sql, want in queries:
        proc = subprocess.run([str(root / 'oo-cli'), 'query', sql, '--max-rows', '5', '--timeout-ms', '10000', *common], capture_output=True, text=True, timeout=40)
        kept(proc, root / 'gold' / name)
        if proc.returncode:
            raise RuntimeError('gateway gold refused: ' + name)
        body = json.loads(proc.stdout)
        got = cell(body['rows'][0][0])
        if got != want:
            raise RuntimeError('gateway gold differs: ' + name)
        compared.append({'name': name, 'value': got, 'query_id': body['query_id']})
    write(root / 'gateway-ready.json', {'listen': listen, 'typed_cell_diff': 0, 'compared': compared})
    return expected

def dialogue(cfg, root, listen, expected):
    verdict = root / 'dialogue' / 'verdict.json'
    if verdict.exists():
        return
    if (root / 'dialogue').exists():
        shutil.rmtree(root / 'dialogue')
    write(root / 'dialogue-truth.json', {'hub': {'expected_named_facts': {'hub_code': expected['hub_code']}}, 'meter': {'expected_named_facts': {'reading': expected['reading']}}})
    write(root / 'dialogue-config.json', {
        'oo_cli': str(root / 'oo-cli'), 'gateway': 'http://' + listen, 'transport': 'local-development', 'auth': 'external',
        'gateway_token_file': str(root / 'gateway/client.token'), 'model_origin': 'http://127.0.0.1:8317', 'model': 'grok-4.7',
        'model_bearer_file': str(BEARER), 'independent_truth_file': str(root / 'dialogue-truth.json'),
        'independent_gateway_audit_file': str(root / 'gateway/wal'), 'max_tool_rounds': 12,
        'questions': [
            {'truth_key': 'hub', 'prompt': 'Call knowledge_search with nonempty query hubs, then knowledge_cat on the returned installed Pack path, describe logistics.hubs, and freshness. Then query SELECT hub_code FROM logistics.hubs WHERE hub_id = 8.'},
            {'truth_key': 'meter', 'prompt': 'Query SELECT reading FROM cust_west.k_meter WHERE id = 18446744073709551615. Return the observed reading.'},
        ],
    })
    proc = subprocess.run([sys.executable, str(DIALOGUE), '--config', str(root / 'dialogue-config.json'), '--out', str(root / 'dialogue')], capture_output=True, timeout=900)
    kept(proc, root / 'dialogue-call')
    if proc.returncode:
        raise RuntimeError('dialogue failed')

def cancel_query(root, pg, listen):
    receipt = root / 'cancel' / 'receipt.json'
    if receipt.exists():
        return
    command = json.loads((root / 'gateway/reviewed/mariadb_full/native-input.json').read_text())
    physical = next(item['stored_physical'] for item in command['catalog']['binding_vector'] if item['dataset'] == 'logistics.hubs')
    out = root / 'cancel'
    out.mkdir(mode=0o700, exist_ok=True)
    lock = subprocess.Popen(['docker', 'exec', pg, 'psql', '-U', 'postgres', '-d', 'oo_a390_control', '-c', 'BEGIN; LOCK TABLE _oo."%s" IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(20); ROLLBACK;' % physical], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    common = ['--json', '--gateway', 'http://' + listen, '--transport', 'local-development', '--auth', 'external', '--token-file', str(root / 'gateway/client.token')]
    wal = root / 'gateway' / 'wal'
    old = set(wal.glob('*-admit.wal'))
    query = subprocess.Popen([str(root / 'oo-cli'), 'query', 'SELECT hub_code FROM logistics.hubs WHERE hub_id = 8', '--max-rows', '5', '--timeout-ms', '15000', *common], stdout=(out / 'query.json').open('w'), stderr=(out / 'query.err').open('w'))
    qid = None
    try:
        time.sleep(1)
        for _ in range(40):
            new = set(wal.glob('*-admit.wal')) - old
            if new:
                qid = new.pop().name.removesuffix('-admit.wal')
                break
            time.sleep(0.25)
        if not qid:
            raise RuntimeError('query was not admitted')
        cancelled = subprocess.run([str(root / 'oo-cli'), 'cancel', qid, *common], capture_output=True, text=True, timeout=20)
        (out / 'cancel.json').write_text(cancelled.stdout)
        (out / 'cancel.err').write_text(cancelled.stderr)
        query.wait(timeout=20)
        response = json.loads(cancelled.stdout or '{}')
        result = json.loads((out / 'query.json').read_text() or '{}')
        write(receipt, {'query_id': qid, 'cancel': response, 'hint': result.get('hint'), 'code': result.get('code')})
        if response.get('outcome') != 'sent' or result.get('hint') != 'sqlstate 57014':
            raise RuntimeError('cancel was not a durable 57014')
    finally:
        if query.poll() is None:
            query.kill()
        lock.kill()
        lock.wait(timeout=10)

def main():
    parser = __import__('argparse').ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    root = pathlib.Path(args.run)
    whole = root / 'whole.json'
    if whole.exists():
        print(whole.read_text())
        return
    os.umask(0o077)
    try:
        status = identity_status()
    except OSError:
        print('identity_not_serving', file=sys.stderr)
        raise SystemExit(2)
    if '401' not in status:
        print('identity_status=' + status, file=sys.stderr)
        raise SystemExit(2)
    cfg = json.loads(pathlib.Path(args.config).read_text())
    prep = json.loads((root / 'preparation.json').read_text())
    pg = prep['pg']
    bundle = root / 'native-collection' / 'bundle'
    columns = json.loads((bundle / 'roles.json').read_text())['roles']['oo_r_mariadb_full']
    if len(columns) != 36:
        raise SystemExit('role column count')
    ensure_api(cfg, root, pg, prep['runtime'], bundle, columns)
    product_config(cfg, root, bundle, columns)
    run_product(root)
    _name, listen = gateway(cfg, root, pg)
    cli_wrapper(root, pg.removesuffix('-pg') + '-gw')
    expected = gold(cfg, root, listen)
    dialogue(cfg, root, listen, expected)
    cancel_query(root, pg, listen)
    product_body = json.loads((root / 'product/product-receipt.json').read_text())
    dialogue_body = json.loads((root / 'dialogue/verdict.json').read_text())
    cycle = json.loads((root / 'cycle-receipt.json').read_text())
    assert product_body.get('protocol') == 'oo.source-onboarding.product/v1' and product_body.get('fixture_review') is True
    assert dialogue_body.get('outcome') == 'verified' and dialogue_body.get('turns', 0) >= 2
    assert cycle.get('insert_update_delete_verified') is True and cycle.get('recovery_verified') is True and cycle.get('typed_cell_diff') == 0
    assert json.loads((root / 'gateway-ready.json').read_text())['typed_cell_diff'] == 0
    body = {'full_product_chain_complete': True, 'periodic_insert_update_delete_verified': True, 'recovery_verified': True, 'product_receipt': 'product/product-receipt.json', 'dialogue_verdict': 'dialogue/verdict.json'}
    write(whole, body)
    print(json.dumps(body))

if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print('mariadb-product: ' + str(exc), file=sys.stderr)
        raise SystemExit(1)
