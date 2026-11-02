"""Validate examiner-supplied business evidence; never infer approval from a query."""
import hashlib
import json
import re


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                     separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def sha(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value) is not None


def errors(question, final, trace):
    contract = question.get('business_contract')
    if not isinstance(contract, dict):
        return ['business_contract_and_oracle_required']
    failures = []
    approval = contract.get('approval', {})
    context = contract.get('context', {})
    oracle = contract.get('oracle', {})
    if not all(isinstance(value, dict) for value in (approval, context, oracle)):
        return ['business_contract_invalid']
    if (approval.get('status') != 'approved'
            or not isinstance(approval.get('reviewer'), str) or not approval['reviewer'].strip()
            or not sha(approval.get('definition_sha256'))):
        failures.append('business_definition_unapproved')
    required = {'metric', 'scope', 'period', 'unit', 'time_basis', 'coverage'}
    if set(context) != required or any(not value for value in context.values()):
        failures.append('business_context_incomplete')
    if final.get('business_context') != context:
        failures.append('business_context_mismatch')
    # Local arithmetic can be reconciled independently without claiming source completeness.
    # These hashes, like report evidence above, are trusted examiner inputs, not authentication.
    independent = oracle.get('kind') in {'source_report', 'finance_report'}
    if oracle.get('kind') == 'local_snapshot_reconciliation':
        snapshot = oracle.get('local_snapshot', {})
        independent = (
            isinstance(snapshot, dict)
            and context.get('coverage') == 'local_imported_snapshot_only'
            and all(sha(snapshot.get(k)) for k in (
                'import_manifest_sha256', 'oracle_execution_sha256',
                'definition_query_sha256', 'independent_query_sha256'))
            and snapshot.get('definition_query_sha256') != snapshot.get('independent_query_sha256')
            and snapshot.get('source_completeness_claimed') is False
            and snapshot.get('same_snapshot_verified') is True
        )
    if (not independent
            or oracle.get('reconciled') is not True
            or not sha(oracle.get('evidence_sha256'))
            or oracle.get('context_sha256') != digest(context)
            or 'expected_rows' not in question
            or oracle.get('rows_sha256') != digest(question.get('expected_rows'))):
        failures.append('business_independent_oracle_required')
    files = contract.get('knowledge_files')
    if not isinstance(files, dict) or not files or not all(
            isinstance(path, str) and path.endswith('.md') and sha(value)
            for path, value in files.items()):
        failures.append('business_pack_binding_required')
    else:
        observed = {}
        for call in trace:
            output = call.get('output', {})
            if (call.get('operation') == 'knowledge_cat' and not call.get('error')
                    and isinstance(output, dict) and not output.get('error') and not output.get('code')
                    and isinstance(output.get('text'), str)):
                actual = hashlib.sha256(output['text'].encode()).hexdigest()
                path = output.get('path')
                if isinstance(path, str):
                    observed.setdefault(path, set()).add(actual)
                    if path in files and output.get('sha256') != actual:
                        failures.append('business_pack_binding_mismatch')
        if any(observed.get(path) != {expected} for path, expected in files.items()):
            failures.append('business_pack_binding_mismatch')
    if not any(call.get('operation') == 'freshness' and not call.get('error')
               and isinstance(call.get('output'), dict)
               and call['output'].get('publication') == 'live'
               and not call['output'].get('error') and not call['output'].get('code')
               for call in trace):
        failures.append('business_freshness_required')
    return failures
