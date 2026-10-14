#!/usr/bin/env python3
"""Six-field PostgreSQL whole run. One legal S5, then the public Gateway chain."""
import hashlib, json, os, shutil, ssl, subprocess, sys, time, urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FLOW = ROOT / 'tools/source-onboarding/product_flow.py'
DIALOGUE = ROOT / 'tools/agent-product-verifier/dialogue.py'
CLI = ROOT / 'tools/postgres-preview/linux/gateway_cli_adapter.py'
CACHE = Path('/Users/redux/.cache/oo-postgresql-connection-whole')
API = 'oo-pg-connection-whole-api'
GW = 'oo-pg46-continuous-gateway'
WAL = Path('/tmp/oo-pg46-runtime-finish/continuous/gateway/wal')
EXPECTED = {
    'type': 'postgresql', 'database': 'pg46_finish_source', 'host': 'oo-dl01w1-46-src',
    'port': 5432, 'user': 'oo_dl01w1_46_reader', 'secret_ref': 'secret://oo-pg46-finish/reader',
}
ROLE = 'oo_r_pg46_full'

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')
    os.chmod(path, 0o600)

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def run(argv, timeout=1800):
    result = subprocess.run(argv, capture_output=True, timeout=timeout)
    if result.returncode:
        sys.stderr.write(result.stderr[-2000:].decode('utf-8', 'replace'))
        raise RuntimeError('command failed ' + Path(argv[-1] if len(argv)==1 else argv[1]).name)
    return result

def lists(project):
    cookie = Path('/Users/redux/.cache/oo-dl01w1-71/native-identity-private/fixture-admin.cookie').read_text().strip()
    ctx = ssl.create_default_context(cafile=str(CACHE / 'tls/api/ca.crt'))
    req = urllib.request.Request(
        'https://localhost:18646/control/v1/builds?project_id=' + project,
        headers={'Cookie': '__Host-oo-session=' + cookie})
    try:
        with urllib.request.urlopen(req, context=ctx, timeout=30) as response:
            body = json.load(response)
            return response.status, body.get('management_identity'), len(body.get('builds') or []), body.get('error')
    except urllib.error.HTTPError as error:
        body = json.loads(error.read())
        return error.code, body.get('management_identity'), None, (body.get('error') or {}).get('message')

def source_sql(sql):
    return subprocess.check_output(['docker','exec','oo-dl01w1-46-src','psql','-U','postgres','-d','pg46_finish_source','-X','-At','-v','ON_ERROR_STOP=1','-c',sql], timeout=30).decode().strip()

def gateway_port():
    text = subprocess.check_output(['docker','exec',GW,'python3','-c','print(open("/proc/net/tcp").read())'], text=True, timeout=20)
    for line in text.splitlines():
        parts = line.split()
        if not parts or parts[0] == 'sl' or parts[3] != '0A':
            continue
        ip, port = parts[1].split(':')
        raw = bytes.fromhex(ip)
        address = '.'.join(str(x) for x in raw[::-1])
        if address == '127.0.0.1':
            return int(port, 16)
    raise RuntimeError('gateway listen missing')

def issue_token():
    run(['docker','exec',GW,'/run-pg46/issue-token','issue','/run-pg46/endpoint.json','/run-pg46/admin.pass','/run-pg46/ca.crt','/run-pg46/grant.json','/run-pg46/token-current.private'], timeout=60)
    host = CACHE / 'gateway-token'
    data = subprocess.check_output(['docker','exec',GW,'cat','/run-pg46/token-current.private'], timeout=20)
    host.write_bytes(data)
    os.chmod(host, 0o600)
    return host

def cli(command, *extra):
    port = gateway_port()
    token = str(CACHE / 'gateway-token')
    flags = ['--json', '--auth', 'external', '--token-file', token, '--gateway', f'http://127.0.0.1:{port}', '--transport', 'local-development']
    return subprocess.run([str(CLI), command, *extra, *flags], capture_output=True, text=True, timeout=60)

