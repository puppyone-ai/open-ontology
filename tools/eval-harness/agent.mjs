import { readFile, realpath } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { fileURLToPath } from 'node:url';
import { searchFlags, searchSchema } from '../agent-contract/knowledge-search.mjs';
import { modelEndpoint } from '../agent-contract/model-endpoint.mjs';
import { aggregateOnly } from '../agent-contract/aggregate-query.mjs';
import { composeBusinessSystemPrompt } from '../agent-contract/business-answer-policy.mjs';
import { fileEvidence, loadSkillBinding, sha256 } from '../agent-contract/skill-binding.mjs';
import { publicationPolicy, publicationFlags, observePublication } from '../agent-contract/publication-binding.mjs';
export { aggregateOnly };

const digest = value => createHash('sha256').update(JSON.stringify(value) ?? 'null').digest('hex');
export const OPERATIONS = ['help', 'tables', 'describe', 'knowledge_cat', 'knowledge_search',
                           'validate', 'query', 'freshness', 'status'];

export function payloadEvidence(payload) {
  const messages = Array.isArray(payload.messages) ? payload.messages : [];
  return {
    message_count: messages.length,
    messages: messages.slice(0, 256).map(message => ({
      role: ['system', 'developer', 'user', 'assistant', 'tool'].includes(message.role) ? message.role : 'other',
      bytes: Buffer.byteLength(JSON.stringify(message)),
      sha256: digest(message),
      tool_calls: Array.isArray(message.tool_calls) ? message.tool_calls.length : 0,
    })),
    messages_omitted: Math.max(0, messages.length - 256),
    tools_count: Array.isArray(payload.tools) ? payload.tools.length : 0,
    tools_sha256: digest(payload.tools),
    stream: payload.stream === true,
    token_limit_field: payload.max_completion_tokens !== undefined ? 'max_completion_tokens'
      : payload.max_tokens !== undefined ? 'max_tokens' : 'absent',
    reasoning_effort_present: payload.reasoning_effort !== undefined,
  };
}

export function modelEventEvidence(message) {
  const raw = message.rawStopReason;
  const recognized = ['stop', 'length', 'tool_calls', 'function_call', 'content_filter',
    'network_error', 'MALFORMED_FUNCTION_CALL', 'STOP', 'MAX_TOKENS'];
  return {
    stop_reason: message.stopReason,
    raw_stop_reason: typeof raw === 'string' ? (recognized.includes(raw) ? raw : 'other') : undefined,
    raw_stop_sha256: raw === undefined ? undefined : digest(raw),
    input_tokens: message.usage?.input, output_tokens: message.usage?.output,
    error_class: modelErrorClass(message),
    error_sha256: message.errorMessage ? digest(message.errorMessage) : undefined,
    text_bytes: (message.content || []).filter(c => c.type === 'text')
      .reduce((n, c) => n + Buffer.byteLength(c.text || ''), 0),
    thinking_bytes: (message.content || []).filter(c => c.type === 'thinking')
      .reduce((n, c) => n + Buffer.byteLength(c.thinking || ''), 0),
  };
}

