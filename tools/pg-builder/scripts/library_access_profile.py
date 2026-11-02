"""Offline operator access profiles over the existing exact-byte column review pin.

No SQL, model, JWT, publication or ACL operations. Installing the generated
config remains an operator action, not a customer request or runtime bypass.
"""
import argparse
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import re
import time

import library_assets
import library_selfbuild as lib

PROFILES = ('full', 'restricted')
PII = ('none', 'personal', 'sensitive', 'restricted', 'unknown')


def inventory(facts):
    columns, system = {}, set()
    for table in facts['tables']:
        dataset = table['dataset']
        if not isinstance(dataset, str) or dataset.count('.') != 1:
            raise ValueError('access_profile_inventory')
        schema, name = dataset.lower().split('.')
        reserved = (schema == 'information_schema' or schema.startswith(('pg_', '_oo'))
                    or schema.endswith('__stage') or name.startswith('_oo'))
        for column in table['columns']:
            identity = dataset + '.' + column['name']
            if identity in columns or column['classification_candidate']['pii'] not in PII:
                raise ValueError('access_profile_inventory')
            columns[identity] = column
            names = [column['name'].lower(), column.get('source_name', column['name']).lower()]
            if reserved or any(n.startswith('_oo') or n in {
                    'tableoid', 'ctid', 'xmin', 'xmax', 'cmin', 'cmax'} for n in names):
                system.add(identity)
    return columns, system


def column_set(value, columns):
    if (not isinstance(value, list) or any(not isinstance(c, str) for c in value)
            or len(value) != len(set(value)) or not set(value) <= columns.keys()):
        raise ValueError('access_profile_inventory')
    return set(value)


def validate_profile(facts, policy, review):
    """Called by authorize: only the pinned policy may select a profile."""
    profile = policy.get('access_profile')
    if profile is None:
        if 'access_profile' in review:
            raise ValueError('access_profile_operator_input_required')
        return
    if (not isinstance(profile, dict) or set(profile) != {'name', 'protected_columns', 'withheld_reviews'}
            or profile['name'] not in PROFILES):
        raise ValueError('access_profile_invalid')
    if 'access_profile' in review and review['access_profile'] != profile:
        raise ValueError('access_profile_operator_input_required')
    columns, system = inventory(facts)
    protected = column_set(profile['protected_columns'], columns)
    if not system <= protected:
        raise ValueError('access_profile_system_isolation')
    expected = {name for name, column in columns.items() if name not in protected
                and (profile['name'] == 'full' or column['classification_candidate']['pii'] == 'none')}
    if (set(policy['columns']) != expected or set(review['columns']) != expected
            or column_set(policy['hidden_columns'], columns) != columns.keys() - expected
            or column_set(review['hidden_columns'], columns) != columns.keys() - expected):
        raise ValueError('access_profile_coverage')
    withheld = profile['withheld_reviews']
    if (not isinstance(withheld, dict) or not set(withheld) <= columns.keys() - protected - expected
            or any(not isinstance(v, dict) for v in withheld.values())):
        raise ValueError('access_profile_coverage')


def load_inputs(config_path, facts_path, drafts_path):
    config = json.loads(Path(config_path).read_text())
    facts = json.loads(Path(facts_path).read_text())
    drafts = json.loads(Path(drafts_path).read_text())
    lib.require_complete_facts(
        facts, declared_enum_config=config.get('declared_enum_adapter'),
        report_config=config.get('report_definition_adapter'),
        dictionary_config=config.get('dictionary_evidence_input'))
    if drafts.get('failures'):
        raise ValueError('access_profile_drafts_failed')
    library_assets.require_draft_coverage(facts, drafts['receipts'])
    return config, facts, drafts['receipts']


def check(config, facts, receipts):
    from library_build import pinned_review
    import library_control
    import library_read_grants

    review = pinned_review(config, facts)
    projected, authorization = library_read_grants.authorize(config, facts, review)
    effective, visibility, _ = library_control.reviewed_documents(
        facts, receipts, projected, config.get('knowledge_budgets'),
        config.get('include_candidate_metrics', False))
    library_assets.bind_release_versions(effective, visibility, config.get('release_versions'))
    return coverage_report(facts, projected, visibility, authorization)