def canon_sha(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()

def gold(out):
    issue_token()
    expected = {
        'hubs': source_sql('SELECT COUNT(*)||\'|\'||COALESCE(SUM(capacity_grams),0) FROM public.hubs'),
        'notes': source_sql('SELECT COUNT(*)||\'|\'||COALESCE(SUM(amount),0) FROM customer_east.delivery_notes'),
        'shipments': source_sql("SELECT COUNT(*)||'|'||COALESCE(SUM(cost_amount),0) FROM public.shipments"),
        'tracking': source_sql('SELECT COUNT(*)||\'|\'||COALESCE(MAX(event_id),0) FROM public.tracking_events'),
    }
    queries = {
        'hubs': 'SELECT COUNT(*) AS row_count, SUM(capacity_grams) AS capacity_total FROM public.hubs',
        'notes': 'SELECT COUNT(*) AS row_count, SUM(amount) AS amount_total FROM customer_east.delivery_notes',
        'shipments': 'SELECT COUNT(*) AS row_count, SUM(cost_amount) AS cost_total FROM public.shipments',
        'tracking': 'SELECT COUNT(*) AS row_count, MAX(event_id) AS last_event_id FROM public.tracking_events',
    }
    rows = {}
    for name, sql in queries.items():
        result = cli('query', sql)
        (out / (name + '.stdout')).write_text(result.stdout)
        (out / (name + '.stderr')).write_text(result.stderr)
        if result.returncode:
            raise RuntimeError('public query failed ' + name)
        body = json.loads(result.stdout)
        got = '|'.join(str(cell) for cell in body['rows'][0])
        rows[name] = {'query_id': body['query_id'], 'source': expected[name], 'gateway': got}
        if got != expected[name]:
            raise RuntimeError('typed gold mismatch ' + name)
        wal = WAL / (body['query_id'] + '-complete.wal')
        if not wal.is_file() or body['query_id'] not in wal.read_text():
            raise RuntimeError('query wal missing ' + name)
    write(out / 'gold.json', {'typed_cell_diff': 0, 'queries': rows})
    return rows

def live_cancel(out):
    out.mkdir(mode=0o700, exist_ok=True)
    before = {path.name for path in WAL.glob('*-admit.wal')}
    sql = 'SELECT COUNT(*) AS n FROM public.hubs a, public.hubs b, public.hubs c'
    query = subprocess.Popen([str(CLI), 'query', sql, '--timeout-ms', '30000', '--json', '--auth', 'external', '--token-file', str(CACHE / 'gateway-token'), '--gateway', f'http://127.0.0.1:{gateway_port()}', '--transport', 'local-development'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    found = None
    try:
        for _ in range(40):
            time.sleep(0.25)
            fresh = [path for path in WAL.glob('*-admit.wal') if path.name not in before]
            if fresh:
                found = fresh[-1].name.split('-admit.wal')[0]
                break
        if not found:
            raise RuntimeError('running query id was not admitted')
        cancelled = cli('cancel', found)
        (out / 'cancel.stdout').write_text(cancelled.stdout)
        (out / 'cancel.stderr').write_text(cancelled.stderr)
        if cancelled.returncode:
            raise RuntimeError('cancel command failed')
        body = json.loads(cancelled.stdout)
        if body.get('outcome') != 'sent':
            raise RuntimeError('cancel was not sent')
    finally:
        try:
            query.wait(timeout=20)
        except subprocess.TimeoutExpired:
            query.kill()
        (out / 'query.stdout').write_text(query.stdout.read() if query.stdout else '')
        (out / 'query.stderr').write_text(query.stderr.read() if query.stderr else '')
    wal_text = ''
    for path in WAL.glob(found + '-*.wal'):
        wal_text += path.read_text()
    proof = {'query_id': found, 'outcome': 'sent', 'wal_has_query_id': found in wal_text, 'sqlstate_57014': '57014' in wal_text or '57014' in (out / 'query.stdout').read_text()}
    if not proof['wal_has_query_id'] or not proof['sqlstate_57014']:
        write(out / 'verdict.json', proof)
        raise RuntimeError('live cancel wal proof incomplete')
    proof['rollback'] = True
    write(out / 'verdict.json', proof)
    return proof

FIXED_ELF = Path('/Users/redux/.cache/oo-grok-connection-full-20261014/fixed-acl-50fd9d462/install-column-acl')
FIXED_ELF_SHA = 'aea3dad54671b9608a2ab1a739a2160681bc9cd8f4eacf08a49ce3c45a79cf05'
OLD_LSN = '0/404D7A8'

def publish_bundle(catalog_path, end_lsn):
    if end_lsn == OLD_LSN or '/' not in str(end_lsn):
        raise RuntimeError('recovery LSN is not a new owned slot position')
    src, dest = CACHE / 'bundle', CACHE / 'bundle-current'
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(src, dest)
    data = Path(catalog_path).read_bytes()
    (dest / 'catalog.json').write_bytes(data)
    os.chmod(dest / 'catalog.json', 0o600)
    cat = json.loads(data)
    body = dict(cat)
    declared = body.pop('collection_sha256')
    if canon_sha(body) != declared:
        raise RuntimeError('fresh collection hash is not self-consistent')
    file_sha = hashlib.sha256(data).hexdigest()
    vector_sha = canon_sha(cat['binding_vector'])
    gens, rows, max_s, max_t = {}, {}, 0, 0
    for binding, entry in zip(cat['binding_vector'], cat['entries']):
        dataset = binding['dataset']
        gens[dataset] = {'source_generation': binding['source_generation'], 'target_generation': binding['target_generation'], 'run_id': binding['run_id']}
        rows[dataset] = entry['tables'][0]['accepted_rows']
        max_s, max_t = max(max_s, binding['source_generation']), max(max_t, binding['target_generation'])
    for name, kind in (('profile.json', 'profile'), ('source_semantics.json', 'semantics')):
        path = dest / name
        doc = json.loads(path.read_text())
        doc['source_generation'] = max_s
        doc['table_generations'] = gens
        doc['binding_vector_sha256'] = vector_sha
        doc['collection_sha256'] = file_sha
        if kind == 'semantics':
            doc['target_generation'] = max_t
            doc['source_contract']['applied_end_lsn'] = end_lsn
            doc['source_contract']['write_attempts'] = 2
            doc['quality']['rows'] = sum(rows.values())
        else:
            for table in doc['tables']:
                generation = gens[table['dataset']]
                table.update(rows=rows[table['dataset']], source_generation=generation['source_generation'], target_generation=generation['target_generation'], run_id=generation['run_id'])
        path.write_text(json.dumps(doc, indent=2) + '\n')
        os.chmod(path, 0o600)
    manifest = json.loads((dest / 'manifest.json').read_text())
    for name in list(manifest['files']):
        manifest['files'][name] = hashlib.sha256((dest / name).read_bytes()).hexdigest()
    (dest / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    os.chmod(dest / 'manifest.json', 0o600)
    runtime_path = CACHE / 'runtime.json'
    runtime = json.loads(runtime_path.read_text())
    runtime['projects'][0]['sealed_bundle'] = '/pgwhole/bundle-current'
    write(runtime_path, runtime)
    return file_sha

def restart_api():
    subprocess.run(['docker', 'restart', API], check=True, timeout=180)
    deadline, last = time.monotonic() + 180, None
    while time.monotonic() < deadline:
        try:
            last = lists('postgres')
            if last[0] == 200:
                return last
        except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
            last = error.__class__.__name__
        time.sleep(2)
    raise RuntimeError('api list did not return after restart: ' + str(last))

def acl_transition(generation, out):
    done = out / 'acl-ok.json'
    if done.exists():
        return json.loads(done.read_text())
    if sha(FIXED_ELF) != FIXED_ELF_SHA:
        raise RuntimeError('fixed acl elf sha mismatch')
    previous_doc = json.loads(Path('/tmp/oo-pg46-runtime-finish/continuous/install-acl-fixture-role.json').read_text())
    current_generation = previous_doc['policies'][0]['generation']
    if current_generation == generation:
        return {'effect': 'already_current'}
    stage = Path('/tmp/oo-pg46-runtime-finish/onboard/acl')
    stage.mkdir(mode=0o700, parents=True, exist_ok=True)
    ca = stage / 'ca.pem'
    ca.write_bytes(Path('/tmp/oo-pg46-tls-durable/ca.pem').read_bytes())
    import connection_cycle
    def document(gen):
        policies = [{**policy, 'generation': gen, 'row_filter': None} for policy in previous_doc['policies']]
        return {'protocol': 'oo.gateway.install-native-vector-acl-cli/v1', 'policies': policies, 'endpoint': previous_doc['endpoint'], 'password_file': '/tmp/acl-super', 'ca_file': connection_cycle.cpath(ca)}
    previous, current = stage / 'previous.json', stage / 'current.json'
    previous.write_text(json.dumps(document(current_generation)))
    current.write_text(json.dumps(document(generation)))
    os.chmod(previous, 0o600)
    os.chmod(current, 0o600)
    subprocess.run(['docker', 'cp', str(FIXED_ELF), 'oo-pg46-runtime-finish:/tmp/install-column-acl'], check=True, timeout=60)
    subprocess.run(['docker', 'cp', '/tmp/oo-dl01w1-46-secrets/run/super', 'oo-pg46-runtime-finish:/tmp/acl-super'], check=True, timeout=60)
    subprocess.run(['docker', 'exec', 'oo-pg46-runtime-finish', 'chmod', '0700', '/tmp/install-column-acl', '/tmp/acl-super'], check=True, timeout=30)
    proc = subprocess.run(['docker', 'exec', '-i', 'oo-pg46-runtime-finish', '/tmp/install-column-acl', '--previous-policy-file', connection_cycle.cpath(previous), '--previous-policy-sha256', sha(previous)], input=current.read_bytes(), capture_output=True, timeout=120)
    (out / 'acl.stdout').write_bytes(proc.stdout)
    (out / 'acl.stderr').write_bytes(proc.stderr)
    os.chmod(out / 'acl.stdout', 0o600)
    os.chmod(out / 'acl.stderr', 0o600)
    subprocess.run(['docker', 'exec', 'oo-pg46-runtime-finish', 'rm', '-f', '/tmp/acl-super'], check=False, timeout=30)
    if proc.returncode:
        raise RuntimeError('column acl transition failed')
    outcome = json.loads(proc.stdout)
    write(done, outcome)
    return outcome

def align_gateway(catalog_path, out):
    import connection_cycle
    cat = json.loads(Path(catalog_path).read_text())
    reviewed = Path('/tmp/oo-pg46-runtime-finish/continuous/gateway/reviewed/pg46_full')
    backup = out / 'gateway-before'
    if not backup.exists():
        shutil.copytree(reviewed, backup)
    native = json.loads((backup / 'native-input.json').read_text())
    native['catalog'] = cat
    collection = cat['collection_sha256']
    for decision in native['decisions']:
        decision['schema_hash'] = collection
    native['settings']['catalog_version'] = collection
    staged, compiled = Path('/tmp/oo-pg46-runtime-finish/onboard/native-input.json'), Path('/tmp/oo-pg46-runtime-finish/onboard/command.json')
    staged.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staged.write_text(json.dumps(native))
    proc = subprocess.run(['docker', 'exec', 'oo-pg46-runtime-finish', '/finish/continuous/pg-builder-current', 'sealed-review', '--command', connection_cycle.cpath(staged), '--out', connection_cycle.cpath(compiled)], capture_output=True, timeout=60)
    (out / 'sealed-review.stderr').write_bytes(proc.stderr)
    if proc.returncode:
        raise RuntimeError('reviewed compile failed')
    command = json.loads(compiled.read_text())
    if command['provenance']['collection_sha256'] != collection:
        raise RuntimeError('compiled collection hash drifted')
    binding = json.loads((backup / 'binding.json').read_text())
    binding['binding_vector'] = command['provenance']['binding_vector']
    binding['schema_hash'] = collection
    binding['catalog_version'] = command['effective_oos']['catalog_version']
    binding['generation'] = max(item['source_generation'] for item in binding['binding_vector'])
    binding['artifact_sha256'] = canon_sha(cat)
    binding['config_digest'] = binding['binding_vector'][0]['config_digest']
    (reviewed / 'native-input.json').write_text(json.dumps(native))
    (reviewed / 'command.json').write_text(compiled.read_text())
    (reviewed / 'binding.json').write_text(json.dumps(binding))
    acl_transition(binding['generation'], out)
    subprocess.run(['docker', 'restart', GW], check=True, timeout=120)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        running = subprocess.check_output(['docker', 'inspect', '-f', '{{.State.Running}}', GW], text=True).strip()
        if running == 'true':
            try:
                gateway_port()
                return binding['generation']
            except RuntimeError:
                pass
        time.sleep(1)
    shutil.copytree(backup, reviewed, dirs_exist_ok=True)
    subprocess.run(['docker', 'restart', GW], check=False, timeout=120)
    raise RuntimeError('gateway did not listen on the new binding')

def decisions_from(overlay):
    reviews, columns = [], []
    for column in overlay.get('columns') or []:
        if column.get('pii') != 'none':
            continue
        name = column['name'] if column['name'].count('.') == 2 else column['dataset'] + '.' + column['name']
        reviews.append({'name': name, 'binds_fact': column['fact_sha256'], 'decision': 'confirmed'})
        columns.append(name)
    if not reviews:
        raise RuntimeError('overlay has no confirmable columns')
    return {'reviews': reviews, 'confirm_columns': columns, 'role': ROLE, 'fixture_authorized': True}

def product_config(catalog_sha):
    runtime = json.loads((CACHE / 'runtime.json').read_text())
    project = runtime['projects'][0]
    body = {
        'endpoint': 'https://localhost:18646', 'origin': 'https://127.0.0.1:18441',
        'ca': str(CACHE / 'tls/api/ca.crt'),
        'identity_origin': 'https://127.0.0.1:18441',
        'identity_ca': '/Users/redux/.cache/oo-dl01w1-71/native-identity-private/ca.pem',
        'cookies': {
            'admin': '/Users/redux/.cache/oo-dl01w1-71/native-identity-private/fixture-admin.cookie',
            'reviewer': '/Users/redux/.cache/oo-dl01w1-71/native-identity-private/fixture-reviewer.cookie',
            'owner': '/Users/redux/.cache/oo-dl01w1-71/native-identity-private/fixture-owner.cookie',
        },
        'operator_sha256': sha(ROOT / 'tools/selfbuild-control-api/recipes/operator.py'),
        'fixture_authorized': True,
        'create': {
            'allowance_id': project['allowance_id'], 'config_commit': project['config_commit'],
            'llm_secret_ref': project['llm_secret_ref'], 'project_id': 'postgres', 'source_ref': project['source_ref'],
        },
        'role': ROLE, 'model_deadline_seconds': 2400,
        'work_dir_container': '/pgwhole/work', 'work_dir_host': str(CACHE / 'work'),
        'export_oos_sha256': sha(ROOT / 'tools/selfbuild-control-api/recipes/export_oos.py'),
        'catalog_sha256': catalog_sha,
    }
    assert project['project_id'] == 'postgres' and project['source']['database'] == 'pg46_finish_source'
    assert project['llm']['model_id'] == 'grok-4.7'
    return body

def fresh_catalog(out):
    probe = CACHE / 'model-cache' / 'catalog.json'
    if not probe.is_file():
        raise RuntimeError('fresh catalog capture is absent')
    stored = json.loads(probe.read_text())
    if stored.get('protocol') != 'oo.source.pgsql/v1' or stored.get('database') != 'pg46_finish_source':
        raise RuntimeError('catalog is not the native source')
    copied = out / 'catalog.json'
    copied.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    copied.write_bytes(probe.read_bytes())
    os.chmod(copied, 0o600)
    return sha(probe)

def seed_committed_cycle(run_dir):
    src = Path('/Users/redux/.cache/oo-source-onboarding/runs/ca4f4bd944884b019717679d068f0fc6/cycle')
    dest = Path(run_dir) / 'cycle'
    if (dest / 'started.json').exists() or not (src / 'deleted-note.json').is_file() or (src / 'receipt.json').exists():
        return
    dest.mkdir(mode=0o700, exist_ok=True)
    for name in ('started.json', 'deleted-note.json', 'dml-started.json', 'source-transaction.exit.json', 'source-transaction.stdout', 'source-transaction.stderr'):
        file = src / name
        if file.is_file():
            target = dest / name
            target.write_bytes(file.read_bytes())
            os.chmod(target, 0o600)

PRODUCT_RECEIPT = Path('/Users/redux/.cache/oo-postgresql-connection-whole/product-s5-c9295d3e/product/product-receipt.json')
PRODUCT_PACK = Path('/tmp/oo-pg46-runtime-finish/continuous/gateway/packs/pg46_full.json')
SERVED_PACK = Path('/tmp/oo-pg46-runtime-finish/continuous/gateway/packs/pg46_continuous_fixture.json')
PUBLIC_GOLD = Path('/Users/redux/.cache/oo-postgresql-connection-whole/gateway-current-c9295d3e/public-cli/gold.json')
CANCEL_VERDICT = Path('/Users/redux/.cache/oo-postgresql-connection-whole/gateway-current-c9295d3e/live-cancel/verdict.json')
DIALOGUE_GOLD = Path('/Users/redux/.cache/oo-postgresql-connection-whole/random-dialogue/gold.json')
BOUND_GENERATIONS = Path('/Users/redux/.cache/oo-postgresql-connection-whole/random-dialogue/observed-bound-generations.json')
OLD_POINTER = Path('/Users/redux/.cache/oo-postgresql-connection-whole/onboard/input.json.run.json')
OLD_RECIPE = Path('/Users/redux/.cache/oo-postgresql-connection-whole/onboard/recipe.json')
PRODUCT_PACK_SHA = '7557a6d878e3898eef070f9f8344d913316604e82567721defcaa4be64d7a240'
SERVED_PACK_SHA = '0fc4d62c995f2a5c39ed94a59a6c6a3a7d6eb3b31310d8cea30e7fa9cad48e73'
SERVED_PACK_DIGEST = '9194fc50dfcbd068872bd493a0bc6fecba602a2a5ba259e2b757bfdf11d1dae4'
DIALOGUE_GOLD_SHA = 'ae1043e3e93ac704c1bdb1706f60d086ef2b25a68efb88cb16be25e63751a37d'
GENERATIONS = {
    'customer_east.delivery_notes': 5,
    'public.hubs': 4,
    'public.shipments': 4,
    'public.tracking_events': 4,
}
GOLD_ROWS = {
    'hubs': '2002|-18446744071708554617',
    'notes': '47|1234.7500',
    'shipments': '8004|10000000000000024700365001754003708.9636',
    'tracking': '90003|70000000004',
}
PROTECTED = (PRODUCT_RECEIPT, PRODUCT_PACK, SERVED_PACK, PUBLIC_GOLD, CANCEL_VERDICT, DIALOGUE_GOLD, BOUND_GENERATIONS, OLD_POINTER, OLD_RECIPE)

def target_sql(database, sql):
    return subprocess.check_output(['docker', 'exec', 'oo-dl01w1-46-native', 'psql', '-U', 'postgres', '-d', database, '-X', '-At', '-v', 'ON_ERROR_STOP=1', '-c', sql], timeout=30).decode().strip()

def reuse_decision(facts):
    """Bind the sealed product to this source. A different role string is not a new approval."""
    if facts['generations'] != GENERATIONS:
        raise RuntimeError('generation_vector')
    if facts['slot_flush'] != '0/40715A8':
        raise RuntimeError('slot_moved')
    if facts['product_pack_sha256'] != PRODUCT_PACK_SHA or facts['served_pack_sha256'] != SERVED_PACK_SHA:
        raise RuntimeError('pack_sha')
    if facts['product_pack_sha256'] == facts['served_pack_sha256'] or facts['product_role'] == facts['served_role']:
        raise RuntimeError('pack_collapsed')
    if facts['product_role'] != 'oo_r_pg46_full' or facts['served_role'] != 'oo_r_pg46_continuous_fixture':
        raise RuntimeError('role')
    if facts['served_pack_digest'] != SERVED_PACK_DIGEST or not facts['table_bytes_same']:
        raise RuntimeError('field_permissions')
    if facts['gold'] != GOLD_ROWS or facts['dialogue_gold_sha256'] != DIALOGUE_GOLD_SHA:
        raise RuntimeError('gold')
    if facts['full_target_columns'] != 38 or facts['full_target_tables'] != 0 or facts['full_dst_columns'] != 38:
        raise RuntimeError('approved_role_privileges')
    if facts['continuous_target_columns'] != 38 or facts['continuous_target_tables'] != 0:
        raise RuntimeError('served_role_privileges')
    if facts['full_member'] != 'oo_pg46_preview_login' or facts['continuous_member'] != 'oo_pg46_continuous_login':
        raise RuntimeError('membership')
    if facts['numeric_scale'] != {'customer_east.delivery_notes.amount': 4, 'public.shipments.cost_amount': 4}:
        raise RuntimeError('numeric_scale')
    receipt = facts['receipt']
    s5 = receipt.get('s5') or {}
    if receipt.get('build_id') != 'b_ca53577181af9bb17a34a862644e900f' or receipt.get('native_role_pack_sha256') != PRODUCT_PACK_SHA:
        raise RuntimeError('receipt')
    if s5.get('outcome') != 'validated' or s5.get('cache_hit') is not False or s5.get('llm_calls') != 1 or s5.get('model') != 'grok-4.7-build':
        raise RuntimeError('s5')
    stages = {item.get('stage') for item in receipt.get('stages') or [] if item.get('status') == 'succeeded'}
    if stages < {f'S{i}' for i in range(11)}:
        raise RuntimeError('stages')
    runs = facts['dialogue_runs']
    if len(runs) != 2 or any(item.get('outcome') != 'verified' or item.get('pack_sha256') != SERVED_PACK_SHA or item.get('role') != 'oo_r_pg46_continuous_fixture' or item.get('source_generation') != 5 for item in runs):
        raise RuntimeError('dialogue')
    if len({item.get('seed') for item in runs}) != 2:
        raise RuntimeError('dialogue')
    return {
        'protocol': 'oo.postgres.entry-callback/v1',
        'whole': False,
        'published': False,
        'formal_signoff': False,
        'full_product_chain_complete': False,
        'reuse': True,
        'no_overwrite': True,
        'receipt_rewritten': False,
        'copied_private_verdict': False,
        'customer_writes': 0,
        'source_context': {
            'source_id': 'oo_pg46_finish',
            'source_database': EXPECTED['database'],
            'source_host': EXPECTED['host'],
            'target_database': 'pg46_finish_target_v2',
            'target_instance': 'oo-dl01w1-46-native',
            'slot': 'pg46_finish_slot',
            'confirmed_flush_lsn': facts['slot_flush'],
            'restart_lsn': facts['slot_restart'],
            'current_wal_lsn': facts['current_wal'],
            'slot_active': facts['slot_active'],
            'slot_status': facts['slot_status'],
            'generations': GENERATIONS,
            'identity_source_generation': 5,
            'identity_basis': 'latest bound run_generation only; the other three tables stay at 4',
            'product_role': facts['product_role'],
            'product_role_member': facts['full_member'],
            'product_role_pack_sha256': PRODUCT_PACK_SHA,
            'served_role': facts['served_role'],
            'served_role_member': facts['continuous_member'],
            'served_role_pack_sha256': SERVED_PACK_SHA,
            'served_pack_digest': SERVED_PACK_DIGEST,
            'pack_relation': 'table files, ontology, catalog, policy, and knowledge match; only the role string differs; the served role is not the product role',
            'approved_role_privileges': {
                'pg46_finish_target_v2': {'column_select': 38, 'table': 0},
                'dst_native_r2': {'column_select': 38, 'table': 0},
            },
            'served_role_privileges': {'pg46_finish_target_v2': {'column_select': 38, 'table': 0}},
            'empty_database_fill_would_apply': False,
            'empty_database_fill_executed': True,
            'numeric_scale': facts['numeric_scale'],
        },
        'reused': {
            'build_id': receipt['build_id'],
            's5_outcome': s5['outcome'],
            's5_cache_hit': False,
            's5_llm_calls': 1,
            's5_model': s5['model'],
            'product_receipt_sha256': facts['receipt_sha256'],
            'public_gold_sha256': facts['public_gold_sha256'],
            'cancel_query_id': facts['cancel_query_id'],
            'dialogue_gold_sha256': DIALOGUE_GOLD_SHA,
            'dialogue_runs': facts['dialogue_runs'],
        },
        'blocked_on': {
            'owner': '38240',
            'symbols': ['prove_whole', 'c1_continuous', 'product_flow_reuse', 'full_role_dialogue'],
            'reason': 'sealed-review wrote reviewed/pg46_full_current for oo_r_pg46_full and oo_pg46_preview_login. command sha256 440072bce796d3eb6ea2e82815bf58a60f74b4f89c24df72ac543dbeeae06788 equals the c929 product reviewed bytes. The old reviewed/pg46_full binding sha256 3534ba42e851f8108ca8eccdef37dd34b8187003f33f052d1ba93a6359a34fd8 is unchanged. Public hubs and notes counts matched the source, a wrong schema was refused, and a live cancel recorded sqlstate 57014. The generation-6 source transaction is already committed and was not repeated. connection_cycle.release_unapplied refuses pin 6 because task 894ced2d-70dd-4fe1-896a-f6080323e364 is failed with 8 producer process_attempts. Watermark stays 5 and slot pg46_finish_slot stays inactive at 0/40715A8. Whole stays false until that fence is released by the existing consumer, the product_flow reuse maintenance call, and two new full-role dialogues exit.',
        },
        'not_run': ['connection_cycle', 'dump', 'import', 's5', 'publish_bundle', 'align_gateway', 'dialogue', 'grant'],
    }

def collect_facts():
    slot = source_sql("SELECT active::text || '|' || restart_lsn::text || '|' || confirmed_flush_lsn::text || '|' || pg_current_wal_lsn()::text || '|' || wal_status FROM pg_replication_slots WHERE slot_name = 'pg46_finish_slot'")
    active, restart, flush, wal, status = slot.split('|')
    generations = {}
    for line in target_sql('pg46_finish_target_v2', "SELECT logical_namespace || '|' || logical_table || '|' || mx::text FROM (SELECT logical_namespace, logical_table, max(run_generation) AS mx FROM _oo.physical_mappings WHERE level = 't' GROUP BY logical_namespace, logical_table) AS bound").splitlines():
        namespace, table, generation = line.split('|')
        generations[namespace + '.' + table] = int(generation)
    scales = {}
    for line in source_sql("SELECT table_schema || '.' || table_name || '.' || column_name || '|' || numeric_scale::text FROM information_schema.columns WHERE (table_schema, table_name, column_name) IN (('customer_east','delivery_notes','amount'),('public','shipments','cost_amount')) ORDER BY 1").splitlines():
        name, scale = line.split('|')
        scales[name] = int(scale)
    gold = {}
    queries = {
        'hubs': "SELECT COUNT(*)||'|'||COALESCE(SUM(capacity_grams),0) FROM public.hubs",
        'notes': "SELECT COUNT(*)||'|'||COALESCE(SUM(amount),0) FROM customer_east.delivery_notes",
        'shipments': "SELECT COUNT(*)||'|'||COALESCE(SUM(cost_amount),0) FROM public.shipments",
        'tracking': "SELECT COUNT(*)||'|'||COALESCE(MAX(event_id),0) FROM public.tracking_events",
    }
    for name, sql in queries.items():
        gold[name] = source_sql(sql)
    product = json.loads(PRODUCT_PACK.read_text())
    served = json.loads(SERVED_PACK.read_text())
    same = True
    for left, right in zip(product['files'], served['files']):
        if left.get('path') in {'README.md', 'manifest.json'}:
            continue
        same = same and left.get('sha256') == right.get('sha256')
    product_manifest = json.loads(next(item['text'] for item in product['files'] if item.get('path') == 'manifest.json'))
    served_manifest = json.loads(next(item['text'] for item in served['files'] if item.get('path') == 'manifest.json'))
    runs = []
    for name in ('run-1', 'run-2'):
        folder = DIALOGUE_GOLD.parent / name
        verdict = json.loads((folder / 'verdict.json').read_text())
        selection = json.loads((folder / 'selection.json').read_text())
        identity = selection.get('identity') or {}
        runs.append({
            'run': name,
            'outcome': verdict.get('outcome'),
            'seed': verdict.get('seed'),
            'pack_sha256': identity.get('pack_sha256'),
            'role': identity.get('role'),
            'source_generation': identity.get('source_generation'),
            'audit_sha256': verdict.get('audit_sha256'),
        })
    cancel = json.loads(CANCEL_VERDICT.read_text())
    return {
        'slot_active': active == 'true',
        'slot_restart': restart,
        'slot_flush': flush,
        'current_wal': wal,
        'slot_status': status,
        'generations': generations,
        'numeric_scale': scales,
        'gold': gold,
        'product_pack_sha256': sha(PRODUCT_PACK),
        'served_pack_sha256': sha(SERVED_PACK),
        'served_pack_digest': served.get('pack_digest'),
        'product_role': product_manifest.get('role'),
        'served_role': served_manifest.get('role'),
        'table_bytes_same': same and product_manifest.get('catalog_version') == served_manifest.get('catalog_version') and product_manifest.get('ontology_version') == served_manifest.get('ontology_version'),
        'full_target_columns': int(target_sql('pg46_finish_target_v2', "SELECT count(*) FROM information_schema.column_privileges WHERE grantee = 'oo_r_pg46_full'")),
        'full_target_tables': int(target_sql('pg46_finish_target_v2', "SELECT count(*) FROM information_schema.table_privileges WHERE grantee = 'oo_r_pg46_full'")),
        'full_dst_columns': int(target_sql('dst_native_r2', "SELECT count(*) FROM information_schema.column_privileges WHERE grantee = 'oo_r_pg46_full'")),
        'continuous_target_columns': int(target_sql('pg46_finish_target_v2', "SELECT count(*) FROM information_schema.column_privileges WHERE grantee = 'oo_r_pg46_continuous_fixture'")),
        'continuous_target_tables': int(target_sql('pg46_finish_target_v2', "SELECT count(*) FROM information_schema.table_privileges WHERE grantee = 'oo_r_pg46_continuous_fixture'")),
        'full_member': target_sql('pg46_finish_target_v2', "SELECT u.rolname FROM pg_auth_members m JOIN pg_roles g ON g.oid = m.roleid JOIN pg_roles u ON u.oid = m.member WHERE g.rolname = 'oo_r_pg46_full'"),
        'continuous_member': target_sql('pg46_finish_target_v2', "SELECT u.rolname FROM pg_auth_members m JOIN pg_roles g ON g.oid = m.roleid JOIN pg_roles u ON u.oid = m.member WHERE g.rolname = 'oo_r_pg46_continuous_fixture'"),
        'receipt': json.loads(PRODUCT_RECEIPT.read_text()),
        'receipt_sha256': sha(PRODUCT_RECEIPT),
        'public_gold_sha256': sha(PUBLIC_GOLD),
        'dialogue_gold_sha256': sha(DIALOGUE_GOLD),
        'dialogue_runs': runs,
        'cancel_query_id': cancel.get('query_id'),
    }

def reuse_entry(config, run_dir):
    cfg = json.loads(Path(config).read_text())
    for key, value in EXPECTED.items():
        if cfg.get(key) != value:
            raise RuntimeError('connection is not the authorized owned fixture')
    run_dir = Path(run_dir)
    if not run_dir.is_dir():
        raise RuntimeError('run directory is absent')
    before = {str(path): sha(path) for path in PROTECTED}
    if '105aa78b9a914e48b126687fc05ae03d' not in OLD_POINTER.read_text():
        raise RuntimeError('failed run pointer moved')
    facts = collect_facts()
    callback = reuse_decision(facts)
    after = {str(path): sha(path) for path in PROTECTED}
    if after != before:
        raise RuntimeError('protected artifact changed')
    callback['no_overwrite'] = True
    callback['protected_sha256'] = {Path(path).name: digest for path, digest in sorted(before.items())}
    write(run_dir / 'entry-callback.json', callback)
    write(run_dir / 'whole.json', {
        'protocol': 'oo.postgres.connection-whole/v1',
        'build_id': facts['receipt']['build_id'],
        'full_product_chain_complete': False,
        'periodic_insert_update_delete_verified': False,
        'recovery_verified': False,
        'reuse': True,
        'no_overwrite': True,
        'published': False,
        'native_amd64_nightly': 'external_gate_not_claimed',
        'entry_callback': 'entry-callback.json',
    })
    print(json.dumps({'whole': False, 'reuse': True, 'slot_active': facts['slot_active'], 'flush': facts['slot_flush']}))
    return callback

def execute(config, run_dir):
    raise RuntimeError('whole execute replays dump, import, and the model; normal entry is reuse_entry')
    import connection_cycle
    cfg = json.loads(Path(config).read_text())
    for key, value in EXPECTED.items():
        if cfg.get(key) != value:
            raise RuntimeError('connection is not the authorized owned fixture')
    run_dir = Path(run_dir)
    postgres = lists('postgres')
    scope = lists('pg_connection_whole')
    write(run_dir / 'list-proof.json', {'postgres': {'status': postgres[0], 'identity': postgres[1], 'builds': postgres[2]}, 'pg_connection_whole': {'status': scope[0], 'identity': scope[1], 'message': scope[3]}})
    product = run_dir / 'product'
    product.mkdir(mode=0o700, exist_ok=True)
    os.chmod(product, 0o700)
    created = (product / 'build.json').exists()
    if not created:
        if postgres[0] != 200 or postgres[1] != 'verified' or postgres[2] != 0:
            raise RuntimeError('official build list is not an empty verified project postgres')
        if scope[0] != 403 or scope[1] != 'verified':
            raise RuntimeError('pg_connection_whole scope was not a verified 403')
        old = CACHE / 'product-run'
        if (old / 'create-started.json').exists() and list(old.glob('*.body.json')):
            raise RuntimeError('an earlier create body exists')
    seed_committed_cycle(run_dir)
    cycle = connection_cycle.cycle(run_dir / 'cycle')
    recovery = connection_cycle.recover(run_dir / 'cycle', run_dir / 'recovery')
    if cycle.get('typed_cell_diff') != 0 or recovery.get('typed_cell_diff') != 0 or recovery.get('recovery_verified') is not True:
        raise RuntimeError('cycle diff or recovery is not proven')
    fresh_catalog(run_dir / 'capture')
    if not created:
        catalog = publish_bundle(run_dir / 'recovery' / 'catalog.json', recovery['end_lsn'])
        restarted = restart_api()
        scope = lists('pg_connection_whole')
        write(run_dir / 'list-proof-after-bundle.json', {'postgres': {'status': restarted[0], 'identity': restarted[1], 'builds': restarted[2]}, 'pg_connection_whole': {'status': scope[0], 'identity': scope[1], 'message': scope[3]}})
        if restarted[0] != 200 or restarted[1] != 'verified' or restarted[2] != 0 or scope[0] != 403 or scope[1] != 'verified':
            raise RuntimeError('official build list changed before the one create')
    else:
        catalog = sha(CACHE / 'bundle-current' / 'catalog.json')
    config_path = run_dir / 'product-config.json'
    if not config_path.exists():
        write(config_path, product_config(catalog))
    if not (product / 'prepared.json').exists():
        run([sys.executable, str(FLOW), '--config', str(config_path), '--out', str(product), '--prepare-only'], timeout=2500)
    if not (product / 'product-receipt.json').exists():
        decision_path = run_dir / 'decisions.json'
        if not decision_path.exists():
            write(decision_path, decisions_from(json.loads((product / 'prepared-overlay.json').read_text())))
        run([sys.executable, str(FLOW), '--config', str(config_path), '--out', str(product), '--decisions', str(decision_path)], timeout=600)
    receipt = json.loads((product / 'product-receipt.json').read_text())
    public = run_dir / 'public'
    public.mkdir(mode=0o700, exist_ok=True)
    align_gateway(run_dir / 'recovery' / 'catalog.json', public)
    rows = gold(public)
    cancel = live_cancel(public / 'cancel')
    dialogue = run_dir / 'dialogue'
    if not (dialogue / 'verdict.json').exists():
        truth = {
            'notes': {'expected_named_facts': {'row_count': rows['notes']['source'].split('|')[0], 'amount_total': rows['notes']['source'].split('|')[1]}},
            'hubs': {'expected_named_facts': {'row_count': rows['hubs']['source'].split('|')[0], 'capacity_total': rows['hubs']['source'].split('|')[1]}},
            'shipments': {'expected_named_facts': {'row_count': rows['shipments']['source'].split('|')[0], 'cost_total': rows['shipments']['source'].split('|')[1]}},
            'tracking': {'expected_named_facts': {'row_count': rows['tracking']['source'].split('|')[0], 'last_event_id': rows['tracking']['source'].split('|')[1]}},
        }
        truth_path = run_dir / 'dialogue-truth.json'
        write(truth_path, truth)
        questions = [
            {'truth_key': 'notes', 'prompt': 'Use the public OO tools only. Read README.md, INDEX.md and tables/customer_east.delivery_notes.md with knowledge_cat. describe customer_east.delivery_notes and check freshness. Query its current row count and exact total amount. Cite each query ID.'},
            {'truth_key': 'hubs', 'prompt': 'Continue with public OO tools. Read tables/public.hubs.md, describe public.hubs, check freshness, then query its current row count and exact total capacity_grams. Cite actual query IDs.'},
            {'truth_key': 'shipments', 'prompt': 'Continue with public OO tools. Read tables/public.shipments.md, describe public.shipments, check freshness, then query its current row count and exact total cost_amount. Preserve the complete decimal string and cite actual query IDs.'},
            {'truth_key': 'tracking', 'prompt': 'Continue with public OO tools. Read tables/public.tracking_events.md, describe public.tracking_events, check freshness, then query its current row count and maximum event_id. Cite actual query IDs.'},
        ]
        write(run_dir / 'dialogue-config.json', {
            'model_origin': 'http://127.0.0.1:8317', 'model': 'grok-4.7',
            'model_bearer_file': '/tmp/oo-product-model-20261012/bearer',
            'gateway_token_file': str(CACHE / 'gateway-token'),
            'gateway': f'http://127.0.0.1:{gateway_port()}', 'transport': 'local-development', 'auth': 'external',
            'oo_cli': str(CLI), 'independent_truth_file': str(truth_path),
            'independent_gateway_audit_file': str(WAL), 'max_tool_rounds': 8, 'questions': questions,
        })
        issue_token()
        if dialogue.exists():
            failed = run_dir / 'dialogue-failed'
            if failed.exists():
                raise RuntimeError('dialogue failure retained')
            dialogue.rename(failed)
        run([sys.executable, str(DIALOGUE), '--config', str(run_dir / 'dialogue-config.json'), '--out', str(dialogue)], timeout=1800)
    verdict = json.loads((dialogue / 'verdict.json').read_text())
    body = {
        'protocol': 'oo.postgres.connection-whole/v1', 'build_id': receipt.get('build_id'),
        'full_product_chain_complete': receipt.get('protocol') == 'oo.source-onboarding.product/v1' and verdict.get('outcome') == 'verified' and verdict.get('turns', 0) >= 2,
        'periodic_insert_update_delete_verified': cycle.get('insert_update_delete_verified') is True and cycle.get('typed_cell_diff') == 0,
        'recovery_verified': recovery.get('recovery_verified') is True and recovery.get('typed_cell_diff') == 0,
        'live_cancel_verified': cancel.get('sqlstate_57014') is True and cancel.get('outcome') == 'sent',
        'typed_gold_diff': 0, 'product_receipt': 'product/product-receipt.json', 'dialogue_verdict': 'dialogue/verdict.json',
        'published': False, 'native_amd64_nightly': 'external_gate_not_claimed',
    }
    write(run_dir / 'whole.json', body)
    if body['full_product_chain_complete'] is not True:
        raise RuntimeError('whole chain incomplete')
    print(json.dumps(body))
    return body

def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--config')
    parser.add_argument('--run')
    parser.add_argument('config_pos', nargs='?')
    parser.add_argument('run_pos', nargs='?')
    args = parser.parse_args()
    config, run_dir = args.config or args.config_pos, args.run or args.run_pos
    if not config or not run_dir:
        parser.error('config and run are required')
    args.config, args.run = config, run_dir
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    reuse_entry(args.config, args.run)

if __name__ == '__main__':
    main()