export function cliArgs(args, config, publicationBinding) {
  if (!OPERATIONS.includes(args.operation)) throw new Error('INVALID_OPERATION');
  if (args.operation === 'help') return ['--help'];
  const argv = args.operation.startsWith('knowledge_') ? ['knowledge', args.operation.slice(10)] : [args.operation];
  if (args.operation === 'freshness') argv.push('--summary');
  argv.push(...searchFlags(args));
  argv.push('--json', '--gateway', config.gateway, '--transport', config.transport || 'local-development', '--auth', 'external', '--token-file', config.tokenFile);
  argv.push(...publicationFlags(args.operation, config, publicationBinding));
  if (config.caFile) argv.push('--ca-file', config.caFile);
  if (config.clientPemFile) argv.push('--client-pem', config.clientPemFile);
  if (config.connectIp !== undefined) argv.push('--connect-ip', config.connectIp);
  if (config.interface !== undefined) argv.push('--interface', config.interface);
  if (['describe', 'knowledge_cat', 'knowledge_search', 'validate', 'query', 'status'].includes(args.operation)) {
    if (typeof args.input !== 'string' || !args.input.trim() || args.input.startsWith('-') || args.input.includes('\0') || args.input.length > 8000) throw new Error('invalid input');
    if (args.operation === 'knowledge_cat' && (!/^[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*$/.test(args.input)
        || args.input.split('/').some(part => part === '.' || part === '..'))) throw new Error('INVALID_KNOWLEDGE_PATH');
    argv.push(args.input);
  }
  return argv;
}

export function toolInputFailure(error) {
  if (error?.message === 'INVALID_OPERATION') return {
    code: 'TOOL_INPUT_INVALID', hint: `operation must be exactly one of: ${OPERATIONS.join(', ')}. Do not append arguments to operation; put them in input.`,
  };
  if (error?.message === 'INVALID_KNOWLEDGE_PATH') return {
    code: 'TOOL_INPUT_INVALID', hint: 'knowledge_cat input must be only the exact relative file path, e.g. README.md, metrics.md, glossary.md. No commentary or extra words.',
  };
  if (['INVALID_SEARCH_PREFIX', 'INVALID_SEARCH_PAGE'].includes(error?.message)) return {
    code: 'TOOL_INPUT_INVALID', hint: 'Search uses a relative path_prefix (or empty), integer offset >=0 and limit 1..100. These options apply only to knowledge_search.',
  };
  return { code: 'CLI_INVOCATION_FAILED' };
}

export function processCli(command, argv) {
  return new Promise((accept, reject) => {
    const child = spawn(command[0], [...command.slice(1), ...argv], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '', limited = false;
    const timer = setTimeout(() => { limited = true; child.kill(); }, 35000);
    const append = (chunk, error) => {
      if (error) stderr += chunk; else stdout += chunk;
      if (stdout.length + stderr.length > 2000000) { limited = true; child.kill(); }
    };
    child.stdout.on('data', c => append(c, false));
    child.stderr.on('data', c => append(c, true));
    child.on('error', () => { clearTimeout(timer); reject(new Error('CLI_START_FAILED')); });
    child.on('close', code => { clearTimeout(timer); accept({ code, stdout, stderr, limited }); });
  });
}

export function parseExact(text) {
  return JSON.parse(text, (_key, value, context) => {
    if (typeof value !== 'number') return value;
    if (!context?.source) throw new Error('EXACT_JSON_RUNTIME_UNSUPPORTED');
    return !Number.isSafeInteger(value) || /[.eE]/.test(context.source) ? context.source : value;
  });
}

export function parseFinal(answer) {
  const clean = answer.trim().replace(/^```(?:json)?\s*|\s*```$/g, '');
  for (let start = 0; start < clean.length; start++) {
    if (clean[start] !== '{') continue;
    try {
      const value = parseExact(clean.slice(start));
      if (value && typeof value === 'object' && !Array.isArray(value)) return value;
    } catch {}
  }
  throw new Error('INVALID_FINAL_JSON');
}

export function modelConfig(config) {
  const contextWindow = config.modelContextWindow ?? 128000;
  const maxTokens = config.modelMaxTokens ?? 8192;
  if (!Number.isSafeInteger(contextWindow) || contextWindow < 1024 || contextWindow > 2000000 ||
      !Number.isSafeInteger(maxTokens) || maxTokens < 128 || maxTokens > 131072 || maxTokens > contextWindow) {
    throw new Error('INVALID_MODEL_LIMITS');
  }
  return { id: config.model || 'grok-4.7', name: 'CLI evaluation', api: 'openai-completions', provider: 'eval-local',
    baseUrl: modelEndpoint(config.modelEndpoint || 'http://127.0.0.1:8317/v1'), reasoning: true, input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow, maxTokens,
    compat: { supportsStore: false, supportsDeveloperRole: false, supportsReasoningEffort: true } };
}

export async function modelKey(config, env = process.env) {
  if (config.apiKeyEnv !== undefined) {
    if (!/^[A-Z][A-Z0-9_]{0,63}$/.test(config.apiKeyEnv)) throw new Error('INVALID_KEY_ENV_NAME');
    const key = env[config.apiKeyEnv]?.trim();
    if (!key) throw new Error('MODEL_KEY_ENV_MISSING');
    return key;
  }
  return (await readFile(config.bearerFile, 'utf8')).trim();
}

export function modelErrorClass(message) {
  if (message.stopReason === 'length') return 'output_limit';
  if (!['error', 'aborted'].includes(message.stopReason)) return undefined;
  const text = String(message.errorMessage || '').toLowerCase();
  if (/ip_blocked/.test(text)) return 'ip_blocked';
  if (/\bmodel_cooldown\b/.test(text)) return 'provider_cooldown';
  if (/quota|usage_limit|insufficient.credit|insufficient.balance/.test(text)) return 'quota';
  if (/429|rate.limit|too.many.requests/.test(text)) return 'rate_limit';
  if (/context.length|context.window|too.many.tokens/.test(text)) return 'context_limit';
  if (/401|403|unauthori[sz]ed|invalid.api.key/.test(text)) return 'authentication';
  if (/timeout|timed.out/.test(text)) return 'timeout';
  if (/\b5\d\d\b|overload|bad.gateway|service.unavailable/.test(text)) return 'upstream';
  if (/\b4\d\d\b/.test(text)) return 'request_rejected';
  if (/provider finish_reason: content_filter/.test(text)) return 'content_filter';
  if (/connection.reset|econnreset|network_error|fetch failed|terminated/.test(text)) return 'transport';
  if (/stream ended without finish_reason/.test(text)) return 'stream_incomplete';
  if (/provider finish_reason:/.test(text)) return 'provider_stop_reason';
  if (message.stopReason === 'aborted') return 'aborted';
  return 'unknown';
}

export function modelFailure(current, message) {
  // A provider's aborted event must not overwrite our known deadline/tool limit.
  return current || (['error', 'aborted', 'length'].includes(message.stopReason)
    ? 'MODEL_OR_BUDGET_FAILURE' : undefined);
}

export function modelRequestTimeout(config) {
  const value = config.modelRequestTimeoutMs ?? 180000;
  if (!Number.isSafeInteger(value) || value < 10000 || value > 600000) {
    throw new Error('INVALID_MODEL_REQUEST_TIMEOUT');
  }
  return value;
}

export function evaluationContext(config) {
  let scope = '';
  if (config.businessScope !== undefined) {
    const value = config.businessScope;
    if (!value || typeof value !== 'object' || Array.isArray(value) ||
        Object.keys(value).sort().join(',') !== 'company_ids,coverage,tenant_id' ||
        typeof value.tenant_id !== 'string' || !/^[0-9]{1,32}$/.test(value.tenant_id) ||
        !Array.isArray(value.company_ids) || !value.company_ids.length || value.company_ids.length > 100 ||
        value.company_ids.some(id => typeof id !== 'string' || !/^[0-9]{1,32}$/.test(id)) ||
        new Set(value.company_ids).size !== value.company_ids.length ||
        value.coverage !== 'local_imported_snapshot_only') throw new Error('INVALID_BUSINESS_SCOPE');
    scope = `\nOperator-confirmed analysis scope: ${JSON.stringify(value)}. Use this scope for "we"; do not ask the user again. This does not expand Gateway authorization or certify metric definitions. Query the local imported data, not a source database; do not claim source completeness.`;
  }
  if (config.currentDate === undefined && config.timezone === undefined) return scope;
  const day = config.currentDate;
  if (typeof day !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(day) ||
      !Number.isFinite(Date.parse(day)) || new Date(day).toISOString().slice(0, 10) !== day ||
      typeof config.timezone !== 'string') throw new Error('INVALID_EVALUATION_CLOCK');
  try { new Intl.DateTimeFormat('en', { timeZone: config.timezone }); }
  catch { throw new Error('INVALID_EVALUATION_CLOCK'); }
  return `\nEvaluation clock: current_date=${day}; timezone=${config.timezone}. Resolve relative dates from this clock, not model training time.` + scope;
}

export function cliFailed(result, output) {
  return result.code !== 0 || result.limited === true ||
    Boolean(output && (output.error || typeof output.code === 'string'));
}

export function queryAccess(config) {
  const access = config.queryAccess ?? 'aggregate_only';
  if (!['aggregate_only', 'gateway_readonly'].includes(access)) throw new Error('INVALID_QUERY_ACCESS');
  return access;
}

export function queryAccessContext(config) {
  return queryAccess(config) === 'gateway_readonly'
    ? '\nOperator-enabled gateway_readonly: 可通过当前 Gateway 查询所需字典和业务数据，不限于聚合。仍优先聚合，仅读取本题必要字段与有限行；不得写库、取凭据或绕过角色授权。此设置不是管理员授权证明。'
    : '\n本评测为 aggregate_only：只允许简单聚合与分组键，不查询逐行明细。';
}

export async function checkRuntimeBinding(config, invoke = processCli, publicationBinding, checks = []) {
  const expected = config.gatewayManifestSha256;
  const policy = publicationPolicy(config);
  if (expected === undefined && !policy) return null;
  if (expected !== undefined && (typeof expected !== 'string' || !/^[a-f0-9]{64}$/.test(expected))) {
    throw new Error('INVALID_GATEWAY_BINDING');
  }
  const args = expected === undefined ? { operation: 'freshness' }
    : { operation: 'knowledge_cat', input: 'manifest.json' };
  const pin = publicationBinding ?? 'current';
  const result = await invoke(config.cliCommand, cliArgs(args, config, pin));
  let output;
  try { output = parseExact(result.stdout || result.stderr); }
  catch { output = { code: 'CLI_INVALID_JSON' }; }
  const metadata = Object.fromEntries(['class', 'code', 'hint', 'exit', 'retryable', 'query_id',
    'policy_version', 'knowledge_version', 'publication_binding'].filter(key => Object.hasOwn(output, key))
    .map(key => [key, output[key]]));
  if (Object.hasOwn(output, 'rows')) metadata.rows = '[suppressed]';
  if (Object.hasOwn(output, 'error')) metadata.error = true;
  const check = { ...args, output: metadata, receipt_sha256: digest(output), error: cliFailed(result, output),
    process_exit_code: result.code, limited: result.limited === true, expected_binding: pin };
  if (policy) checks.push(check);
  let receipt;
  try { if (policy) receipt = observePublication(result, output, pin); }
  catch (error) { check.error = true; throw error; }
  const actual = typeof output.text === 'string'
    ? createHash('sha256').update(output.text).digest('hex') : null;
  if (cliFailed(result, output) || (expected !== undefined &&
      (output.path !== 'manifest.json' || actual !== expected || output.sha256 !== actual))) {
    check.error = true;
    throw new Error('GATEWAY_BINDING_DRIFT');
  }
  if ((Object.hasOwn(config, 'agentSkill') || policy) && (!output.policy_version || !output.knowledge_version)) {
    check.error = true;
    throw new Error('GATEWAY_BINDING_DRIFT');
  }
  return { manifest_sha256: actual, policy_version: output.policy_version,
    knowledge_version: output.knowledge_version, ...(policy ? { publication_binding: receipt } : {}) };
}

export async function prepareEvaluation(config, invoke = processCli) {
  const publication = publicationPolicy(config);
  const skill = await loadSkillBinding(config, invoke);
  const resources = await Promise.all([
    './agent-resources/SKILL.md', '../agent-contract/business-answer-policy.md',
  ].map(name => fileEvidence(fileURLToPath(new URL(name, import.meta.url)), { text: true })));
  const context = evaluationContext(config) + queryAccessContext(config) + (publication
    ? '\nThe runtime pins publication identity and data vector for the whole question. Do not override them or replay a drifting SQL. On publication drift all partial results are discarded; only the operator-controlled runner may restart the entire question with a cold Agent.' : '');
  const mapping = skill ? `\nVerified CLI mapping: ${JSON.stringify({
    declared_path: skill.evidence.cli.declared_path,
    actual_path: skill.evidence.cli.actual.path, tool: 'oo',
  })}. This is an explicit equivalent tool mapping, not proof that agent-skill ships the selected private Skill.` : '';
  const systemPrompt = composeBusinessSystemPrompt(
    resources[0].text + context + mapping, resources[1].text, skill?.text);
  return { systemPrompt, evidence: {
    schema_version: 1, system_prompt_sha256: sha256(systemPrompt),
    system_prompt_bytes: Buffer.byteLength(systemPrompt),
    injected_files: resources.map(resource => resource.evidence),
    context_sha256: sha256(context + mapping), skill: skill?.evidence ?? null,
    ...(publication ? { publication_policy: publication } : {}),
  } };
}

export function verifyWirePrompt(payload, prepared) {
  const messages = (payload.messages || []).filter(message => ['system', 'developer'].includes(message.role));
  if (messages.length !== 1 || messages[0].content !== prepared.systemPrompt) {
    throw new Error('PROMPT_BINDING_DRIFT');
  }
  return prepared.evidence.system_prompt_sha256;
}

async function main() {
  let raw = '';
  for await (const chunk of process.stdin) raw += chunk;
  const { text, config, expected_binding: expectedBinding } = JSON.parse(raw);
  const inspect = process.argv[2] === '--inspect-binding';
  if (!config || typeof config !== 'object' || Array.isArray(config) || (!inspect && typeof text !== 'string')) {
    throw new Error('INVALID_AGENT_INPUT');
  }
  let prepared;
  try {
    prepared = await prepareEvaluation(config);
    if (expectedBinding && !isDeepStrictEqual(expectedBinding, prepared.evidence)) {
      throw new Error('EVALUATION_INPUT_DRIFT');
    }
  } catch (error) {
    const code = /^(?:INVALID_|SKILL_|CLI_BINDING_|EVALUATION_INPUT_)[A-Z_]+$/.test(error.message)
      ? error.message : 'EVALUATION_BINDING_FAILED';
    process.stdout.write(JSON.stringify({ trace: [], error: code }));
    process.exitCode = 2;
    return;
  }
  if (inspect) {
    process.stdout.write(JSON.stringify(prepared.evidence));
    return;
  }
  const publication = publicationPolicy(config);
  const bindingChecks = [];
  let runtimeBinding;
  try { runtimeBinding = await checkRuntimeBinding(config, processCli, undefined, bindingChecks); }
  catch (error) {
    const code = error.message === 'INVALID_GATEWAY_BINDING' || /^PUBLICATION_[A-Z_]+$/.test(error.message)
      ? error.message : 'GATEWAY_BINDING_DRIFT';
    process.stdout.write(JSON.stringify({ trace: [], error: code,
      prompt_binding: prepared.evidence, ...(publication ? { binding_checks: bindingChecks,
        question_discarded: true } : {}) }));
    return;
  }
  const coreRoot = pathToFileURL(resolve(config.demoDir, 'node_modules/@earendil-works/pi-agent-core/package.json'));
  const aiRoot = pathToFileURL(resolve(config.demoDir, 'node_modules/@earendil-works/pi-ai/package.json'));
  const { Agent } = await import(new URL('./dist/index.js', coreRoot));
  const { Type } = await import(new URL('./dist/index.js', aiRoot));
  const { streamSimple } = await import(new URL('./dist/api/openai-completions.js', aiRoot));
  const trace = [];
  const modelEvents = [];
  const modelRequests = [];
  const runStarted = Date.now();
  const requestTimeout = modelRequestTimeout(config);
  const requestTimers = new Map();
  let key;
  const agent = new Agent({
    toolExecution: 'sequential', maxRetryDelayMs: 0,
    streamFn: (model, context, options) => {
      if (publication && failure) throw new Error('PUBLICATION_ATTEMPT_CLOSED');
      const request = { request_index: modelRequests.length + 1, started_ms: Date.now() - runStarted };
      modelRequests.push(request);
      const controller = new AbortController();
      const deadline = setTimeout(() => {
        request.timed_out = true;
        failure ||= 'MODEL_REQUEST_TIMEOUT';
        controller.abort();
      }, requestTimeout);
      deadline.unref();
      requestTimers.set(request.request_index, deadline);
      const signal = options.signal ? AbortSignal.any([options.signal, controller.signal]) : controller.signal;
      return streamSimple(model, context, { ...options, signal, maxRetries: 0,
        onPayload: payload => {
          request.payload = payloadEvidence(payload);
          if (prepared.evidence.skill || publication) {
            try { request.system_prompt_sha256 = verifyWirePrompt(payload, prepared); }
            catch { failure = 'PROMPT_BINDING_DRIFT'; controller.abort(); throw new Error(failure); }
          }
        },
        onResponse: response => {
          request.http_status = response.status;
          request.headers_received_ms = Date.now() - runStarted;
        },
      });
    },
    getApiKey: async () => key ||= await modelKey(config),
    initialState: {
      systemPrompt: prepared.systemPrompt,
      model: modelConfig(config),
      thinkingLevel: 'medium',
      tools: [{ name: 'oo', label: 'oo CLI', description: 'Only data access: knowledge_cat input is a returned path; knowledge_search input is one literal substring, not a list of keywords; describe input is schema.table; query/validate input is SQL. freshness/tables take no input.',
        parameters: Type.Object({ operation: Type.Union(OPERATIONS.map(value => Type.Literal(value))),
                                 input: Type.Optional(Type.String()),
                                 ...searchSchema(Type) }),
        execute: async (_id, args) => {
          if (publication && failure) throw new Error('PUBLICATION_ATTEMPT_CLOSED');
          if (trace.length >= 60) { failure = 'TOOL_BUDGET'; agent.abort(); throw new Error('TOOL_BUDGET'); }
          let output, result, error = false;
          const started = Date.now();
          if (args.operation === 'query' && queryAccess(config) === 'aggregate_only' && !aggregateOnly(args.input)) {
            output = { code: 'EVAL_AGGREGATE_ONLY', message: 'Only simple aggregate projections and grouping keys may be queried; no row data.' };
            error = true;
          } else {
            try {
              if (prepared.evidence.skill && !isDeepStrictEqual(
                (await prepareEvaluation(config)).evidence, prepared.evidence)) {
                throw new Error('EVALUATION_INPUT_DRIFT');
              }
              result = await processCli(config.cliCommand, cliArgs(args, config, runtimeBinding?.publication_binding));
              error = result.code !== 0 || result.limited;
              try { output = parseExact(result.stdout || result.stderr); }
              catch {
                output = args.operation === 'help' && !error
                  ? { text: result.stdout }
                  : { code: 'CLI_INVALID_JSON', text: '[non-JSON output suppressed]' };
              }
              if (result.limited) output = { code: 'CLI_OUTPUT_OR_TIME_LIMIT' };
              error = cliFailed(result, output);
              if (publication && args.operation !== 'help') {
                observePublication(result, output, runtimeBinding.publication_binding);
              }
              if (!error && prepared.evidence.skill && args.operation === 'query' &&
                  ['policy_version', 'knowledge_version'].some(key => output[key] !== runtimeBinding[key])) {
                error = true;
                failure = 'GATEWAY_BINDING_DRIFT';
                agent.abort();
              }
            } catch (cause) {
              error = true;
              if (/^(SKILL_|CLI_BINDING_|EVALUATION_INPUT_)/.test(cause.message)) {
                failure = 'EVALUATION_INPUT_DRIFT';
                agent.abort();
                output = { code: failure };
              } else if (/^PUBLICATION_[A-Z_]+$/.test(cause.message)) {
                failure = cause.message;
                agent.abort();
                output ??= { code: failure };
              } else output = toolInputFailure(cause);
            }
          }
          trace.push({ ...args, output, error, elapsed_ms: Date.now() - started,
            ...(publication ? { expected_binding: runtimeBinding.publication_binding,
              process_exit_code: result?.code ?? null, limited: result?.limited === true } : {}) });
          return { content: [{ type: 'text', text: JSON.stringify(output) }], details: {} };
        } }],
    },
  });
  let failure;
  agent.subscribe(event => {
    if (event.type === 'message_end' && event.message.role === 'assistant') {
      const request = modelRequests.at(-1);
      clearTimeout(requestTimers.get(request?.request_index));
      requestTimers.delete(request?.request_index);
      modelEvents.push({ ...modelEventEvidence(event.message),
        request_index: request?.request_index, ended_ms: Date.now() - runStarted });
      failure = modelFailure(failure, event.message);
    }
  });
  const timer = setTimeout(() => { failure = 'AGENT_TIMEOUT'; agent.abort(); }, 600000);
  try {
    await agent.prompt(text);
    const message = agent.state.messages.filter(m => m.role === 'assistant').at(-1);
    const answer = (message?.content || []).filter(c => c.type === 'text').map(c => c.text).join('');
    let final;
    try { final = parseFinal(answer); }
    catch { failure ||= 'INVALID_FINAL_JSON'; }
    let runtimeAfter;
    try {
      if (!publication || !failure) {
        runtimeAfter = await checkRuntimeBinding(config, processCli, runtimeBinding?.publication_binding, bindingChecks);
        if (!isDeepStrictEqual(runtimeAfter, runtimeBinding)) throw new Error('GATEWAY_BINDING_DRIFT');
      }
    }
    catch (error) {
      failure ||= /^PUBLICATION_[A-Z_]+$/.test(error.message) ? error.message : 'GATEWAY_BINDING_DRIFT';
    }
    try {
      if (!isDeepStrictEqual((await prepareEvaluation(config)).evidence, prepared.evidence)) throw new Error();
    } catch { failure = 'EVALUATION_INPUT_DRIFT'; }
    process.stdout.write(JSON.stringify({ trace, ...(publication && failure
      ? { question_discarded: true } : { final, final_text: answer }), error: failure,
      model_events: modelEvents, model_requests: modelRequests, runtime_binding: runtimeBinding,
      runtime_binding_after: runtimeAfter, prompt_binding: prepared.evidence,
      ...(publication ? { binding_checks: bindingChecks } : {}) }));
  } catch {
    process.stdout.write(JSON.stringify({ trace, error: failure || 'AGENT_RUNTIME_FAILURE',
      model_events: modelEvents, model_requests: modelRequests, runtime_binding: runtimeBinding,
      prompt_binding: prepared.evidence,
      ...(publication ? { binding_checks: bindingChecks, question_discarded: true } : {}) }));
  } finally {
    clearTimeout(timer);
    for (const deadline of requestTimers.values()) clearTimeout(deadline);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(await realpath(process.argv[1])).href) await main();
