#!/usr/bin/env python3
"""Owned anonymous PG17 library publication/adoption acceptance harness."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

import library


def main():
    fixture = json.loads(Path(os.environ['OO_LIBRARY_FIXTURE']).read_text())
    port = os.environ['OO_LIBRARY_PG_PORT']
    database = fixture['database']
    pipeline = Path(os.environ['OO_LIBRARY_PIPELINE']).resolve()
    coordinator = Path(os.environ['OO_LIBRARY_COORDINATOR']).resolve()
    ca = str(Path(os.environ['OO_LIBRARY_CA']).resolve())
    shape = json.loads(Path(os.environ['OO_LIBRARY_EFFECTIVE']).read_text())

    def sql(command):
        result = subprocess.run(['psql', '-X', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1',
                                 '-p', port, '-U', 'postgres', '-d', database, '-tAc', command],
                                text=True, capture_output=True, timeout=60)
        if result.returncode:
            raise AssertionError(result.stderr)
        return result.stdout.strip()

    sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_release_admin') THEN CREATE ROLE oo_release_admin LOGIN; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_release_reader') THEN CREATE ROLE oo_release_reader LOGIN; END IF; END $$")
    sql(f'GRANT CREATE,CONNECT ON DATABASE {database} TO oo_release_admin')
    sql('GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint),pg_catalog.pg_advisory_lock(bigint),pg_catalog.pg_advisory_unlock(bigint) TO oo_release_admin')
    sql('GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_library_reader')
    membership = fixture['receipt']['library']['manifest']
    initial_command = os.environ.get('OO_LIBRARY_INITIAL_COMMAND', 'publish')
    for first in fixture['receipt']['shards'] if initial_command == 'adopt' else fixture['receipt']['shards'][:1]:
        encoded = json.dumps(first['receipt']).replace("'", "''")
        sql(f"SELECT _oo.publish_native_per_table_collection('{first['anchor_run']}'::uuid,'{first['receipt_sha256']}','{encoded}'::jsonb)")
    effective = shape['effective']
    visibility = shape['visibility']
    prototype = effective['datasets'][0]
    effective['datasets'] = []
    visibility['datasets'] = []
    effective['ontology']['include_schemas'] = ['app']
    visibility['role'] = 'oo_r_lib_all'
    for source in membership['sources']:
        for table in source['tables']:
            dataset = copy.deepcopy(prototype)
            dataset['dataset'] = dataset['source_name'] = f'app.{table}'
            dataset['label'] = table
            dataset['columns'][0]['data_type'] = 'int8'
            dataset['columns'][1]['name'] = dataset['columns'][1]['label'] = 'value'
            effective['datasets'].append(dataset)
            visibility['datasets'].append(dict(dataset=f'app.{table}', columns=['id', 'value'],
                                               row_security='none', global_stats='denied'))
    with tempfile.TemporaryDirectory(prefix='oo-wf-lib-product-') as d:
        root = Path(d)
        password = root/'password'
        password.write_text('fixture-only\n')
        password.chmod(0o600)
        config = dict(host='127.0.0.1', port=int(port), database=database, user='oo_release_admin',
                      password_file=str(password), ca_file=ca, deployment_mode='primary_replica' if os.environ.get('OO_LIBRARY_REPLICA_PORT') else 'all_in_one')
        control_path = root/'control.json'
        control_path.write_text(json.dumps(config))
        maintained = dict(config, target_host='127.0.0.1', target_port=int(port),
                          target_database=database, target_user='oo_target_admin',
                          target_password_file=str(password), target_ca_file=ca,
                          target_reader_user='oo_library_reader', target_reader_password_file=str(password))
        if os.environ.get('OO_LIBRARY_REPLICA_PORT'):
            maintained['replica_pg'] = dict(host='127.0.0.1', port=int(os.environ['OO_LIBRARY_REPLICA_PORT']),
                                             database=database, user='oo_target_admin',
                                             password_file=str(password), ca_file=ca)
        maintained_path = root/'maintain.json'
        maintained_path.write_text(json.dumps(maintained))

        def release(command, document):
            run = subprocess.run([str(coordinator), command, '--config', str(control_path)],
                                 input=json.dumps(document), text=True, capture_output=True, timeout=120)
            assert run.stdout, (command, run.returncode, run.stderr)
            result = json.loads(run.stdout)
            assert run.returncode == 0, (command, result, run.stderr)
            return result

        release('install', dict(protocol='oo.release-coordinator/install/v1', project_id='shop',
                                authority_id='anonymous-library-fixture', covers_gates=[
                                    'G1', 'G2', 'G3', 'G4', 'G5a', 'G5b', 'G6', 'G7', 'G8',
                                    'G9', 'G10', 'G11', 'G12', 'G13']))
        staged = release('stage', dict(protocol='oo.release-coordinator/stage/v1', project_id='shop',
                                      candidate_id='library-candidate', version='library-v1', base_version=None,
                                      effective_oos=effective, visibility=visibility, policy_id='pol-shop',
                                      policy_version='1', knowledge_id='know-shop', knowledge_version='1',
                                      catalog_version='cat-1'))
        binding = dict(pipeline=dict(path=str(pipeline), sha256=hashlib.sha256(pipeline.read_bytes()).hexdigest()),
                       coordinator=dict(path=str(coordinator), sha256=hashlib.sha256(coordinator.read_bytes()).hexdigest()),
                       target=dict(host='127.0.0.1', port=int(port), database=database,
                                   runtime_principal='oo_target_admin', password_file=str(password),
                                   ssl='verify-full', ca_file=ca), release_config=str(maintained_path),
                       activation=dict(project_id='shop', candidate_id='library-candidate',
                                       source_hash=staged['source_hash'], expected_version=None,
                                       schema_name='app', relation_name=membership['sources'][0]['tables'][0],
                                       role_name='oo_r_lib_all'))
        precheck = library.publish_library(membership, binding, root/'precheck', True)
        assert not precheck['published']
        assert sql('SELECT count(*) FROM _oo.native_library_publication') == '0'
        before = sql("SELECT count(*)||':'||sum(applied_rows) FROM _oo.full_import_table")
        manifest_path = root/'membership.json'
        manifest_path.write_text(json.dumps(membership))
        binding_path = root/'binding.json'
        binding_path.write_text(json.dumps(binding))

        def product(command, output, recover=False):
            run = subprocess.run(['python3', str(Path(library.__file__)), command,
                                  '--manifest', str(manifest_path), '--binding', str(binding_path),
                                  '--out', str(root/output)] + (['--recover'] if recover else []), text=True, capture_output=True, timeout=180)
            if run.returncode:
                if (root/output/'release-receipt.json').exists():
                    print((root/output/'release-receipt.json').read_text())
                raise AssertionError((run.returncode, run.stdout, run.stderr))
            return json.loads(run.stdout)

        published = product(initial_command, 'published')
        assert published['published']
        assert published['release']['native_publication']['ready'] is True
        assert published['release']['deployment_mode'] == config['deployment_mode']
        assert published['release']['readonly_replica'] == bool(os.environ.get('OO_LIBRARY_REPLICA_PORT'))
        assert published['release']['visibility_observation']['readonly_replica'] == bool(os.environ.get('OO_LIBRARY_REPLICA_PORT'))

        adopted = product('adopt', 'adopted')
        assert adopted['published']
        assert adopted['receipt_sha256'] == published['receipt_sha256']
        assert sql('SELECT count(*) FROM _oo.native_library_publication') == '1'
        assert sql("SELECT count(*) FROM _oo.native_library_source_status WHERE status='superseded_by_library'") == '3'
        assert sql('SELECT count(*) FROM _oo.native_library_head WHERE state=\'ready\'') == '1'
        assert sql("SELECT count(*)||':'||sum(applied_rows) FROM _oo.full_import_table") == before
        assert sql('SELECT count(DISTINCT (candidate_id,opened_version,opened_pack_digest)) FROM oo_publish_target.fence WHERE state=\'open\'') == '1'
        library_id=membership['library_id']
        sql(f"SELECT _oo.native_library_fail_closed('{library_id}','{published['receipt_sha256']}')")
        assert sql("SELECT count(*) FROM oo_publish_target.fence WHERE state='open'") == '0'
        recovered = product('adopt', 'recovered', recover=True)
        assert recovered['published']
        assert sql("SELECT count(*) FROM oo_publish_target.fence WHERE state='open'") == '36'
        assert sql('SELECT count(*) FROM _oo.native_library_publication') == '1'
        checked = subprocess.run(['python3', str(Path(library.__file__)), 'status', '--manifest', str(manifest_path),
                                  '--binding', str(binding_path)], text=True, capture_output=True, timeout=60)
        assert checked.returncode == 0, checked.stderr
        observed = json.loads(checked.stdout)
        assert observed['publication'] == 'ready' and observed['imported'] == 36
        assert observed['generation_vector_verified']
        print(json.dumps(dict(protocol='oo.library.anonymous-acceptance/v1', tables=36, shards=3,
                              publish=True, adopt=True, reimport=False, replay=True, recover=True, initial_command=initial_command,
                              receipt_sha256=published['receipt_sha256'], pack_digest=staged['pack_digest'])))


if __name__ == '__main__':
    main()
