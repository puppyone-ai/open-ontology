"""Anonymous multi-source library regression through a disposable Agent DB fixture."""
import argparse
import json
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import library_selfbuild as lib


def docker(*args, input=None):
    result = subprocess.run(['docker', *args], input=input, text=True, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError('fixture_docker_command_failed')
    return result.stdout


def inventory():
    result = []
    for index in range(70):
        columns = [('id', 'bigint', '主键'), ('owner_unit_id', 'bigint', '所属网点编号'), ('status', 'text', '状态')]
        if index == 0:
            columns.extend((f'measure_{i:03}', 'bigint', '数量') for i in range(499))
        result.append(dict(dataset=f'app.item_{index:03}', columns=columns, key='id', rows=100, source=f'shard_{index // 16}'))
    result.append(dict(dataset='app.unit', columns=[('unit_id', 'bigint', '网点编号'), ('unit_name', 'text', '网点名称')], key='unit_id', rows=3, source='master'))
    result.append(dict(dataset='app.lookup', columns=[('id', 'bigint', '主键'), ('kind_code', 'text', '代码类型'), ('value_code', 'text', '代码'), ('value_name', 'text', '名称')], key='id', rows=3, source='master'))
    return result


def install(container, ddl):
    sql = ['CREATE SCHEMA _oo',
           'CREATE TABLE _oo.table_freshness(schema_name text,table_name text,source text,source_generation bigint,target_generation bigint,physical_name text,state text)',
           'CREATE TABLE _oo.physical_mappings(source text,logical_namespace text,logical_table text,logical_column text,physical_name text,scope_namespace_physical text,scope_table_physical text,level text)']
    for ordinal, table in enumerate(inventory()):
        schema, name = table['dataset'].split('.')
        physical = f'pt_fixture_{ordinal:03}'
        ddl_lines = [f'CREATE TABLE `{name}` (']
        definitions, values = [], []
        for index, (column, typ, comment) in enumerate(table['columns']):
            mapped = f'pc_fixture_{index:03}'
            definitions.append(lib.quote(mapped) + ' ' + typ)
            ddl_lines.append(f"  `{column}` {typ} COMMENT '{comment}',")
            sql.append('INSERT INTO _oo.physical_mappings VALUES (' + ','.join(lib.literal(v) for v in [table['source'], schema, name, column, mapped, '_oo', physical, 'c']) + ')')
            if column in [table['key']]:
                value = 'n'
            elif column == 'owner_unit_id':
                value = '(n%3)+1'
            elif column == 'status':
                value = "CASE WHEN n=100 THEN '' WHEN n=99 THEN 'missing' ELSE ((n%3)+1)::text END"
            elif column == 'value_code':
                value = '((n%3)+1)::text'
            elif column == 'kind_code':
                value = "'status'::text"
            elif typ == 'text':
                value = "'fixture-'||n::text"
            else:
                value = '1'
            values.append(value)
        ddl_lines.extend([f"  PRIMARY KEY (`{table['key']}`)", ');'])
        (ddl / (name + '.sql')).write_text('\n'.join(ddl_lines))
        sql.append('CREATE TABLE _oo.' + lib.quote(physical) + '(' + ','.join(definitions) + ')')
        sql.append('INSERT INTO _oo.' + lib.quote(physical) + ' SELECT ' + ','.join(values) + ' FROM generate_series(1,' + str(table['rows']) + ') n')
        sql.append('INSERT INTO _oo.table_freshness VALUES (' + ','.join(lib.literal(v) for v in [schema, name, table['source']]) + ",1,2," + lib.literal(physical) + ",'live')")
    sql.append('ANALYZE')
    docker('exec', '-i', container, 'psql', '-X', '-w', '-U', 'postgres', '-v', 'ON_ERROR_STOP=1', input=';\n'.join(sql) + ';')


class FixtureModel(BaseHTTPRequestHandler):
    calls = 0

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        content = request['messages'][1]['content']
        batch = json.loads(content[content.index('{'):])
        assert len(batch['tables']) <= 16
        assert 'relationships' in batch and 'enum_candidates' in batch and 'neighbors' in batch
        draft = {'datasets': [{'dataset': t['dataset'], 'label': t['dataset'], 'description': 'Anonymous fixture description',
                               'columns': [{'name': c['name'], 'label': c['name'], 'description': 'Anonymous fixture field'} for c in t['columns']]}
                              for t in batch['tables']],
                 'glossary': [{'term': 'Fixture term ' + str(batch['index']), 'definition': 'Anonymous approved fixture term', 'dataset': batch['tables'][0]['dataset']}],
                 'metrics': [], 'examples': []}
        body = lib.canonical({'choices': [{'finish_reason': 'stop', 'message': {'content': '```json\n' + lib.canonical(draft) + '\n```'}}], 'usage': {'total_tokens': 0}}).encode()
        type(self).calls += 1
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def run(output, builder):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    ddl = output / 'ddl'
    ddl.mkdir()
    container, volume = 'oo-wf-bld-fixture-pg', 'oo-wf-bld-fixture-data'
    existing = docker('ps', '-a', '--format', '{{.Names}}').splitlines()
    if container in existing or volume in docker('volume', 'ls', '--format', '{{.Name}}').splitlines():
        raise ValueError('fixture_name_collision')
    server = HTTPServer(('127.0.0.1', 0), FixtureModel)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    created = False
    try:
        docker('volume', 'create', volume)
        created = True
        docker('run', '-d', '--name', container, '--network', 'none', '--mount', f'type=volume,source={volume},target=/var/lib/postgresql/data', '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', 'postgres:17')
        ready = False
        for _ in range(30):
            result = subprocess.run(['docker', 'exec', container, 'pg_isready', '-U', 'postgres'], capture_output=True)
            if result.returncode == 0:
                ready = True
                break
            time.sleep(1)
        if not ready:
            raise RuntimeError('fixture_database_not_ready')
        install(container, ddl)
        db = lib.ReadOnlyPostgres(['docker', 'exec', '-i', container, 'psql', '-U', 'postgres', '-d', 'postgres'])
        tables = lib.collect_tables(db, ddl)
        facts = lib.build_facts(tables, lib.database_containment(db), lib.database_enums(db))
        lib.write(output / 'facts.json', facts)
        bearer = output / 'fixture-bearer'
        bearer.write_text('fixture-test-token')
        bearer.chmod(0o600)
        ceilings = {'max_datasets': 4096, 'max_columns': 1024, 'max_effective_columns': 1024,
                    'max_relationships': 32768, 'max_enums': 32768, 'max_entities': 4096, 'max_entries': 16384,
                    'max_file_bytes': 4194304, 'max_total_bytes': 67108864,
                    'json_scan': {'max_bytes': 67108864, 'max_nodes': 4000000, 'max_depth': 32}}
        lib.write(output / 'ceilings.json', ceilings)
        config = {'draft_datasets': [t['dataset'] for t in tables], 'max_draft_batches': 5,
                  'model': 'fixture/test', 'endpoint': f'http://127.0.0.1:{server.server_port}/v1',
                  'bearer_file': str(bearer), 'fixture_review': True, 'builder_binary': builder,
                  'pack_ceilings_file': str(output / 'ceilings.json')}
        receipts = lib.run_drafts(facts, config, output)
        before = FixtureModel.calls
        assert lib.run_drafts(facts, config, output) == receipts
        assert FixtureModel.calls == before
        pack = lib.compile_pack(facts, receipts, config, output)
        files = {f['path']: f['text'] for f in pack['artifact']['files']}
        assert len(tables) == 72 and max(len(t['columns']) for t in tables) == 502
        assert len(facts['relationships']) == 70 and len(receipts) == 5
        assert sum(bool(e['dictionary_candidates']) for e in facts['enum_candidates']) >= 70
        assert len([p for p in files if p.startswith('tables/')]) == 72
        assert 'app.unit' in files['relationships.md'] and 'Fixture term' in files['glossary.md']
        report = {'tables': 72, 'sources': len({t['source_id'] for t in tables}), 'wide_columns': 502,
                  'relationships': 70, 's5_batches': 5, 's5_mock': True, 'cache_resume_additional_calls': FixtureModel.calls - before,
                  'single_pack_digest': pack['pack_digest'], 'business_owner_signed': False}
        lib.write(output / 'report.json', report)
        print(lib.canonical(report))
    finally:
        server.shutdown()
        server.server_close()
        if created:
            subprocess.run(['docker', 'stop', container], capture_output=True)
            subprocess.run(['docker', 'rm', container], capture_output=True)
            docker('volume', 'rm', volume)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    parser.add_argument('--builder', required=True)
    args = parser.parse_args()
    run(args.output, args.builder)
