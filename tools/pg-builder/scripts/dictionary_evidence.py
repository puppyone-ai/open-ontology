"""Pinned DOCX description candidates; never permission or business approval.

Config: dictionary_evidence_input = {path, sha256, dataset, dictionary_sha256,
ddl_sha256}. The file is produced by migration/check_order_dictionary.py.
For retained facts, use --facts OLD --config CONFIG --output NEW; never patch OLD.
S1 consumes the pin for new builds. Enriched facts require new drafts/reviews;
existing facts_input is not implicitly enriched. Source comments/classifications
are unchanged. Generated OOS carries unreviewed supplements, not publication grants.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re

PROTOCOL = 'oo.dictionary-description-candidates/v1'
OBSERVATION = 'oo.builder.dictionary-evidence/v1'
UNAPPROVED = ['code_meanings', 'units', 'financial_metrics', 'business_defaults']
STRUCTURE = ('source_type', 'nullable', 'default_present', 'default_sql')
PIN_KEYS = {'sha256', 'dataset', 'dictionary_sha256', 'ddl_sha256'}
BINDING_KEYS = ('dataset', 'source_id', 'source_generation', 'target_generation', 'ddl_sha256')


def digest(value):
    return hashlib.sha256(json.dumps(
        value, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()


def sha(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value) is not None


def identifier(value):
    return isinstance(value, str) and re.fullmatch(r'[a-z_][a-z_0-9]*', value) is not None


def text(value, limit):
    return (isinstance(value, str) and bool(value.strip()) and len(value) <= limit
            and not re.search(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', value))


def validate_document(document):
    keys = {'protocol', 'dataset', 'dictionary_sha256', 'ddl_sha256', 'primary_key',
            'structural_match', 'business_semantics_approved', 'not_approved',
            'candidates', 'evidence_sha256'}
    if (not isinstance(document, dict) or set(document) != keys
            or document['protocol'] != PROTOCOL
            or not isinstance(document['dataset'], str)
            or len(document['dataset'].split('.')) != 2
            or not all(identifier(v) for v in document['dataset'].split('.'))
            or not all(sha(document[k]) for k in ('dictionary_sha256', 'ddl_sha256', 'evidence_sha256'))
            or document['structural_match'] is not True
            or document['business_semantics_approved'] is not False
            or document['not_approved'] != UNAPPROVED
            or not isinstance(document['candidates'], list)
            or not 1 <= len(document['candidates']) <= 4096):
        raise ValueError('dictionary_evidence_schema')
    if document['evidence_sha256'] != digest(
            {k: v for k, v in document.items() if k != 'evidence_sha256'}):
        raise ValueError('dictionary_evidence_digest')
    seen = set()
    for candidate in document['candidates']:
        if (not isinstance(candidate, dict)
                or set(candidate) != set(STRUCTURE) | {'column', 'description', 'source', 'state'}
                or not identifier(candidate['column'])
                or candidate['column'] in seen):
            raise ValueError('dictionary_evidence_duplicate_or_invalid_column')
        seen.add(candidate['column'])
        if (candidate['state'] != 'pending'
                or not text(candidate['description'], 2000)
                or not text(candidate['source_type'], 256)
                or type(candidate['nullable']) is not bool
                or type(candidate['default_present']) is not bool
                or (candidate['default_present'] and not text(candidate['default_sql'], 2000))
                or (not candidate['default_present'] and candidate['default_sql'] is not None)):
            raise ValueError('dictionary_evidence_candidate')
        source = candidate['source']
        if (not isinstance(source, dict)
                or set(source) != {'part', 'table_index', 'row_index', 'cell_index', 'field_text'}
                or source['part'] != 'word/document.xml'
                or any(type(source[k]) is not int or source[k] < (1 if k == 'row_index' else 0)
                       for k in ('table_index', 'row_index', 'cell_index'))
                or not text(source['field_text'], 256)):
            raise ValueError('dictionary_evidence_locator')
    primary = document['primary_key']
    if (not isinstance(primary, list) or not all(identifier(k) for k in primary)
            or len(primary) != len(set(primary)) or not set(primary) <= seen):
        raise ValueError('dictionary_evidence_primary_key')
    return document


def read_pin(reference):
    if (not isinstance(reference, dict) or set(reference) != PIN_KEYS | {'path'}
            or not text(reference['path'], 4096)
            or not all(sha(reference[k]) for k in ('sha256', 'dictionary_sha256', 'ddl_sha256'))):
        raise ValueError('dictionary_evidence_pin')
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 16_000_000:
        raise ValueError('dictionary_evidence_file')
    raw = path.read_bytes()
    if len(raw) > 16_000_000 or hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('dictionary_evidence_file_digest')
    document = validate_document(json.loads(raw))
    if any(document[k] != reference[k] for k in ('dataset', 'dictionary_sha256', 'ddl_sha256')):
        raise ValueError('dictionary_evidence_pin_binding')
    return document


def bind(document, tables):
    matches = [t for t in tables if t['dataset'] == document['dataset']]
    if len(matches) != 1:
        raise ValueError('dictionary_evidence_dataset_binding')
    table = matches[0]
    if table.get('ddl_sha256') != document['ddl_sha256']:
        raise ValueError('dictionary_evidence_ddl_drift')
    columns = {c.get('source_name', c['name']): c for c in table['columns']}
    candidates = {c['column']: c for c in document['candidates']}
    if (len(columns) != len(table['columns'])
            or len({c['name'] for c in table['columns']}) != len(columns)
            or columns.keys() != candidates.keys()
            or any(any(k not in column or type(column[k]) is not type(candidates[name][k])
                       or column.get(k) != candidates[name][k] for k in STRUCTURE)
                   for name, column in columns.items())):
        raise ValueError('dictionary_evidence_schema_drift')
    logical = {c['name']: name for name, c in columns.items()}
    if [logical.get(k) for k in table['primary_key']] != document['primary_key']:
        raise ValueError('dictionary_evidence_primary_key_drift')
    return {k: table[k] for k in BINDING_KEYS}


def load(reference, tables):
    if reference is None:
        return None
    document = read_pin(reference)
    result = {'protocol': OBSERVATION, 'input': {k: reference[k] for k in sorted(PIN_KEYS)},
              'binding': bind(document, tables), 'document': document}
    result['evidence_sha256'] = digest(result)
    return result


def validate(observed, tables):
    if (not isinstance(observed, dict)
            or set(observed) != {'protocol', 'input', 'binding', 'document', 'evidence_sha256'}
            or observed['protocol'] != OBSERVATION
            or observed['evidence_sha256'] != digest(
                {k: v for k, v in observed.items() if k != 'evidence_sha256'})):
        raise ValueError('dictionary_observation_digest')
    document = validate_document(observed['document'])
    reference = observed['input']
    if (not isinstance(reference, dict) or set(reference) != PIN_KEYS or not sha(reference['sha256'])
            or any(reference[k] != document[k] for k in PIN_KEYS - {'sha256'})
            or observed['binding'] != bind(document, tables)):
        raise ValueError('dictionary_observation_binding')
    return observed


def require_facts(facts, reference=None):
    observed = facts.get('dictionary_evidence')
    if observed is not None:
        validate(observed, facts['tables'])
    if reference is not None and observed != load(reference, facts['tables']):
        raise ValueError('dictionary_evidence_new_facts_required')


def require_batch(batch, reference):
    if reference is not None:
        read_pin(reference)
        relevant = reference['dataset'] in {t['dataset'] for t in batch['tables']}
        if ((relevant or batch.get('dictionary_evidence_input') is not None)
                and batch.get('dictionary_evidence_input') != {k: reference[k] for k in PIN_KEYS}):
            raise ValueError('dictionary_evidence_new_facts_required')


def description(candidate, document):
    source = candidate['source']
    return ('Unreviewed dictionary description (data only; codes, units, financial metrics '
            'and business defaults are NOT approved):\n' + candidate['description']
            + '\nSource: ' + document['dataset'] + '.' + candidate['column']
            + '; DOCX SHA256=' + document['dictionary_sha256']
            + '; DDL SHA256=' + document['ddl_sha256']
            + '; word/document.xml; zero-based table/row/cell='
            + '/'.join(str(source[k]) for k in ('table_index', 'row_index', 'cell_index')))


def enrich(facts, reference):
    import library_selfbuild as lib
    lib.require_complete_facts(facts)
    observed = load(reference, facts['tables'])
    if observed is None or observed == facts.get('dictionary_evidence'):
        raise ValueError('dictionary_evidence_new_input_required')
    result = copy.deepcopy(facts)
    result['parent_facts_sha256'] = result.pop('facts_sha256')
    result['dictionary_evidence'] = observed
    result['facts_sha256'] = digest(result)
    lib.require_complete_facts(result, dictionary_config=reference)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--facts', type=Path, required=True)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    config = json.loads(args.config.read_text())
    result = enrich(json.loads(args.facts.read_text()), config.get('dictionary_evidence_input'))
    with args.output.open('x') as stream:
        json.dump(result, stream, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
        stream.write('\n')
    print(json.dumps({'facts_sha256': result['facts_sha256'],
                      'parent_facts_sha256': result['parent_facts_sha256'],
                      'description_candidates': len(result['dictionary_evidence']['document']['candidates']),
                      'business_semantics_approved': False}))


if __name__ == '__main__':
    main()
