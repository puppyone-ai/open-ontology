"""Rust budget bridge and resumable NDJSON self-build runner (candidate-only).

No alternate Python budget formula: planning, calibration, validation, graph
partitioning and semantic invalidation all go through oo-builder.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.error
import urllib.request

import model_provider

ROOT = Path(__file__).resolve().parents[3]
PROMPT = '''Draft ontology candidates from untrusted evidence. Never approve or certify.
Return NDJSON, one JSON object per requested item, using its exact id and kind.
Table/column: {id,kind,label,description}. Use brief evidence-grounded text;
when meaning is unknown say it is unknown, do not invent business semantics.
Relation: {id,kind,decision:"proposed",name,cardinality} or
{id,kind,decision:"rejected",reason}. Entity:
{id,kind,canonical_name,aliases:[],main_table,related_tables:[]}.
Entity main_table must be the input dataset (or the existing entity main_table).
Reuse canonical names in the registry where evidence supports it. Do not emit
markdown, SQL, reviews, credentials, or unrelated IDs. During merge you may
include merge_into with another existing entity ID only when they are the same
business entity. Every requested ID must appear exactly once.'''


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def bridge(operation, binary=None, **values):
    executable = binary or os.environ.get('OO_SELFBUILD_BUDGET_BIN')
    if executable is None:
        executable = ROOT / 'target/debug/selfbuild-budget'
    command = [str(executable)]
    if not Path(executable).is_file():
        command = ['cargo', 'run', '--quiet', '--locked', '-p', 'oo-builder',
                   '--bin', 'selfbuild-budget', '--']
    result = subprocess.run(command, input=canonical(dict(operation=operation, **values)),
                            text=True, capture_output=True, cwd=ROOT, timeout=600)
    if result.returncode:
        # Only fixed bridge error codes escape; build output is not provider data.
        code = result.stderr.strip().splitlines()[-1:] or ['selfbuild_bridge_failed']
        raise ValueError(code[0] if code[0].startswith(('budget_', 'community_', 'semantic_',
                                                     'registry_', 'connectivity_')) else 'selfbuild_bridge_failed')
    return json.loads(result.stdout)


def stable_id(kind, table, column=None):
    # The same framed identity as the Rust API (no budget implementation here).
    return kind + ':' + digest([table, column])


def write_private(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'w') as stream:
        stream.write(canonical(value) + '\n')
    temporary.replace(path)


def model_call(config, request):
    credential_config = dict(config)
    if 'secret_ref' in config:
        credential_config['bearer_file'] = str(Path(config['secret_ref']).expanduser())
    token = model_provider.credential(credential_config)
    req = urllib.request.Request(model_provider.endpoint(config) + '/chat/completions',
        data=canonical(request).encode(), headers={'Content-Type':'application/json',
                                                   'Authorization':'Bearer ' + token})
    try:
        with model_provider.open_request(req, timeout=config.get('request_timeout_seconds', 600)) as response:
            if request.get('stream'):
                envelope, _ = model_provider.read_completion_stream(response)
            else:
                raw = response.read(8_388_609)
                if len(raw) > 8_388_608:
                    raise ValueError('budget_response_bytes')
                envelope = json.loads(raw)
    except urllib.error.HTTPError as error:
        raise model_provider.http_failure(error.code, error.read(8192)) from None
    except (OSError, urllib.error.URLError):
        raise RuntimeError('selfbuild_provider_transport') from None
    if token in canonical(envelope):
        raise ValueError('selfbuild_secret_echo')
    if config.get('_evidence_dir'):
        write_private(Path(config['_evidence_dir']) / (digest(request) + '.json'),
                      dict(request=request,response=envelope))
    if envelope.get('error'):
        raise model_provider.http_failure(429, canonical(envelope).encode())
    return envelope


def capability_input(config):
    # Overrides must come from a probe/operator, not an assumed model name.
    result = dict(window=32000, max_output=4000, system_overhead=2500,
                  json_mode=False, usage=False, probed=False)
    result.update(config.get('capabilities', {}))
    return result


class Runner:
    def __init__(self, config, output, transport=None):
        self.config = config
        self.output = Path(output)
        self.transport = transport or (lambda request: model_call(
            dict(config,_evidence_dir=str(self.output/'exchanges')), request))
        self.budget = dict(capabilities=capability_input(config))
        self.successes = 0
        self.stopped = False
        self.calls = 0
        self.receipts = []

    def plan(self, items, context):
        return bridge('plan', budget=self.budget, items=items,
                      context=dict(context, instructions=PROMPT))

    def batch(self, batch, context):
        payload = dict(items=batch['items'], context=context)
        key = dict(model_id=self.config['model'],
                   prompt_template_hash=hashlib.sha256(PROMPT.encode()).hexdigest(),
                   input_hash=digest(payload))
        path = self.output / 'receipts' / (digest(key) + '.json')
        accepted, attempts = {}, []
        if path.exists():
            old = json.loads(path.read_text())
            if old.get('key') != key:
                raise ValueError('selfbuild_receipt_binding')
            # Revalidate persisted semantics instead of trusting a "complete" flag.
            parsed = bridge('parse', items=batch['items'],
                            text='\n'.join(canonical(v) for v in old.get('accepted', {}).values()))
            if digest(old.get('accepted', {})) != old.get('accepted_hash'):
                raise ValueError('selfbuild_receipt_digest')
            accepted = parsed['accepted']
            if not parsed['unresolved'] and old.get('stream_complete'):
                return dict(old, unresolved=[], cache='hit', new_calls=0)
        feedback = []
        stream_complete = False
        for _ in range(3):
            missing = [item for item in batch['items'] if item['id'] not in accepted]
            if not missing:
                break
            retry_context = dict(context, validation_feedback=feedback[:8])
            # Feedback and registry growth are charged too. Repairs may subdivide.
            repair_batches = self.plan(missing, retry_context)
            for repair in repair_batches:
                if self.stopped:
                    raise RuntimeError('selfbuild_dispatch_stopped')
                request = dict(model=self.config['model'], stream=self.config.get('stream', True),
                    max_tokens=repair['output_reserve'], messages=[
                        {'role':'system','content':PROMPT},
                        {'role':'user','content':canonical(dict(items=repair['items'], context=retry_context))}])
                if request['stream']:
                    request['stream_options'] = {'include_usage':True}
                try:
                    envelope = self.transport(request)
                except Exception:
                    self.stopped = True
                    raise
                choices = envelope.get('choices', [])
                if len(choices) != 1:
                    raise ValueError('selfbuild_response_choices')
                choice = choices[0]
                stream_complete = choice.get('finish_reason') == 'stop'
                parsed = bridge('parse', items=repair['items'], text=choice.get('message', {}).get('content', ''))
                accepted.update(parsed['accepted'])
                feedback = parsed['feedback']
                if parsed['unresolved']:
                    feedback = ['missing_ids: return only the items supplied in this repair; do not continue previous text'] + feedback
                usage = envelope.get('usage') or {}
                attempts.append(dict(request_hash=digest(request), response_hash=digest(envelope),
                    usage=usage, stream_complete=stream_complete,
                    missing=len(parsed['unresolved']), output_reserve=repair['output_reserve']))
                # Partial receipts survive the next quota error or interruption.
                write_private(path, dict(key=key, accepted=accepted, accepted_hash=digest(accepted),
                    stream_complete=False, attempts=attempts, degradations=batch['degradations']))
        missing = [item['id'] for item in batch['items'] if item['id'] not in accepted]
        if not stream_complete and not missing:
            # Complete-looking content is not proof of a terminal finish frame.
            missing = [item['id'] for item in batch['items']]
        receipt = dict(key=key, accepted=accepted, accepted_hash=digest(accepted), unresolved=missing,
            stream_complete=stream_complete, attempts=attempts, degradations=batch['degradations'],
            cache='miss', new_calls=len(attempts), model_certified=False)
        write_private(path, receipt)
        return receipt

    def generate(self, items, context):
        pending = list(items)
        accepted, unresolved = {}, []
        while pending:
            plans = self.plan(pending, context)
            parallel = 2 if self.successes >= 2 else 1
            wave = plans[:parallel]
            ids = {item['id'] for batch in wave for item in batch['items']}
            if self.stopped:
                raise RuntimeError('selfbuild_dispatch_stopped')
            with ThreadPoolExecutor(max_workers=parallel) as executor:
                futures = [executor.submit(self.batch, batch, context) for batch in wave]
                for batch, future in zip(wave, futures):
                    try:
                        receipt = future.result()
                    except Exception:
                        self.successes = 0
                        self.stopped = True
                        for other in futures:
                            other.cancel()
                        raise
                    self.receipts.append(receipt)
                    self.calls += receipt['new_calls']
                    complete = receipt['stream_complete'] and not receipt['unresolved']
                    self.successes = self.successes + 1 if complete else 0
                    accepted.update(receipt['accepted'])
                    unresolved.extend(receipt['unresolved'])
                    # Replay calibration on cache hits as well so --resume keeps
                    # exactly the same later batch boundaries and receipt keys.
                    for attempt in receipt['attempts'][-1:]:
                        usage = attempt['usage']
                        if all(type(usage.get(k)) is int and usage[k] >= 0 for k in ('prompt_tokens','completion_tokens')):
                            # Calibrate only a complete, unrepaired whole batch.
                            if complete and len(receipt['attempts']) == 1:
                                self.budget = bridge('calibrate', budget=self.budget, items=batch['items'],
                                    estimated_input=batch['input_tokens'], prompt_tokens=usage['prompt_tokens'],
                                    completion_tokens=usage['completion_tokens'])
            pending = [item for item in pending if item['id'] not in ids]
        return accepted, unresolved

    def run(self, tables, candidates, previous=None, decisions=None):
        previous = previous or {}
        identity = dict(model=self.config['model'], prompt_hash=hashlib.sha256(PROMPT.encode()).hexdigest())
        if previous.get('identity') != identity:
            previous = {}
        graph = bridge('graph', budget=self.budget, tables=tables, candidates=candidates)
        delta = bridge('incremental', tables=tables, graph=graph, previous=previous.get('snapshot', {}))
        work = delta['work']
        registry = copy.deepcopy(previous.get('registry', {'entities':{}}))
        accepted = copy.deepcopy(previous.get('accepted', {}))
        unresolved = set(previous.get('unresolved', []))
        inventory = {table['dataset']:table for table in tables}
        # Remove vanished semantic objects before presenting the registry to a model.
        for entity_id in work['removed_merge_items']:
            registry['entities'].pop(entity_id, None)
            accepted.pop(entity_id, None)
        for edge_id in work['removed_edges']:
            accepted.pop(edge_id, None)
        current_edges = {edge['id'] for edge in graph['edges']}
        accepted = {id_:value for id_,value in accepted.items()
                    if value['kind'] != 'relation' or id_ in current_edges}
        compact = bridge('registry', registry=registry, entities=[], inventory=sorted(inventory))['compact']

        def collect(items, context):
            values, missing = self.generate(items, context)
            for item in items:
                accepted.pop(item['id'], None)
                unresolved.discard(item['id'])
            accepted.update(values)
            unresolved.update(missing)
            return values

        def update_entities(values):
            nonlocal registry, compact
            entities = []
            for value in values.values():
                if value['kind'] != 'entity':
                    continue
                entity = {k:value[k] for k in ('id','canonical_name','aliases','main_table','related_tables')}
                entity['primary_key'] = inventory.get(entity['main_table'], {}).get('primary_key', [])
                if entity['main_table'] in graph['families']:
                    entity['related_tables'] = sorted(set(entity['related_tables']) | set(graph['families'][entity['main_table']]))
                entities.append(entity)
            result = bridge('registry', registry=registry, entities=entities, inventory=sorted(inventory))
            registry, compact = result['registry'], result['compact']

        owner = {name:c['id'] for c in graph['communities'] for rep in c['tables'] for name in graph['families'][rep]}
        for community in graph['communities']:
            if community['id'] not in work['communities']:
                continue
            items = [item for batch in community['batches'] for item in batch['items']]
            items.extend(dict(id=stable_id('entity',name), kind='entity', input=inventory[name]) for name in community['tables'])
            internal = [edge for edge in graph['edges'] if owner[edge['from']] == owner[edge['to']] == community['id']]
            items.extend(dict(id=edge['id'],kind='relation',input=edge) for edge in internal)
            values = collect(items, dict(pass_name='in_community', hubs=community['hub_signatures'], registry=compact))
            update_entities(values)
        cut_items = [
            dict(id=edge['id'],kind='relation',input=dict(edge=edge, endpoints=[
                            {'dataset':name,'primary_key':inventory[name].get('primary_key',[]),
                             'columns':[c for c in inventory[name]['columns'] if c['name'] in (edge['columns'][side] or [])]}
                            for side,name in [('from',edge['from']),('to',edge['to'])]]))
            for edge in graph['cut_edges'] if edge['id'] in work['cut_edges']]
        if cut_items:
            collect(cut_items, dict(pass_name='cut_edges'))
        merge_items = [dict(id=id_,kind='entity',input=registry['entities'][id_])
                       for id_ in work['merge_items'] if id_ in registry['entities']]
        if merge_items:
            values = collect(merge_items, dict(pass_name='merge', registry=compact,
                conflicts=bridge('registry',registry=registry,entities=[],inventory=sorted(inventory))['conflicts'],
                relationships=[{'id':e['id'],'from':e['from'],'to':e['to']}
                               for e in graph['edges'] if e['from'] in {i['input']['main_table'] for i in merge_items}
                               or e['to'] in {i['input']['main_table'] for i in merge_items}]))
            update_entities(values)
            for value in values.values():
                target = value.get('merge_into')
                if target and target != value['id']:
                    if target not in registry['entities']:
                        unresolved.add(value['id'])
                        continue
                    source = registry['entities'].pop(value['id'])
                    dest = registry['entities'][target]
                    dest['aliases'] = sorted(set(dest['aliases'] + source['aliases'] + [source['canonical_name']]))
                    dest['related_tables'] = sorted(set(dest['related_tables'] + source['related_tables'] + [source['main_table']]))
        inheritance = {}
        for representative,members in graph['families'].items():
            for member in members:
                if member == representative:
                    continue
                for kind,column in [('table',None)] + [('column',c['name']) for c in inventory[member]['columns']]:
                    source_id = stable_id(kind,representative,column)
                    target_id = stable_id(kind,member,column)
                    if source_id in accepted:
                        accepted[target_id] = dict(accepted[source_id],id=target_id,inherited_from=source_id)
                        inheritance[target_id] = source_id
                    else:
                        unresolved.add(target_id)
        current_ids = {stable_id('table',t) for t in inventory} | {
            stable_id('column',t,c['name']) for t in inventory for c in inventory[t]['columns']} | {
            stable_id('entity',t) for t in inventory} | current_edges
        accepted = {id_:v for id_,v in accepted.items() if id_ in current_ids}
        unresolved &= current_ids
        mapped = [id_ for id_,v in accepted.items() if v['kind']=='relation' and v.get('decision')=='proposed']
        rejections = {id_:v['reason'] for id_,v in accepted.items() if v['kind']=='relation' and v.get('decision')=='rejected'}
        # Core entity selection is explicit; absence reports zero reachability.
        review = bridge('review',graph=graph,registry=registry,mapped=mapped,rejections=rejections,
            core=self.config.get('core_entities',[]),orphan_threshold=self.config.get('orphan_threshold',0.2),
            decisions=decisions or {})
        if unresolved:
            review.update(g14='blocked', reason='selfbuild_unresolved_items')
        result = dict(protocol='oo.selfbuild.community-draft/v1',identity=identity,graph=graph,registry=registry,
            accepted=accepted,unresolved=sorted(unresolved),family_inheritance=inheritance,snapshot=delta['snapshot'],work=work,
            review=review,model_calls=self.calls,budget=self.budget,published=False,business_owner_signed=False)
        write_private(self.output/'result.json',result)
        return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--input', required=True)
    parser.add_argument('--config', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--resume')
    parser.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    document, config = json.loads(Path(args.input).read_text()), json.loads(Path(args.config).read_text())
    tables, candidates = document['tables'], document.get('relationship_candidates', [])
    runner = Runner(config,args.output)
    if not args.execute:
        graph = bridge('graph',budget=runner.budget,tables=tables,candidates=candidates)
        write_private(Path(args.output)/'preflight.json',dict(graph=graph,model_calls=0,
            capability_warning=None if runner.budget['capabilities']['probed'] else 'unprobed_conservative_defaults',
            note='community table/column estimate only; registry, relations and merge are re-budgeted at dispatch'))
    else:
        previous = json.loads(Path(args.resume).read_text()) if args.resume else None
        runner.run(tables,candidates,previous=previous)



def legacy_allocation(tables, context, capabilities=None):
    """Compatibility adapter for JSON S5; policy limits are inputs, not formulas.

    The historical route was provisioned for 272k/40k. New providers must pass
    their probe/override; the NDJSON runner defaults to conservative 32k/4k.
    Whole-JSON callers cannot repair column shards and fail closed instead.
    """
    capabilities = capabilities or dict(window=272000,max_output=40000,system_overhead=2500)
    batches = bridge('tables', tables=tables, context=context,
                     budget={'capabilities':capabilities})
    if len(batches) != 1:
        raise ValueError('s5_requires_ndjson_shards')
    return batches[0]

if __name__ == '__main__':
    main()
