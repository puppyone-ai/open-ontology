export const PUBLICATION_READS = [
  'query', 'tables', 'describe', 'knowledge_cat', 'knowledge_search', 'validate', 'freshness',
];

export function publicationPolicy(config) {
  if (!Object.hasOwn(config, 'publicationBinding')) return null;
  const value = config.publicationBinding;
  if (!value || typeof value !== 'object' || Array.isArray(value) || value.mode !== 'f22' ||
      Object.keys(value).some(key => !['mode', 'allowWholeQuestionRetry'].includes(key)) ||
      (Object.hasOwn(value, 'allowWholeQuestionRetry') && typeof value.allowWholeQuestionRetry !== 'boolean')) {
    throw new Error('INVALID_PUBLICATION_BINDING_CONFIG');
  }
  return { protocol: 'f22-v1', allow_whole_question_retry: value.allowWholeQuestionRetry === true };
}

export function validPublicationReceipt(value) {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).sort().join(',') === 'data_vector_sha256,publication_id' &&
    typeof value.publication_id === 'string' && /^pub1_[a-f0-9]{64}$/.test(value.publication_id) &&
    typeof value.data_vector_sha256 === 'string' && /^[a-f0-9]{64}$/.test(value.data_vector_sha256));
}

export function publicationFlags(operation, config, expected) {
  if (!publicationPolicy(config) || operation === 'help') return [];
  if (!PUBLICATION_READS.includes(operation)) throw new Error('PUBLICATION_OPERATION_UNSUPPORTED');
  if (expected === 'current') return ['--expected-publication-id', 'current'];
  if (!validPublicationReceipt(expected)) throw new Error('PUBLICATION_BINDING_REQUIRED');
  return ['--expected-publication-id', expected.publication_id,
    '--expected-data-vector-sha256', expected.data_vector_sha256];
}

export function isPublicationDrift(result, output) {
  const hints = typeof output?.hint === 'string' ? output.hint.split(';').map(part => part.trim()) : [];
  return result.code === 2 && !result.limited && output?.class === 'gateway' &&
    output.code === 'CONFLICT' && output.exit === 2 && output.retryable === false &&
    !Object.hasOwn(output, 'rows') && !Object.hasOwn(output, 'publication_binding') &&
    !Object.hasOwn(output, 'error') &&
    hints.includes('reason=PUBLICATION_DRIFT') && hints.includes('action=RECOMPUTE_WHOLE_QUESTION');
}

export function observePublication(result, output, expected) {
  if (isPublicationDrift(result, output)) throw new Error('PUBLICATION_DRIFT');
  if (result.code !== 0 || result.limited || output?.error || typeof output?.code === 'string') {
    if (['usage', 'protocol'].includes(output?.class) ||
        ['UNAUTHENTICATED', 'PERMISSION_DENIED', 'FORBIDDEN', 'SCHEMA_INVALID'].includes(output?.code)) {
      throw new Error('PUBLICATION_CALL_FAILED');
    }
    return null;
  }
  const receipt = output?.publication_binding;
  if (!validPublicationReceipt(receipt) || (expected !== 'current' &&
      (receipt.publication_id !== expected?.publication_id ||
       receipt.data_vector_sha256 !== expected?.data_vector_sha256))) {
    throw new Error('PUBLICATION_BINDING_INVALID');
  }
  return { publication_id: receipt.publication_id, data_vector_sha256: receipt.data_vector_sha256 };
}
