"""Bound semantic candidates and operator review inputs, never inferred approval."""
import copy
import hashlib
import json
from pathlib import Path
import re

import dictionary_evidence
from library_metrics import digest, identifier, text

DRAFT_CONTRACT = 'oo.library.semantic-draft/v3'
EXAMPLE_FIELDS = {'id', 'question', 'sql', 'uses'}

class ExampleDependencyError(ValueError):
    def __init__(self, dataset):
        super().__init__('s5_example_dependencies')
        self.feedback = {'code': 's5_example_dependencies', 'dataset': dataset,
                         'required_use': 'dataset:' + dataset}

def validate_glossary(tables, drafts):
    inventory = {t['dataset'] for t in tables}
    count = 0
    for draft in drafts:
        terms = draft.get('glossary', [])
        if not isinstance(terms, list):
            raise ValueError('s5_glossary_schema')
        count += len(terms)
        if count > 4096:
            raise ValueError('s5_glossary_budget')
        for term in terms:
            if (not isinstance(term, dict) or set(term) != {'term', 'definition', 'dataset'}
                    or not text(term['term'], 256) or not text(term['definition'], 2000)
                    or not isinstance(term['dataset'], str) or term['dataset'] not in inventory):
                raise ValueError('s5_glossary_definition')


def example_candidates(tables, drafts):
    inventory = {t['dataset'] for t in tables}
    result = {}
    for draft in drafts:
        examples = draft.get('examples', [])
        if not isinstance(examples, list) or len(examples) > 4096:
            raise ValueError('s5_example_budget')
        for item in examples:
            if not isinstance(item, dict) or set(item) != EXAMPLE_FIELDS | {'dataset'}:
                raise ValueError('s5_example_fields')
            if (item['dataset'] not in inventory or not isinstance(item['id'], str)
                    or not re.fullmatch(r'[a-z][a-z0-9-]{1,63}', item['id'])):
                raise ValueError('s5_example_binding')
            if not text(item['question'], 2000) or not text(item['sql'], 16000):
                raise ValueError('s5_example_text')
            uses = item['uses']
            if (not isinstance(uses, list) or len(uses) > 128
                    or any(not text(v, 256) for v in uses)
                    or 'dataset:' + item['dataset'] not in uses):
                raise ExampleDependencyError(item['dataset'])
            definition = {k: copy.deepcopy(item[k]) for k in sorted(EXAMPLE_FIELDS)}
            bound = {'dataset': item['dataset'], 'definition': definition}
            candidate = dict(bound, candidate_sha256=digest(bound))
            if item['id'] in result and result[item['id']] != candidate:
                raise ValueError('s5_example_conflict')
            result[item['id']] = candidate
            if len(result) > 4096:
                raise ValueError('s5_example_budget')
    return [result[k] for k in sorted(result)]


def require_draft_coverage(facts, receipts):
    """A successful subset of batches must never become a successful library."""
    expected = {t['dataset']: {c['name'] for c in t['columns']} for t in facts['tables']}
    seen = set()
    for receipt in receipts:
        if receipt.get('facts_sha256') != facts['facts_sha256']:
            raise ValueError('semantic_draft_facts_binding')
        draft = receipt.get('draft', {})
        if (receipt.get('draft_contract') != DRAFT_CONTRACT
                or any(not isinstance(draft.get(k), list)
                       for k in ('datasets', 'metrics', 'glossary', 'examples'))):
            raise ValueError('semantic_draft_contract')
        for table in draft['datasets']:
            name = table.get('dataset')
            columns = [c['name'] for c in table.get('columns', [])]
            if (name not in expected or name in seen or len(columns) != len(set(columns))
                    or set(columns) != expected[name]):
                raise ValueError('semantic_draft_coverage')
            seen.add(name)
    if seen != set(expected):
        raise ValueError('semantic_draft_coverage')


def load_reviews(config, facts, project):
    dictionary_evidence.require_facts(facts, config.get('dictionary_evidence_input'))
    reference = config.get('semantic_review_input')
    if reference is None:
        return {}
    if (not isinstance(reference, dict) or set(reference) != {'path', 'sha256'}
            or not isinstance(reference['path'], str)):
        raise ValueError('semantic_review_reference')
    path = Path(reference['path'])
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 32_000_000:
        raise ValueError('semantic_review_file')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != reference['sha256']:
        raise ValueError('semantic_review_digest')
    value = json.loads(raw)
    if (not isinstance(value, dict)
            or set(value) != {'protocol', 'project', 'facts_sha256', 'reviews'}
            or value['protocol'] != 'oo.library.semantic-review/v1'
            or value['project'] != project or value['facts_sha256'] != facts['facts_sha256']
            or not isinstance(value['reviews'], dict)
            or set(value['reviews']) - {'enums', 'examples', 'glossary'}):
        raise ValueError('semantic_review_binding')
    return value['reviews']


