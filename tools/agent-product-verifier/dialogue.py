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
    if body.get('class') and body.get('exit'):
        return body
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

def verify_named_facts(answer, expected, results):
    report = json.loads(answer)
    if not isinstance(report, dict) or set(report) != {'facts', 'evidence'}:
        raise ValueError('named factual answer must contain only facts and evidence')
    if not isinstance(report['facts'], dict) or not isinstance(report['evidence'], dict):
        raise ValueError('facts and evidence must be objects keyed by fact name')
    if set(report['facts']) != set(expected) or set(report['evidence']) != set(expected):
        raise ValueError('named factual answer or evidence is incomplete')
    for name, value in expected.items():
        claimed = report['facts'][name]
        if isinstance(claimed, dict): claimed = claimed.get('value')
        if isinstance(claimed, (bool, float)) or str(claimed) != str(value):
            raise ValueError('named fact differs from independent truth: ' + name)
        link = report['evidence'][name]
        body = results[link['query_id']]
        names = [c['name'] for c in body['columns']]
        if names.count(link['column']) != 1:
            raise ValueError('fact column is not uniquely present')
        row = link.get('row', 0)
        if type(row) is not int or row < 0:
            raise ValueError('invalid fact row')
        if str(body['rows'][row][names.index(link['column'])]) != str(value):
            raise ValueError('fact is not grounded in successful query result')


def main():
    p=argparse.ArgumentParser(); p.add_argument('--config',required=True); p.add_argument('--out',required=True)
    a=p.parse_args(); cfg=json.loads(pathlib.Path(a.config).read_text()); out=pathlib.Path(a.out)
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
    transcript=[]; query_ids=set(); used=set(); answers=[]; turn_history=[]
    if cfg.get('resume_transcript'):
        data=pathlib.Path(cfg['resume_transcript']).read_bytes()
        if hashlib.sha256(data).hexdigest() != cfg['resume_transcript_sha256']:
            raise ValueError('resume transcript digest mismatch')
        transcript=json.loads(data)
        for event in transcript:
            if 'tool' in event and event['exit'] == 0:
                used.add(event['tool'])
                if event['tool'] == 'query': query_ids.update(ids(event['response']))
            if 'model' not in event: continue
            msg=event['model']['choices'][0]['message']
            if msg.get('tool_calls'): continue
            turn=event['turn']
            if turn != len(answers): raise ValueError('resume must contain consecutive completed turns')
            answer=msg.get('content') or ''
            expected=gold[cfg['questions'][turn]['truth_key']]
            results={e['response']['query_id']:e['response'] for e in transcript
                     if e.get('turn') == turn and e.get('tool') == 'query' and e['exit'] == 0}
            if 'expected_named_facts' in expected:
                verify_named_facts(answer, expected['expected_named_facts'], results)
            else:
                raise ValueError('resume requires query-grounded named facts')
            answers.append(answer)
            turn_history.extend(({'role':'user','content':cfg['questions'][turn]['prompt']},
                                 {'role':'assistant','content':answer}))
        if not answers or any(e['turn'] >= len(answers) for e in transcript):
            raise ValueError('resume must end in a verified completed turn')
        messages=[messages[0], *turn_history]
    for turn, question in enumerate(cfg['questions']):
        if turn < len(answers): continue
        expected = gold[question['truth_key']]
        prompt = question['prompt']
        if 'expected_named_facts' in expected:
            prompt += '\nReturn JSON with facts and evidence only (these facts are your answer). facts keys: ' + ', '.join(expected['expected_named_facts']) + '. evidence must be an object keyed by fact name, with each value containing query_id, column and zero-based row from an actual successful query. Use bare JSON, no Markdown fences; exact string values or objects containing value and type/freshness metadata.'
        messages.append({'role':'user','content':prompt})
        turn_results = {}
        for step in range(cfg.get('max_tool_rounds',20)):
            choice = 'required' if step == 0 else 'auto'
            if 'expected_named_facts' in expected and step > 0 and not turn_results:
                choice = {'type': 'function', 'function': {'name': 'query'}}
            payload={'model':model_name,'reasoning_effort':'low','messages':messages,'tools':tools,'tool_choice':choice}
            req=urllib.request.Request(cfg['model_origin']+'/v1/chat/completions',data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+bearer})
            with urllib.request.urlopen(req,timeout=120) as response: model=json.load(response)
            msg=model['choices'][0]['message']; messages.append(msg); transcript.append({'turn':turn,'step':step,'model':model})
            (out/'transcript.json').write_text(json.dumps(transcript,indent=2)+'\n')
            calls=msg.get('tool_calls',[])
            if not calls:
                if step == 0: raise ValueError('model answered without actual tools in this turn')
                answer=msg.get('content') or ''; answers.append(answer)
                if 'expected_named_facts' in expected:
                    verify_named_facts(answer, expected['expected_named_facts'], turn_results)
                elif not all(str(x) in answer for x in expected['expected_exact_facts']):
                    raise ValueError('answer differs from independent truth')
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
                body=json.loads(proc.stdout)
                if name == 'query' and proc.returncode == 0: query_ids.update(ids(body))
                if proc.returncode == 0:
                    used.add(name)
                    if name == 'query': turn_results[body['query_id']] = body
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
