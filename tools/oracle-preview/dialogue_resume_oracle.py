#!/usr/bin/env python3
"""Resume Oracle's one Grok dialogue after its actual non-public Pack path call."""
import argparse, hashlib, json, os, pathlib, stat, subprocess, urllib.request

TOOLS = {
    'tables': {}, 'describe': {'table':'string'}, 'knowledge_cat': {'path':'string'},
    'knowledge_search': {'query':'string'}, 'validate': {'sql':'string'},
    'query': {'sql':'string'}, 'freshness': {},
}

def model_view(name, body):
    """Keep source results in the transcript; send only decision facts to the model."""
    if name == 'freshness':
        keys = ('knowledge_version', 'policy_version', 'publication', 'capture_method',
                'deletion_guarantee', 'replica_lag', 'freshness', 'sources', 'quality_counts')
        return {**{k: body[k] for k in keys if k in body},
                'current_bindings': [m for m in body.get('mappings', []) if m.get('bound') is True],
                'warnings': compact_warnings(body.get('warnings', []))}
    if name == 'query':
        keys = ('columns', 'rows', 'returned_rows', 'truncated', 'truncation_reasons',
                'query_id', 'knowledge_version', 'policy_version', 'sources', 'freshness', 'hints')
        view = {**{k: body[k] for k in keys if k in body},
                'warnings': compact_warnings(body.get('warnings', []))}
        names = [column.get('name') for column in body.get('columns', [])]
        rows = body.get('rows', [])
        if names and all(isinstance(name, str) for name in names) and len(set(names)) == len(names) and all(len(row) == len(names) for row in rows):
            view['rows'] = [dict(zip(names, row)) for row in rows]
        return view
    return body

def compact_warnings(warnings):
    result = []
    for warning in warnings:
        if warning.get('code') == 'STALE_DATA':
            result.append({'code': 'STALE_DATA',
                           'message': 'Source freshness is not established; use sources, publication and quality fields. Full provenance remains in the raw tool response.'})
        else:
            result.append(warning)
    return result

def private(path):
    p=pathlib.Path(path); s=p.lstat()
    if not stat.S_ISREG(s.st_mode) or s.st_uid != os.getuid() or s.st_mode & 0o077:
        raise ValueError('private regular file with owner-only access required')
    return p.read_text().strip()

def ids(value):
    found=set()
    if isinstance(value, dict):
        if isinstance(value.get('query_id'), str): found.add(value['query_id'])
        for item in value.values(): found.update(ids(item))
    elif isinstance(value, list):
        for item in value: found.update(ids(item))
    return found