def project(facts, drafts, effective, reviews):
    """The pinned review can enrich evidence, but cannot alter candidate SQL."""
    from decimal import Decimal
    if not isinstance(reviews, dict) or set(reviews) - {'enums', 'examples', 'glossary'}:
        raise ValueError('semantic_review_kind')
    candidates = {
        'enums': {e['enum']: e for e in facts['enum_candidates']},
        'examples': {e['definition']['id']: e for e in example_candidates(facts['tables'], drafts)},
        'glossary': {t['term']: t for t in effective['glossary']},
    }
    audit = {}
    for kind, available in candidates.items():
        decisions = reviews.get(kind, {})
        if not isinstance(decisions, dict) or set(decisions) - set(available):
            raise ValueError('semantic_review_unknown')
        audit[kind] = []
        for name, candidate in sorted(available.items()):
            sha = candidate['candidate_sha256'] if kind == 'examples' else digest(candidate)
            decision = decisions.get(name)
            state = 'pending'
            if decision is not None:
                if (not isinstance(decision, dict)
                        or set(decision) != {'candidate_sha256', 'reviewer', 'definition', 'evidence_sha256'}
                        or decision['candidate_sha256'] != sha
                        or not text(decision['reviewer'], 256)
                        or not isinstance(decision['evidence_sha256'], str)
                        or not re.fullmatch(r'[0-9a-f]{64}', decision['evidence_sha256'])
                        or not isinstance(decision['definition'], dict)):
                    raise ValueError('semantic_review_stale')
                definition = copy.deepcopy(decision['definition'])
                reviewer = decision['reviewer']
                if kind == 'examples':
                    if ({k: definition.get(k) for k in EXAMPLE_FIELDS} != candidate['definition']
                            or set(definition) != EXAMPLE_FIELDS | {'verified_by', 'verified_at', 'last_checked'}
                            or definition['verified_by'] != reviewer
                            or not isinstance(definition['last_checked'], dict)
                            or definition['last_checked'].get('status') != 'ok'
                            or definition['last_checked'].get('result_hash') != decision['evidence_sha256']):
                        raise ValueError('example_review_incomplete')
                    effective['examples'].append(definition)
                elif kind == 'glossary':
                    if (definition != dict(candidate, status='approved', owner=reviewer)):
                        raise ValueError('glossary_review_definition_drift')
                    target = next(t for t in effective['glossary'] if t['term'] == name)
                    target.clear()
                    target.update(definition)
                else:
                    baseline = next(e for e in effective['enums'] if e['enum'] == name)
                    observed = {str(v['code']): v for v in candidate.get('values', [])}
                    values = definition.get('values', [])
                    codes = [v.get('code') for v in values]
                    if (definition.get('enum') != name or definition.get('owner') != reviewer
                            or len(codes) != len(set(codes))
                            or set(codes) != {v['code'] for v in baseline['values']}):
                        raise ValueError('enum_review_coverage')
                    unknown, approved = [], 0
                    for value in values:
                        code = value['code']
                        if value.get('status') == 'approved':
                            if (not text(value.get('label'), 256)
                                    or value.get('review') != {'state': 'approved', 'by': reviewer}
                                    or value.get('source') not in {'dictionary_table', 'human'}
                                    or value.get('class') not in definition.get('classes', {})):
                                raise ValueError('enum_review_incomplete')
                            approved += 1
                        else:
                            original = next(v for v in baseline['values'] if v['code'] == code)
                            if value != original:
                                raise ValueError('enum_unapproved_change')
                            unknown.append(code)
                        if value.get('evidence', {}).get('rows') != observed[code]['rows']:
                            raise ValueError('enum_review_evidence_drift')
                    coverage = Decimal(approved) / len(values) if values else Decimal(0)
                    if (set(definition.get('unexplained_values', [])) != set(unknown)
                            or abs(Decimal(str(definition.get('coverage', -1))) - coverage) > Decimal('0.000001')):
                        raise ValueError('enum_review_coverage')
                    baseline.clear()
                    baseline.update(definition)
                state = 'projected'
            audit[kind].append({'name': name, 'state': state, 'candidate_sha256': sha})
    return audit


def requirements(config):
    value = config.get('required_assets', {})
    allowed = {'datasets', 'relationships', 'enums', 'entities', 'metrics', 'glossary', 'examples'}
    if (not isinstance(value, dict) or set(value) - allowed
            or any(not isinstance(v, list) or not v or any(not text(n, 256) for n in v)
                   or len(v) != len(set(v)) for v in value.values())):
        raise ValueError('required_assets_invalid')
    return value


def require_pack_assets(artifact, required):
    if not required:
        return
    files = {f['path']: f['text'] for f in artifact['files']}
    try:
        inventory = json.loads(files['availability.json'])
        if inventory['protocol'] != 'oo.knowledge.asset-availability/v1':
            raise ValueError
        for kind, names in required.items():
            if not set(names) <= set(inventory['available'][kind]):
                raise ValueError
    except (KeyError, TypeError, ValueError):
        raise ValueError('required_assets_unavailable') from None

def bind_release_versions(effective, visibility, versions):
    if versions is None:
        return
    if (not isinstance(versions, dict) or set(versions) != {'ontology', 'catalog', 'policy', 'knowledge'}
            or any(not isinstance(v, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,62}', v)
                   for v in versions.values())):
        raise ValueError('release_versions_invalid')
    for document in (effective, visibility):
        document['ontology_version'] = versions['ontology']
        document['catalog_version'] = versions['catalog']
        document['policy'] = dict(document['policy'], version=versions['policy'])
        document['knowledge'] = dict(document['knowledge'], version=versions['knowledge'])
