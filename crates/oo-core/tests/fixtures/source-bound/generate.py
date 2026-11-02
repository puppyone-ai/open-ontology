#!/usr/bin/env python3
"""Regenerate synthetic Oracle catalogs/dictionaries; no source or historical evidence."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).parent
DIGEST = 'ab' * 32
ID = ('id', 21, -1, 'int16', 'ID|NUMBER|4|0|0')
CASES = {
    'batch': [ID, ('n16', 1700, (16 << 16) + 6 + 4, 'decimal128', 'N16|NUMBER|16|6|0'),
              ('n1', 21, -1, None, 'N1|NUMBER|1|0|0'),
              ('d', 1114, -1, 'timestamp_micros', 'D|DATE|-|-|0'),
              ('c6', 1042, 10, 'utf8', 'C6|CHAR|-|-|6'),
              ('v2', 1043, -1, 'utf8', 'V2|VARCHAR2|-|-|40'),
              ('r', 17, -1, 'binary', 'R|RAW|-|-|0')],
    'wide': [ID, ('n_free_text', 1043, -1, 'utf8', 'N_FREE|NUMBER|-|-|0')],
    'ts': [ID, ('ts_text', 1043, -1, 'utf8', 'TS|TIMESTAMP(9)|-|9|0'),
           ('tstz_text', 1043, -1, 'utf8', 'TSTZ|TIMESTAMP(9) WITH TIME ZONE|-|9|0')],
}


def generate():
    for case, fields in CASES.items():
        physical = 'pt_' + '44' * 30
        preimage = f'fixture.{case}\n' + ''.join(f'{n}\t{o}\t{t}\t\n' for n, o, t, _, _ in fields)
        schema = hashlib.sha256(preimage.encode()).hexdigest()
        catalog = dict(protocol='oo.pipeline.sealed-import-catalog/v1', capture='sealed_import',
                       state='awaiting_publish', engine='oracle', source_id='synthetic_' + case,
                       source_generation=1, target_generation=1,
                       run_id='11111111-1111-4111-8111-111111111111', schema_digest=schema,
                       config_digest=DIGEST, input_digest=DIGEST, type_digest=DIGEST,
                       tool_hook_digest=DIGEST, hook_checked=False, shadows_queried=False,
                       source_introspection='not_used', quality=dict(
                           loss_concealed=False, manifest_reject_count=0,
                           payload_includes_reject_lines=False, stored_omits_manifest_rejects=False,
                           stored_rejected_rows=0, stored_rows=[dict(
                               accepted_rows=1, attnum=None, batch_id='t0.b0', key_digest=None,
                               kind='batch', physical_name=physical, reason=None, rejected_rows=0,
                               row_index=None, value_len=None)]), tables=[dict(
                           logical_schema='fixture', logical_table=case, accepted_rows=1,
                           canonical_digest=DIGEST, stored_namespace='pn_' + '33' * 30,
                           stored_physical=physical, rejects=[], columns=[dict(
                               logical_name=n, oid=o, typmod=t, caveat=None, key=n == 'id',
                               source_type=s, stored_physical='pc_' + f'{i+1:060x}')
                               for i, (n, o, t, s, _) in enumerate(fields)])])
        audit = {k: catalog[k] for k in ['source_id', 'run_id', 'schema_digest']}
        audit.update(banner='Synthetic Oracle fixture 23.26.3.0.0', scn='100',
                     published_scn='100', registry_snapshot='fixture-registry-100',
                     dictionary_columns=[f[-1] for f in fields])
        for name, body in [(f'catalog-{case}/catalog.json', catalog),
                           (f'identity-67/{case}/audit/readonly-audit.json', audit)]:
            path = ROOT / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(body, indent=2, sort_keys=True) + '\n')

if __name__ == '__main__':
    generate()