def coverage_report(facts, review, visibility, authorization):
    columns, system = inventory(facts)
    visible = {t['dataset'] + '.' + c for t in visibility['datasets'] for c in t['columns']}
    if visible & system:
        raise ValueError('access_profile_system_isolation')
    profile = review.get('access_profile')
    protected = column_set(profile['protected_columns'] if profile else review['hidden_columns'], columns) | system
    hidden = columns.keys() - visible
    counts = {}
    for pii in PII:
        members = {name for name, col in columns.items() if col['classification_candidate']['pii'] == pii}
        counts[pii] = {'total': len(members), 'visible': len(members & visible),
                       'hidden': len(members - visible)}
    return {
        'protocol': 'oo.library.access-profile-report/v1',
        'profile': profile['name'] if profile else 'reviewed',
        'scope': {k: review[k] for k in ('project', 'role', 'principal')},
        'facts_sha256': facts['facts_sha256'],
        'column_review_input_sha256': authorization['input_sha256'],
        'visibility_sha256': lib.digest(visibility),
        'read_grants_valid_until': min((g['expires_at'] for g in authorization['grants']), default=None),
        'counts': {
            'tables_total': len(facts['tables']), 'tables_visible': len(visibility['datasets']),
            'columns_total': len(columns), 'columns_visible': len(visible), 'columns_hidden': len(hidden),
            'system_columns': len(system), 'protected_columns': len(protected),
            'explicit_pii_grants': len(authorization['grants']),
            'visible_sensitive_including_unknown': counts['sensitive']['visible'] + counts['unknown']['visible'],
        },
        'source_classification': counts,
        'coverage': {
            'all_known_columns_visible': visible == columns.keys(),
            'all_business_columns_visible': visible == columns.keys() - system,
            'all_eligible_columns_visible': visible == columns.keys() - protected,
        },
        'hidden_columns': [
            {'column': name, 'reason': 'system' if name in system else
             'operator_protected' if name in protected else
             'restricted_profile' if profile and profile['name'] == 'restricted' else 'not_reviewed'}
            for name in sorted(hidden)],
        'published': False, 'runtime_verified': False, 'business_owner_signed': False,
    }


