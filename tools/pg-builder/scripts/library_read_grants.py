"""F20: intersect reviewed classifications with the existing pinned operator input.

This is a compiler adapter, not an IAM or an HTTP request format. Only the
operator-owned build config supplies column_review_input; ledger fields never do.
"""
from copy import deepcopy
import time

def authorize(config, facts, review):
    selected = review.get('columns', {})
    needs_grant = any(c.get('pii') != 'none' for c in selected.values())
    reference = config.get('column_review_input')
    if reference is None:
        if 'access_profile' in review:
            raise ValueError('access_profile_operator_input_required')
        if needs_grant or review.get('review_origin') == 'operator_pinned_file':
            raise ValueError('pii_read_grant_required')
        return deepcopy(review), None

    # Reuse the local entrypoint's exact-byte pin, file and facts/project checks.
    from library_build import pinned_review
    policy = pinned_review(dict(config, project=review['project']), facts)
    if config.get('project', review['project']) != review['project']:
        raise ValueError('read_grant_scope')
    if any(policy.get(k) != review.get(k) for k in ('project', 'role', 'principal')):
        raise ValueError('read_grant_scope')
    inventory = {t['dataset'] + '.' + c['name']
                 for t in facts['tables'] for c in t['columns']}
    grants = policy.get('columns')
    if not isinstance(grants, dict) or not set(grants) <= inventory or not set(selected) <= inventory:
        raise ValueError('read_grant_scope')
    hidden = policy.get('hidden_columns')
    if (not isinstance(hidden, list) or any(not isinstance(c, str) for c in hidden)
            or not set(hidden) <= inventory):
        raise ValueError('read_grant_scope')
    hidden = set(hidden) | set(review.get('hidden_columns', []))
    if (set(selected) | set(grants)) & hidden:
        raise ValueError('read_grant_hidden')
    if review.get('review_origin') == 'operator_pinned_file' and review != policy:
        raise ValueError('column_review_digest')
    from library_access_profile import validate_profile
    validate_profile(facts, policy, review)

    projected = deepcopy(review)
    if 'access_profile' in policy:
        projected['access_profile'] = deepcopy(policy['access_profile'])
    used = []
    now = time.time()
    for identity, classification in selected.items():
        grant = grants.get(identity)
        if not isinstance(grant, dict):
            raise ValueError('read_grant_scope')
        if any(grant.get(k) != classification.get(k) for k in ('pii', 'semantic_type')):
            raise ValueError('read_grant_classification')
        # A classification decision is deliberately not a read authorization.
        for key in ('explicit_allow', 'reason', 'read_grant_expires_at', 'read_grant_reviewer'):
            projected['columns'][identity].pop(key, None)
        pii = classification.get('pii')
        allow = grant.get('explicit_allow', False)
        if type(allow) is not bool or allow != (pii != 'none'):
            raise ValueError('pii_read_grant_required')
        if pii == 'none':
            continue
        if not isinstance(grant.get('reviewer'), str) or not grant['reviewer'].strip():
            raise ValueError('read_grant_reviewer')
        grant_reviewer = grant.get('read_grant_reviewer', grant['reviewer'])
        if not isinstance(grant_reviewer, str) or not grant_reviewer.strip():
            raise ValueError('read_grant_reviewer')
        expires = grant.get('read_grant_expires_at')
        if type(expires) is not int or expires <= now:
            raise ValueError('read_grant_expired')
        projected['columns'][identity]['explicit_allow'] = True
        projected['columns'][identity]['read_grant_expires_at'] = expires
        if 'read_grant_reviewer' in grant:
            projected['columns'][identity]['read_grant_reviewer'] = grant_reviewer
        if 'reason' in grant:
            projected['columns'][identity]['reason'] = grant['reason']
        used.append({'column': identity, 'authorized_by': grant_reviewer,
                     'expires_at': expires})
    return projected, {
        'origin': 'operator_pinned_file', 'input_sha256': reference['sha256'],
        'facts_sha256': facts['facts_sha256'],
        'scope': {k: policy[k] for k in ('project', 'role', 'principal')},
        'grants': used,
    }
