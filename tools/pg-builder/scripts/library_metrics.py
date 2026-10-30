"""Metric candidates and explicitly pinned operator reviews; no implicit approval."""
import copy
import hashlib
import json
from pathlib import Path
import re

DRAFT_CONTRACT = 'oo.library.semantic-draft/v3'
MAX_METRICS = 4096
FIELDS = {'metric', 'label', 'synonyms', 'description', 'unit', 'grain',
          'time_dimension', 'enum_filters', 'sql', 'dimensions', 'caveats'}
CERTIFICATION = {'owner', 'certified', 'status', 'default_anchor', 'tests', 'control_total'}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False,
                                    separators=(',', ':')).encode()).hexdigest()


def identifier(value):
    return isinstance(value, str) and re.fullmatch(r'[a-z][a-z0-9_]{0,62}', value) is not None


def text(value, limit):
    return (isinstance(value, str) and bool(value.strip()) and len(value) <= limit
            and not re.search(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', value))


def current_draft(receipt):
    return (receipt.get('draft_contract') == DRAFT_CONTRACT
            and isinstance(receipt.get('draft'), dict)
            and all(isinstance(receipt['draft'].get(k), list)
                    for k in ('datasets', 'metrics', 'glossary', 'examples')))


def candidates(tables, drafts):
    inventory = {t['dataset']: {c['name'] for c in t['columns']} for t in tables}
    found = {}
    for draft in drafts:
        items = draft.get('metrics', [])
        if not isinstance(items, list) or len(items) > MAX_METRICS:
            raise ValueError('s5_metric_budget')
        for item in items:
            if not isinstance(item, dict) or set(item) - (FIELDS | {'dataset'}):
                raise ValueError('s5_metric_fields')
            item = copy.deepcopy(item)
            # An empty optional hint asserts no unit/time axis; never invent one.
            for optional in ('unit', 'time_dimension'):
                if optional in item and (item[optional] is None or item[optional] == ''):
                    del item[optional]
            dataset = item.get('dataset')
            if not isinstance(dataset, str) or dataset not in inventory:
                raise ValueError('s5_metric_dataset_binding')
            name = item.get('metric')
            if not identifier(name):
                raise ValueError('s5_metric_identifier')
            for field, limit in [('label', 256), ('sql', 16000)]:
                if not text(item.get(field), limit):
                    raise ValueError('s5_metric_' + field + '_invalid')
            for field, limit in [('description', 4000), ('unit', 256), ('time_dimension', 512)]:
                if field in item and not text(item[field], limit):
                    raise ValueError('s5_metric_' + field + '_invalid')
            for field in ('synonyms', 'grain', 'dimensions', 'caveats'):
                if field in item and (not isinstance(item[field], list) or len(item[field]) > 128
                                      or any(not text(v, 2000 if field == 'caveats' else 256)
                                             for v in item[field])):
                    raise ValueError('s5_metric_' + field + '_invalid')
            if any(not identifier(name) for name in item.get('dimensions', [])):
                raise ValueError('s5_metric_dimensions_invalid')
            if 'time_dimension' in item:
                # A local column reference is unambiguous only inside its explicit dataset.
                if item['time_dimension'] in inventory[dataset]:
                    item['time_dimension'] = dataset + '.' + item['time_dimension']
                path = item['time_dimension'].rsplit('.', 1)
                if len(path) != 2 or path[0] not in inventory or path[1] not in inventory[path[0]]:
                    raise ValueError('s5_metric_time_binding')
            filters = item.get('enum_filters', [])
            if not isinstance(filters, list) or len(filters) > 128:
                raise ValueError('s5_metric_enum_binding')
            for enum in filters:
                if (not isinstance(enum, dict) or set(enum) != {'column', 'enum', 'classes'}
                        or not isinstance(enum['classes'], list) or not enum['classes']
                        or any(not isinstance(c, str) or not re.fullmatch(r'[a-z][a-z0-9_]{0,63}', c)
                               for c in enum['classes'])):
                    raise ValueError('s5_metric_enum_binding')
                for key in ('column', 'enum'):
                    path = enum[key].rsplit('.', 1) if isinstance(enum[key], str) else []
                    if len(path) != 2 or path[0] not in inventory or path[1] not in inventory[path[0]]:
                        raise ValueError('s5_metric_enum_binding')
            definition = {k: copy.deepcopy(v) for k, v in item.items() if k != 'dataset'}
            definition.update(certified=False, status='draft')
            bound = {'dataset': dataset, 'definition': definition}
            candidate = dict(bound, candidate_sha256=digest(bound))
            if name in found and found[name] != candidate:
                raise ValueError('s5_metric_conflict')
            found[name] = candidate
            if len(found) > MAX_METRICS:
                raise ValueError('s5_metric_budget')
    return [found[name] for name in sorted(found)]


def load_reviews(config, facts, project):
    """Only an operator-installed, hash-pinned input can supply a review."""
    return load_bound_input(config, facts, project, 'metric_review_input',
                            'oo.library.metric-review/v1', 'reviews')

def load_amendments(config, facts, project):
    return load_bound_input(config, facts, project, 'metric_amendment_input',
                            'oo.library.metric-amendment/v1', 'amendments')

def load_bound_input(config, facts, project, key, protocol, entries):
    reference = config.get(key)
    if reference is None:
        return {}
    if (not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}
            or not isinstance(reference['path'], str)):
        raise ValueError('metric_review_reference')
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8_388_608:
        raise ValueError('metric_review_file')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('metric_review_digest')
    value = json.loads(raw)
    if (not isinstance(value, dict)
            or set(value) != {'protocol', 'project', 'facts_sha256', entries}
            or value['protocol'] != protocol
            or value['project'] != project or value['facts_sha256'] != facts['facts_sha256']
            or not isinstance(value[entries], dict)):
        raise ValueError('metric_review_binding')
    return value[entries]

def amend_candidates(tables, items, amendments):
    """Corrections create new unapproved candidates; they never inherit certification."""
    names = {item['definition']['metric'] for item in items}
    if not isinstance(amendments, dict) or set(amendments) - names:
        raise ValueError('metric_amendment_unknown')
    result, audit = [], []
    for item in items:
        name = item['definition']['metric']
        amendment = amendments.get(name)
        if amendment is None:
            result.append(copy.deepcopy(item))
            continue
        if (not isinstance(amendment, dict) or set(amendment) != {
                'base_candidate_sha256', 'definition', 'by', 'reason', 'evidence_sha256'}
                or amendment['base_candidate_sha256'] != item['candidate_sha256']
                or not text(amendment['by'], 256) or not text(amendment['reason'], 2000)
                or not isinstance(amendment['evidence_sha256'], str)
                or not re.fullmatch(r'[0-9a-f]{64}', amendment['evidence_sha256'])
                or not isinstance(amendment['definition'], dict)
                or amendment['definition'].get('metric') != name):
            raise ValueError('metric_amendment_stale')
        proposed = dict(amendment['definition'], dataset=item['dataset'])
        updated = candidates(tables, [{'metrics': [proposed]}])[0]
        if updated['candidate_sha256'] == item['candidate_sha256']:
            raise ValueError('metric_amendment_no_change')
        result.append(updated)
        audit.append({'metric': name, 'from_candidate_sha256': item['candidate_sha256'],
                      'candidate_sha256': updated['candidate_sha256'], 'by': amendment['by'],
                      'reason': amendment['reason'], 'evidence_sha256': amendment['evidence_sha256'],
                      'certified': False})
    return result, audit


def project_metrics(items, effective, reviews):
    if not isinstance(reviews, dict) or set(reviews) - {c['definition']['metric'] for c in items}:
        raise ValueError('metric_review_unknown')
    definitions = {m['metric']: m for m in effective['metrics']}
    projection = []
    for candidate in items:
        name = candidate['definition']['metric']
        decision = reviews.get(name)
        state = 'pending'
        if decision is not None:
            if (not isinstance(decision, dict)
                    or set(decision) != {'candidate_sha256', 'reviewer', 'definition'}
                    or decision['candidate_sha256'] != candidate['candidate_sha256']
                    or not text(decision['reviewer'], 256)):
                raise ValueError('metric_review_stale')
            approved = decision['definition']
            if (not isinstance(approved, dict) or set(approved) - (FIELDS | CERTIFICATION)
                    or approved.get('owner') != decision['reviewer']
                    or approved.get('certified') is not True or approved.get('status') != 'active'
                    or not approved.get('default_anchor') or not approved.get('tests')
                    or not approved.get('control_total')):
                raise ValueError('metric_review_incomplete')
            baseline = {k: v for k, v in candidate['definition'].items() if k not in CERTIFICATION}
            if {k: v for k, v in approved.items() if k not in CERTIFICATION} != baseline:
                raise ValueError('metric_review_definition_drift')
            definitions[name].clear()
            definitions[name].update(copy.deepcopy(approved))
            state = 'projected'
        projection.append({'metric': name, 'dataset': candidate['dataset'], 'state': state,
                           'candidate_sha256': candidate['candidate_sha256']})
    return projection


def requirements(config):
    value = config.get('required_metrics', [])
    if (not isinstance(value, list) or len(value) > MAX_METRICS
            or any(not identifier(name) for name in value) or len(value) != len(set(value))):
        raise ValueError('required_metrics_invalid')
    return value


def require_pack_metrics(artifact, required):
    files = {item['path']: item['text'] for item in artifact['files']}
    if not required:
        return
    try:
        status = json.loads(files['metrics/availability.json'])
        if status['protocol'] != 'oo.knowledge.metric-availability/v1':
            raise ValueError
        available = status['certified_metrics']
        if not isinstance(available, list) or any(not identifier(name) for name in available):
            raise ValueError
    except (KeyError, TypeError, ValueError):
        raise ValueError('metric_availability_missing') from None
    if not set(required) <= set(available):
        raise ValueError('required_metrics_unavailable')
