#!/usr/bin/env python3
"""Autonomous model uses only the actual OO CLI. Credentials stay in private files."""
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
        return {**{k: body[k] for k in keys if k in body},
                'warnings': compact_warnings(body.get('warnings', []))}
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

def main():
    p=argparse.ArgumentParser(); p.add_argument('--config',required=True); p.add_argument('--out',required=True)
    a=p.parse_args(); cfg=json.loads(pathlib.Path(a.config).read_text()); out=pathlib.Path(a.out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    if cfg['model_origin'] != 'http://127.0.0.1:8317': raise ValueError('approved product origin required')
    bearer=private(cfg['model_bearer_file']); private(cfg['gateway_token_file'])
    truth=pathlib.Path(cfg['independent_truth_file']); gold=json.loads(truth.read_text())
    common=['--json','--gateway',cfg['gateway'],'--transport',cfg['transport'],'--auth',cfg['auth'],'--token-file',cfg['gateway_token_file']]
    if cfg.get('ca_file'): common += ['--ca-file',cfg['ca_file']]
    tools=[{'type':'function','function':{'name':name,'description':'Read actual role-bound OO Gateway '+name,
       'parameters':{'type':'object','properties':{k:{'type':v} for k,v in args.items()},'required':list(args),'additionalProperties':False}}} for name,args in TOOLS.items()]
    messages=[{'role':'system','content':'Use OO tools autonomously to inspect role knowledge, schema, freshness, validate and query. Preserve exact decimal and original source type/quality warnings. Never infer a value without evidence. Include query_id in factual answers. You cannot use direct DB or source credentials.'}]
    transcript=[]; query_ids=set(); used=set(); answers=[]; turn_history=[]
    for turn, question in enumerate(cfg['questions']):
        messages.append({'role':'user','content':question['prompt']})
        for step in range(cfg.get('max_tool_rounds',20)):
            payload={'model':'gpt-6.1-sol','reasoning_effort':'low','messages':messages,'tools':tools,'tool_choice':'auto'}
            req=urllib.request.Request(cfg['model_origin']+'/v1/chat/completions',data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+bearer})
            with urllib.request.urlopen(req,timeout=120) as response: model=json.load(response)
            msg=model['choices'][0]['message']; messages.append(msg); transcript.append({'turn':turn,'step':step,'model':model})
            (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
            calls=msg.get('tool_calls',[])
            if not calls:
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
                body=json.loads(proc.stdout); query_ids.update(ids(body)); used.add(name)
                view=model_view(name,body)
                transcript.append({'turn':turn,'tool':name,'arguments':args,'exit':proc.returncode,'response':body,'model_view':view})
                (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
                if proc.returncode and not (
                    name in {'validate', 'query'}
                    and body.get('class') == 'gateway'
                    and body.get('code') in {'COLUMN_NOT_ALLOWED', 'STATEMENT_NOT_ALLOWED', 'FUNCTION_NOT_ALLOWED'}
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
    verdict={'model':'gpt-6.1-sol','reasoning_effort':'low','turns':len(answers),'tools':sorted(used),'query_ids':sorted(query_ids),'audit_sha256':hashlib.sha256(audit_bytes).hexdigest(),'truth_sha256':hashlib.sha256(truth.read_bytes()).hexdigest(),'outcome':'verified'}
    (out/'verdict.json').write_text(json.dumps(verdict,indent=2)+'\n')
    print(json.dumps(verdict))

if __name__=='__main__': main()
