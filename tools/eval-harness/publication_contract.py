"""Validate F22 evidence and authorize only explicit, bounded whole-question recomputation."""
import re

READS = {'query', 'tables', 'describe', 'knowledge_cat', 'knowledge_search', 'validate', 'freshness'}


def valid_receipt(value):
    return (isinstance(value, dict) and set(value) == {'publication_id', 'data_vector_sha256'}
            and isinstance(value['publication_id'], str)
            and re.fullmatch(r'pub1_[a-f0-9]{64}', value['publication_id']) is not None
            and isinstance(value['data_vector_sha256'], str)
            and re.fullmatch(r'[a-f0-9]{64}', value['data_vector_sha256']) is not None)


def failed(call):
    output = call.get('output')
    return bool(call.get('error') or (isinstance(output, dict) and
                                     (output.get('error') or output.get('code'))))


def is_drift(call):
    output = call.get('output') or {}
    hints = output.get('hint', '')
    parts = [part.strip() for part in hints.split(';')] if isinstance(hints, str) else []
    return (call.get('process_exit_code') == 2 and not call.get('limited')
            and output.get('class') == 'gateway' and output.get('code') == 'CONFLICT'
            and output.get('exit') == 2 and output.get('retryable') is False
            and not any(key in output for key in ('rows', 'publication_binding', 'error'))
            and 'reason=PUBLICATION_DRIFT' in parts and 'action=RECOMPUTE_WHOLE_QUESTION' in parts)


def retry_allowed(response, policy, attempt):
    if (not policy or policy.get('allow_whole_question_retry') is not True or attempt != 1
            or response.get('error') != 'PUBLICATION_DRIFT'
            or response.get('question_discarded') is not True):
        return False
    failures = [call for call in response.get('binding_checks', []) + response.get('trace', []) if failed(call)]
    # A timeout, authentication/input failure or protocol failure is never a retry warrant.
    return bool(failures) and all(is_drift(call) for call in failures)


def evidence_error(response):
    runtime = response.get('runtime_binding') or {}
    receipt = runtime.get('publication_binding')
    if not valid_receipt(receipt) or response.get('runtime_binding_after') != runtime:
        return 'publication_runtime_binding_invalid'
    checks = response.get('binding_checks', [])
    if len(checks) != 2 or checks[0].get('expected_binding') != 'current':
        return 'publication_acquisition_unverified'
    for index, call in enumerate(checks):
        if (failed(call) or call.get('process_exit_code') != 0 or call.get('limited')
                or call.get('output', {}).get('publication_binding') != receipt
                or (index and call.get('expected_binding') != receipt)):
            return 'publication_check_receipt_mismatch'
    for call in response.get('trace', []):
        operation = call.get('operation')
        if operation == 'help':
            continue  # Offline CLI help is not a Gateway read.
        if operation not in READS or call.get('expected_binding') != receipt:
            return 'publication_read_unpinned'
        if not failed(call) and (call.get('process_exit_code') != 0 or call.get('limited')
                                or call.get('output', {}).get('publication_binding') != receipt):
            return 'publication_read_receipt_mismatch'
    return None
