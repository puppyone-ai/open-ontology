import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cliArgs, checkRuntimeBinding, prepareEvaluation } from './agent.mjs';
import { publicationPolicy, PUBLICATION_READS, observePublication,
  isPublicationDrift } from '../agent-contract/publication-binding.mjs';

const receipt = { publication_id: 'pub1_' + 'a'.repeat(64), data_vector_sha256: 'b'.repeat(64) };
const config = { publicationBinding: { mode: 'f22' }, cliCommand: ['/fixture/oo'],
  gateway: 'http://127.0.0.1:1', tokenFile: '/not-read' };
const drift = { class: 'gateway', code: 'CONFLICT', exit: 2, retryable: false,
  hint: 'reason=PUBLICATION_DRIFT; action=RECOMPUTE_WHOLE_QUESTION', query_id: 'q_drift' };

test('F22 is explicit; malformed selection never falls back and retry is opt-in', async () => {
  assert.equal(publicationPolicy({}), null);
  assert.equal(publicationPolicy(config).allow_whole_question_retry, false);
  assert.equal(publicationPolicy({ publicationBinding: {
    mode: 'f22', allowWholeQuestionRetry: true,
  } }).allow_whole_question_retry, true);
  for (const publicationBinding of [null, false, 'f22', {}, { mode: 'f22', allowWholeQuestionRetry: 1 },
    { mode: 'F22' }, { mode: 'f22', maxRetries: 2 }]) {
    await assert.rejects(prepareEvaluation({ publicationBinding }), /INVALID_PUBLICATION_BINDING_CONFIG/);
  }
  assert((await prepareEvaluation(config)).evidence.publication_policy);
});

test('all supported reads carry the frozen pair; help stays offline and status is refused', () => {
  for (const operation of PUBLICATION_READS) {
    const args = cliArgs({ operation, input: 'manifest.json', publication_id: 'model-override',
      data_vector_sha256: 'model-override' }, config, receipt);
    assert.equal(args[args.indexOf('--expected-publication-id') + 1], receipt.publication_id);
    assert.equal(args[args.indexOf('--expected-data-vector-sha256') + 1], receipt.data_vector_sha256);
    assert(!args.includes('model-override'));
    assert.throws(() => cliArgs({ operation, input: 'x' }, config), /PUBLICATION_BINDING_REQUIRED/);
  }
  const first = cliArgs({ operation: 'freshness' }, config, 'current');
  assert(first.includes('current'));
  assert(!first.includes('--expected-data-vector-sha256'));
  assert.deepEqual(cliArgs({ operation: 'help' }, config, receipt), ['--help']);
  assert.throws(() => cliArgs({ operation: 'status', input: 'q_x' }, config, receipt),
    /PUBLICATION_OPERATION_UNSUPPORTED/);
  const { publicationBinding: _unused, ...generic } = config;
  assert(!cliArgs({ operation: 'tables' }, generic).includes('current'));
});

test('first authenticated read acquires the receipt, then runtime verification pins both fields', async () => {
  const calls = [];
  const checks = [];
  const invoke = async (_prefix, args) => {
    calls.push(args);
    return { code: 0, stdout: JSON.stringify({ publication_binding: receipt,
      policy_version: 'p1', knowledge_version: 'k1', source_time: 'unmeasured' }) };
  };
  const runtime = await checkRuntimeBinding(config, invoke, undefined, checks);
  const after = await checkRuntimeBinding(config, invoke, runtime.publication_binding, checks);
  assert.deepEqual(runtime, after);
  assert.deepEqual(runtime.publication_binding, receipt);
  assert(calls[0].includes('current'));
  assert(calls[1].includes(receipt.data_vector_sha256));
  assert.equal(checks.length, 2);
  assert(!JSON.stringify(checks).includes('not-read'));
});

test('successful reads without acknowledgement or with data-only vector drift fail closed', () => {
  for (const value of [undefined, null, {}, { ...receipt, publication_id: 'current' },
    { ...receipt, data_vector_sha256: 'c'.repeat(64) },
    { ...receipt, publication_id: 'pub1_' + 'A'.repeat(64) },
    { ...receipt, data_vector_sha256: receipt.data_vector_sha256 + ' ' }]) {
    assert.throws(() => observePublication({ code: 0 }, { publication_binding: value }, receipt),
      /PUBLICATION_BINDING_INVALID/);
  }
  assert.deepEqual(observePublication({ code: 0 }, { publication_binding: receipt }, 'current'), receipt);
});

test('only the exact flat CLI drift receipt is a recomputation signal, not auth/input/protocol errors', async () => {
  assert(isPublicationDrift({ code: 2 }, drift));
  assert.throws(() => observePublication({ code: 2 }, drift, receipt), /PUBLICATION_DRIFT/);
  for (const patch of [{ class: 'usage' }, { class: 'protocol' }, { code: 'UNAUTHENTICATED' },
    { retryable: true }, { exit: 4 }, { hint: 'PUBLICATION_DRIFT' },
    { rows: [] }, { publication_binding: receipt }, { error: {} }]) {
    assert(!isPublicationDrift({ code: 2 }, { ...drift, ...patch }));
  }
  assert(!isPublicationDrift({ code: 0 }, drift));
  assert(!isPublicationDrift({ code: 2, limited: true }, drift));
  const checks = [];
  await assert.rejects(checkRuntimeBinding(config, async () => ({
    code: 2, stdout: JSON.stringify(drift),
  }), receipt, checks), /PUBLICATION_DRIFT/);
  assert.equal(checks[0].output.query_id, 'q_drift');
  assert.equal(checks[0].process_exit_code, 2);
  assert.equal(checks[0].error, true);
});
