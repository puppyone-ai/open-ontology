#!/usr/bin/env python3
"""Bind a configured native Doris capture to exact admission/Loader inputs."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DECLARATION = ROOT / 'testing/dl01-driver-preflight/doris/scripts/capture_receipt.py'
DECLARATION_SHA = '981e51e0cefd70d2cea97fe4c612ed6c5b4ff175045701e768a1a06ba828bce1'


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def require(ok):
    if not ok:
        raise ValueError('Doris import preparation refused')


def save(p, x):
    with p.open('x') as f:
        json.dump(x, f, indent=2)
        f.write('\n')


def main():
    parser = argparse.ArgumentParser()
    for f in ('config', 'capture', 'out'):
        parser.add_argument('--' + f, required=True)
    a = parser.parse_args()
    os.umask(0o077)
    c = json.loads(Path(a.config).read_text())
    tick_path = Path(a.capture).resolve()
    tick = json.loads(tick_path.read_text())
    require(tick['protocol'] == 'oo.doris.capture-tick/v1')
    require(tick['database'] == c['database'] and tick['product_reader_write_attempts'] == 0)
    require(sha(c['reader']) == c['reader_sha256'] and sha(DECLARATION) == DECLARATION_SHA)
    spec = importlib.util.spec_from_file_location('doris_decl', DECLARATION)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    require({t['name'] for t in tick['tables']} == set(module.TABLES))
    out = Path(a.out).resolve()
    out.mkdir(mode=0o700)
    config = dict(c['admission'])
    config['secret_ref'] = c['secret_ref']
    require(config['requested_write_class'] == 'R0' and config['source_id'] == c['source_id'])
    save(out / 'admission-config.json', config)
    for offset, table in enumerate(tick['tables']):
        name = table['name']
        generation = c['first_generation'] + offset
        directory = out / name
        directory.mkdir()
        arrow = tick_path.parent / table['arrow']
        require(table['route'] == 'C5' and table['delete_mode'] == 'reconcile')
        witness = {'protocol': 'oo.source-admission.capture-witness/v1', 'source_engine': 'doris',
                   'source_id': c['source_id'], 'source_generation': generation,
                   'snapshot': {'kind': 'capture-tick', 'value': tick['label']},
                   'run_id': c['source_id'] + '-' + tick['label'] + '-' + name + '-' + str(tick['finished_unix_ms']),
                   'logical_schema': c['database'], 'logical_table': name,
                   'capturer_path': str(tick_path), 'arrow_path': str(arrow), 'driver_path': c['reader'],
                   'schema_preimage_path': c['schema_preimage'], 'type_preimage_path': c['type_preimage']}
        save(directory / 'witness.json', witness)
        report = {'protocol': 'oo.source-admission.report/v1', 'source_id': c['source_id'],
                  'target_id': c['source_id'], 'secret_ref': c['secret_ref'],
                  'driver': {'kind': 'native', 'name': 'doris-reader', 'version': '0.1.0', 'digest': c['reader_sha256']},
                  'compat_mode': 'doris', 'license_flags': ['Apache-2.0'], 'permissions': config['permissions'],
                  'capabilities': [{'name': 'BatchFull', 'cursor': None, 'feed': None, 'cdc': None,
                                    'token_order': None, 'budget_unit': None, 'level': 'experimental',
                                    'write_class': 'R0', 'consistency': 'S0'}], 'capture_methods': [], 'r1w': None,
                  'cost_model': {'unit': 'Rows', 'confidence': 'unknown',
                                 'estimate': 'Doris billing unpublished; configured source row cap'},
                  'capture_witness': str(directory / 'witness.json')}
        save(directory / 'report.json', report)
        declaration = module.TABLES[name]
        request = {'protocol': 'oo.pipeline.native-update/v1', 'source_engine': 'doris',
                   'source_id': c['source_id'], 'source_generation': generation,
                   'parent_source_generation': generation - 1 if generation > 1 else None,
                   'snapshot': witness['snapshot'], 'logical_schema': c['database'], 'logical_table': name,
                   'batch_id': tick['label'] + '-' + name, 'capture_method': 'c5_diff',
                   'delete_detection': 'reconcile', 'consistency_grade': table['consistency'],
                   'cross_table_consistent': False, 'full_snapshot': True, 'mirror': True,
                   'row_identity': declaration['identity'], 'key_columns': declaration['keys'],
                   'position_codec': 'snapshot-zero/v1', 'position_lineage': 'doris.snapshot.' + name,
                   'applied_pos': '0', 'input_digest': sha(arrow), 'audit_digest': sha(tick_path),
                   'arrow_path': str(arrow), 'admission_path': str(directory / 'sealed/admission-receipt.json'),
                   'registry_path': str(directory / 'sealed/registry.json'),
                   'columns': [module.column_decl(column) for column in declaration['columns']],
                   'tombstones': [], 'target': c['target']}
        save(directory / 'request-unsealed.json', request)
    save(out / 'preparation-receipt.json', {'source_capture_sha256': sha(tick_path),
                                         'declaration_sha256': DECLARATION_SHA,
                                         'state': 'awaiting_admission', 'tables': len(tick['tables'])})


if __name__ == '__main__':
    main()