def pack_followup(a):
    """One Oracle-only continuation after the four already verified answers."""
    cfg=json.loads(pathlib.Path(a.config).read_text()); out=pathlib.Path(a.out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    if cfg['model_origin']!='http://127.0.0.1:8317' or cfg['model']!='grok-4.7':
        raise ValueError('original approved dialogue transport/model required')
    bearer=private(cfg['model_bearer_file']); private(cfg['gateway_token_file'])
    prior_bytes=pathlib.Path(a.partial).read_bytes()
    if hashlib.sha256(prior_bytes).hexdigest()!=a.partial_sha256: raise ValueError('original transcript changed')
    prior=json.loads(prior_bytes)
    if len(prior)!=38 or [x.get('turn') for x in prior if 'model' in x][-1]!=3:
        raise ValueError('expected completed four-turn Oracle transcript')
    gold=json.loads(pathlib.Path(cfg['independent_truth_file']).read_text())
    answers=[]
    for turn,question in enumerate(cfg['questions']):
        found=[x['model']['choices'][0]['message'] for x in prior if x.get('turn')==turn and 'model' in x and not x['model']['choices'][0]['message'].get('tool_calls')]
        if len(found)!=1: raise ValueError('original answer count changed')
        answer=found[0].get('content') or ''
        if not all(str(v) in answer for v in gold[question['truth_key']]['expected_exact_facts']):
            raise ValueError('original gold answer changed')
        answers.append(answer)
    pack=json.loads(pathlib.Path(a.role_pack).read_text())
    public={f['path']:f['sha256'] for f in pack['files'] if f['path'].startswith('tables/') and f['path'].endswith('.md')}
    if len(public)!=3 or pack.get('pack_digest')!='04377a13dddd44b62ac031791294853169286439dc3830f728d536714e0e1313':
        raise ValueError('expected original three-table role pack')
    common=['--json','--gateway',cfg['gateway'],'--transport',cfg['transport'],'--auth',cfg['auth'],'--token-file',cfg['gateway_token_file']]
    # This is entry metadata provided by the operator, not model discovery or an answer.
    prompt=('The operator supplies these installed public Pack entry paths (knowledge_version 1): '
            +', '.join(sorted(public))+'. Read the relevant actual Pack documents with knowledge_cat. '
            'Explain what their amount, date, and timezone-related column type/semantic labels do and do not establish. '
            'Use only the retrieved document content; do not run SQL or infer new data values.')
    messages=[{'role':'system','content':'You are continuing the same Oracle OO Gateway dialogue. Use the actual public OO tool before answering. Do not invent Pack contents or data values.'}]
    for question,answer in zip(cfg['questions'],answers):
        messages += [{'role':'user','content':question['prompt']},{'role':'assistant','content':answer}]
    messages.append({'role':'user','content':prompt})
    tool=[{'type':'function','function':{'name':'knowledge_cat','description':'Read one installed public relative Pack document path from the operator entry list',
           'parameters':{'type':'object','properties':{'path':{'type':'string'}},'required':['path'],'additionalProperties':False}}}]
    transcript=[]; read=set(); response_models=set(); usage=[]
    for step in range(5):
        payload={'model':'grok-4.7','reasoning_effort':'low','messages':messages,'tools':tool,'tool_choice':'required' if step==0 else 'auto'}
        req=urllib.request.Request(cfg['model_origin']+'/v1/chat/completions',data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+bearer})
        with urllib.request.urlopen(req,timeout=120) as response: model=json.load(response)
        response_models.add(model.get('model')); usage.append(model.get('usage'))
        msg=model['choices'][0]['message']; messages.append(msg)
        transcript.append({'step':step,'model':model}); (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
        calls=msg.get('tool_calls') or []
        if not calls:
            if step==0 or not read: raise ValueError('no actual Pack document consumed')
            answer=msg.get('content') or ''
            if not answer: raise ValueError('missing Pack explanation')
            result={'outcome':'pack_document_read','prior_transcript_sha256':a.partial_sha256,'prior_four_gold_answers':True,
                    'operator_supplied_entry_paths':sorted(public),'autonomous_document_paths':sorted(read),
                    'response_models':sorted(response_models),'usage':usage,'answer':answer}
            (out/'receipt.json').write_text(json.dumps(result,indent=2)+'\n'); print(json.dumps(result)); return
        for call in calls:
            if call['function']['name']!='knowledge_cat': raise ValueError('unexpected tool')
            args=json.loads(call['function']['arguments']); path=args.get('path')
            if set(args)!={'path'} or path not in public: raise ValueError('document path outside supplied public pack')
            proc=subprocess.run([cfg['oo_cli'],'knowledge','cat',path,*common],capture_output=True,text=True,timeout=45)
            body=json.loads(proc.stdout)
            transcript.append({'step':step,'tool':'knowledge_cat','arguments':args,'exit':proc.returncode,'response':body})
            (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
            if proc.returncode or body.get('sha256')!=public[path] or body.get('knowledge_version')!='1' or body.get('policy_version')!='1':
                raise ValueError('Pack document/version mismatch')
            read.add(path)
            messages.append({'role':'tool','tool_call_id':call['id'],'content':json.dumps({'exit':0,'response':body})})
    raise ValueError('follow-up exhausted bounded tool rounds')

def main():
    p=argparse.ArgumentParser(); p.add_argument('--config',required=True); p.add_argument('--out',required=True)
    p.add_argument('--partial',required=True); p.add_argument('--partial-sha256',required=True)
    p.add_argument('--role-pack'); a=p.parse_args()
    if a.role_pack: return pack_followup(a)
    cfg=json.loads(pathlib.Path(a.config).read_text()); out=pathlib.Path(a.out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    if cfg['model_origin'] != 'http://127.0.0.1:8317': raise ValueError('approved product origin required')
    model_name=cfg.get('model','gpt-6.1-sol')
    if model_name not in {'gpt-6.1-sol', 'grok-4.7'}: raise ValueError('approved dialogue model required')
    bearer=private(cfg['model_bearer_file']); private(cfg['gateway_token_file'])
    truth=pathlib.Path(cfg['independent_truth_file']); gold=json.loads(truth.read_text())
    common=['--json','--gateway',cfg['gateway'],'--transport',cfg['transport'],'--auth',cfg['auth'],'--token-file',cfg['gateway_token_file']]
    if cfg.get('ca_file'): common += ['--ca-file',cfg['ca_file']]
    tools=[{'type':'function','function':{'name':name,'description':('Read an installed relative Pack file path returned by knowledge_search; never use / or a guessed path' if name == 'knowledge_cat' else 'Describe an exact schema.table name returned by tables' if name == 'describe' else 'Read actual role-bound OO Gateway '+name),
       'parameters':{'type':'object','properties':{k:{'type':v} for k,v in args.items()},'required':list(args),'additionalProperties':False}}} for name,args in TOOLS.items()]
    messages=[{'role':'system','content':'Use OO tools autonomously to inspect role knowledge, schema, freshness, validate and query. Preserve exact decimal and original source type/quality warnings. Never infer a value without evidence. Include query_id in factual answers. You cannot use direct DB or source credentials.'}]
    partial=pathlib.Path(a.partial).read_bytes()
    if hashlib.sha256(partial).hexdigest()!=a.partial_sha256: raise ValueError('partial transcript changed')
    previous=json.loads(partial)
    if len(previous)!=9 or [(previous[i].get('turn'),previous[i].get('step')) for i in (0,4)]!=[(0,0),(0,1)]:
        raise ValueError('expected the one Oracle Grok turn-zero failure')
    if [previous[i].get('tool') for i in (1,2,3,5,6,7,8)]!=[
        'tables','knowledge_search','freshness','describe','describe','describe','knowledge_cat']:
        raise ValueError('partial tool order changed')
    if previous[8].get('exit')!=2 or previous[8].get('response',{}).get('message')!='path is not public':
        raise ValueError('expected actual Pack-path refusal')
    transcript=[]; query_ids=set(); used=set(); answers=[]; turn_history=[]
    for turn, question in enumerate(cfg['questions']):
        messages.append({'role':'user','content':question['prompt']})
        first_step=0
        if turn==0:
            for model_index, tool_indexes in ((0,(1,2,3)),(4,(5,6,7,8))):
                model=previous[model_index]['model']
                if model.get('model') not in {'grok-4.7','grok-4.7-build'}: raise ValueError('not the Grok response')
                msg=model['choices'][0]['message']; calls=msg.get('tool_calls') or []
                if len(calls)!=len(tool_indexes): raise ValueError('partial call count changed')
                messages.append(msg); transcript.append(previous[model_index])
                for call, index in zip(calls,tool_indexes):
                    item=previous[index]; name=call['function']['name']; args=json.loads(call['function']['arguments'])
                    if name!=item['tool'] or args!=item['arguments'] or name not in TOOLS:
                        raise ValueError('partial call changed')
                    cmd={'tables':['tables'], 'knowledge_search':['knowledge','search',args.get('query','')],
                         'freshness':['freshness'], 'describe':['describe',args.get('table','')],
                         'knowledge_cat':['knowledge','cat',args.get('path','')]}[name]
                    proc=subprocess.run([cfg['oo_cli']]+cmd+common,capture_output=True,text=True,timeout=45)
                    body=json.loads(proc.stdout)
                    if proc.returncode!=item['exit']: raise ValueError('replayed public CLI exit changed')
                    if proc.returncode and (name!='knowledge_cat' or body.get('class')!='usage'
                                           or body.get('message')!='path is not public'):
                        raise ValueError('replayed public CLI refusal changed')
                    query_ids.update(ids(body))
                    if proc.returncode==0: used.add(name)
                    view=model_view(name,body)
                    transcript.append({'turn':0,'tool':name,'arguments':args,'exit':proc.returncode,
                                       'response':body,'model_view':view,'replayed_from_partial':a.partial_sha256})
                    messages.append({'role':'tool','tool_call_id':call['id'],
                                     'content':json.dumps({'exit':proc.returncode,'response':view})})
            (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
            first_step=2
        for step in range(first_step,cfg.get('max_tool_rounds',20)):
            payload={'model':model_name,'reasoning_effort':'low','messages':messages,'tools':tools,'tool_choice':'required' if step == 0 else 'auto'}
            req=urllib.request.Request(cfg['model_origin']+'/v1/chat/completions',data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+bearer})
            with urllib.request.urlopen(req,timeout=120) as response: model=json.load(response)
            msg=model['choices'][0]['message']; messages.append(msg); transcript.append({'turn':turn,'step':step,'model':model})
            (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
            calls=msg.get('tool_calls',[])
            if not calls:
                if step == 0: raise ValueError('model answered without actual tools in this turn')
                answer=msg.get('content') or ''; answers.append(answer)
                if not all(str(x) in answer for x in gold[question['truth_key']]['expected_exact_facts']): raise ValueError('answer differs from independent truth')
                turn_history.extend(({'role':'user','content':question['prompt']}, {'role':'assistant','content':answer}))
                messages=[messages[0], *turn_history]
                break
            for call in calls:
                name=call['function']['name']; args=json.loads(call['function']['arguments'])
                if name not in TOOLS or set(args)!=set(TOOLS[name]): raise ValueError('unknown tool or argument')
                cmd={'knowledge_cat':['knowledge','cat',args.get('path','')], 'knowledge_search':['knowledge','search',args.get('query','')],
                     'query':['query',args.get('sql',''),'--max-rows','100','--timeout-ms','10000'],
                     'describe':['describe',args.get('table','')], 'validate':['validate',args.get('sql','')], 'tables':['tables'],'freshness':['freshness']}[name]
                proc=subprocess.run([cfg['oo_cli']]+cmd+common,capture_output=True,text=True,timeout=45)
                body=json.loads(proc.stdout); query_ids.update(ids(body))
                if proc.returncode == 0: used.add(name)
                view=model_view(name,body)
                transcript.append({'turn':turn,'tool':name,'arguments':args,'exit':proc.returncode,'response':body,'model_view':view})
                (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
                if proc.returncode and not (
                    (name in {'validate', 'query'}
                     and body.get('class') == 'gateway'
                     and body.get('code') in {'COLUMN_NOT_ALLOWED', 'STATEMENT_NOT_ALLOWED', 'FUNCTION_NOT_ALLOWED'})
                    or (body.get('class') == 'usage' and (
                        (name == 'describe' and body.get('message') == 'table is not public')
                        or (name == 'knowledge_cat' and body.get('message') == 'path is not public')))
                ):
                    raise ValueError('OO CLI tool failed: '+name)
                messages.append({'role':'tool','tool_call_id':call['id'],'content':json.dumps({'exit':proc.returncode,'response':view})})
        else: raise ValueError('model exhausted tool rounds')
    (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
    required={'query','freshness','describe','knowledge_cat'}
    if not required <= used or not query_ids: raise ValueError('missing autonomous public tool coverage')
    audit=pathlib.Path(cfg['independent_gateway_audit_file']); audit_ids=set()
    audit_files=sorted(audit.glob('*-complete.wal')) if audit.is_dir() else [audit]
    if not audit_files: raise ValueError('Gateway completion audit is absent')
    audit_bytes=b''
    for path in audit_files:
        data=path.read_bytes(); audit_bytes+=path.name.encode()+b'\0'+data
        for line in data.splitlines(): audit_ids.update(ids(json.loads(line)))
    if not query_ids <= audit_ids: raise ValueError('query_id not present in independent Gateway audit')
    actual_models=sorted({item['model']['model'] for item in transcript if 'model' in item})
    verdict={'model':model_name,'response_models':actual_models,'reasoning_effort':'low','turns':len(answers),'tools':sorted(used),'query_ids':sorted(query_ids),'audit_sha256':hashlib.sha256(audit_bytes).hexdigest(),'truth_sha256':hashlib.sha256(truth.read_bytes()).hexdigest(),'outcome':'verified'}
    (out/'verdict.json').write_text(json.dumps(verdict,indent=2)+'\n')
    print(json.dumps(verdict))

if __name__=='__main__': main()