def configure(config_path, facts_path, drafts_path, profile, reviewer, expires_at, output):
    from library_build import pinned_review
    import library_control

    if profile not in PROFILES:
        raise ValueError('access_profile_invalid')
    if not isinstance(reviewer, str) or not reviewer.strip():
        raise ValueError('read_grant_reviewer')
    if type(expires_at) is not int or expires_at <= time.time():
        raise ValueError('read_grant_expired')
    config, facts, receipts = load_inputs(config_path, facts_path, drafts_path)
    review = pinned_review(config, facts)
    if any(not isinstance(review.get(k), str) or not review[k].strip()
           for k in ('project', 'role', 'principal')):
        raise ValueError('read_grant_scope')
    validate_profile(facts, review, review)
    columns, system = inventory(facts)
    existing = review.get('access_profile')
    protected = column_set(existing['protected_columns'] if existing else review['hidden_columns'], columns) | system
    if not isinstance(review.get('columns'), dict) or not set(review['columns']) <= columns.keys():
        raise ValueError('read_grant_scope')
    selected = {name for name, column in columns.items() if name not in protected
                and (profile == 'full' or column['classification_candidate']['pii'] == 'none')}
    prior = dict(review['columns'], **(existing['withheld_reviews'] if existing else {}))
    decisions = {}
    for identity in sorted(selected):
        classification = columns[identity]['classification_candidate']
        pii, semantic = classification['pii'], classification['semantic_type']
        expected = dict(pii='sensitive' if pii == 'unknown' else pii,
                        semantic_type='quantity' if semantic == 'money' else semantic)
        decision = deepcopy(prior.get(identity, dict(expected, reviewer=reviewer, label_approved=False)))
        if not isinstance(decision, dict) or any(decision.get(k) != v for k, v in expected.items()):
            raise ValueError('read_grant_classification')
        # Keep classification/label attribution; the new read grant has its own operator.
        for key in ('explicit_allow', 'read_grant_expires_at', 'read_grant_reviewer', 'reason'):
            decision.pop(key, None)
        if pii != 'none':
            decision.update(explicit_allow=True, read_grant_expires_at=expires_at, read_grant_reviewer=reviewer)
        if pii == 'unknown':
            decision['reason'] = 'Operator full read scope; unknown conservatively retained as sensitive, not none.'
        decisions[identity] = decision
    # Retain semantic reviews for reversibility, but never count them as read grants.
    withheld = {name: {k: v for k, v in decision.items()
                       if k not in {'explicit_allow', 'read_grant_expires_at', 'read_grant_reviewer', 'reason'}}
                for name, decision in prior.items() if name not in selected | protected}
    review.update(columns=decisions, hidden_columns=sorted(columns.keys() - selected),
                  access_profile={'name': profile, 'protected_columns': sorted(protected),
                                  'withheld_reviews': withheld},
                  business_owner_signed=False)
    review.pop('review_origin', None)
    document = {'protocol': 'oo.library.column-review/v1', 'project': review['project'],
                'facts_sha256': facts['facts_sha256'], 'review': review}
    output = Path(output).resolve()
    if any((parent / '.git').exists() for parent in (output, *output.parents)):
        raise ValueError('access_profile_output_must_be_outside_git')
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    review_path = output / 'column-review.json'
    digest = library_control.save(review_path, document)
    new_config = dict(config, column_review_input={'path': str(review_path), 'sha256': digest})
    report = check(new_config, facts, receipts)
    report['input_files_sha256'] = {
        name: hashlib.sha256(Path(path).read_bytes()).hexdigest()
        for name, path in [('config', config_path), ('facts', facts_path), ('drafts', drafts_path)]}
    library_control.save(output / 'config.json', new_config)
    library_control.save(output / 'config-patch.json', {'column_review_input': new_config['column_review_input']})
    library_control.save(output / 'report.json', report)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(
        description='Library control: no arguments reads the existing stage request from stdin.',
        epilog='Access profiles are operator-local configuration, never customer/JWT flags.')
    commands = parser.add_subparsers(dest='command', required=True)
    access = commands.add_parser('access-profile', help='configure/check pinned operator read access offline')
    actions = access.add_subparsers(dest='action', required=True)
    for action in ('configure', 'check'):
        sub = actions.add_parser(action, help='create a new config/review/report' if action == 'configure'
                                 else 'verify exact grants and report all/sensitive/hidden column coverage')
        sub.add_argument('--config', required=True, help='operator-owned build JSON with column_review_input pin')
        sub.add_argument('--facts', required=True, help='retained facts.json; digest and classifications are checked')
        sub.add_argument('--drafts', required=True, help='retained drafts.json; no model calls')
        if action == 'configure':
            sub.add_argument('--profile', choices=PROFILES, required=True,
                             help='full: all eligible known columns incl. PII; restricted: only source pii=none')
            sub.add_argument('--reviewer', required=True, help='operator attribution, not business approval')
            sub.add_argument('--expires-at', type=int, required=True, help='future Unix seconds for explicit PII grants')
            sub.add_argument('--output', required=True, help='NEW private directory outside Git; never overwrites inputs')
        sub.epilog = ('Protected hidden/system columns remain isolated. Unknown is sensitive in full, hidden in restricted. '
                      'Configure writes config.json + column-review.json + config-patch.json + report.json, but installs nothing. '
                      'Use the generated config in the normal library_build.py/compile path after operator verification; '
                      'existing builds are immutable, use a new work directory. JWT, read-only, tenant/role and PG ACL gates stay unchanged.')
    args = parser.parse_args(argv)
    os.umask(0o077)
    try:
        if args.action == 'configure':
            result = configure(args.config, args.facts, args.drafts, args.profile,
                               args.reviewer, args.expires_at, args.output)
        else:
            result = check(*load_inputs(args.config, args.facts, args.drafts))
        print(lib.canonical(result))
    except (ValueError, OSError, KeyError, TypeError) as error:
        code = str(error)
        print(lib.canonical({'error': type(error).__name__,
                             'reason': code if re.fullmatch(r'[a-z][a-z_]{0,99}', code) else type(error).__name__,
                             'published': False, 'runtime_verified': False}))
        raise SystemExit(2) from None
