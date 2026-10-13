#!/usr/bin/env python3
"""One bounded public-OO follow-up to correct a prior source-liveness claim."""
import argparse
import hashlib
import json
import os
import pathlib
import stat
import subprocess
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[3]
CLI = ROOT / 'tools/mongodb-preview/scripts/oo-gateway-cli.py'


def private_text(path):
    path = pathlib.Path(path)
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ValueError('private owner-only regular file required')
    return path.read_text().strip()


def save(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    config = json.loads(pathlib.Path(args.config).read_text())
    if config['model_origin'] != 'http://127.0.0.1:8317' or config['model'] != 'grok-4.7':
        raise ValueError('fixed approved model endpoint required')
    previous = pathlib.Path(config['previous_transcript'])
    if hashlib.sha256(previous.read_bytes()).hexdigest() != config['previous_transcript_sha256']:
        raise ValueError('previous transcript changed')
    events = json.loads(previous.read_text())
    questions = config['questions']
    if len(questions) != 3 or sorted({event['turn'] for event in events}) != [0, 1, 2]:
        raise ValueError('three completed turns required')
    prior = []
    for turn, question in enumerate(questions):
        answer = next((event['model']['choices'][0]['message'].get('content')
                       for event in reversed(events)
                       if event['turn'] == turn and 'model' in event
                       and event['model']['choices'][0]['message'].get('content')), None)
        if not answer:
            raise ValueError('prior answer absent')
        prior.extend([{'role': 'user', 'content': question['prompt']},
                      {'role': 'assistant', 'content': answer}])
    bearer = private_text(config['model_bearer_file'])
    private_text(config['gateway_token_file'])
    common = ['--json', '--auth', 'external', '--token-file', config['gateway_token_file'],
              '--gateway', config['gateway'], '--transport', 'local-development']
    describe = subprocess.run([str(CLI), 'describe', 'oo_w141.shipments', *common],
                              capture_output=True, text=True, timeout=30, check=True)
    if json.loads(describe.stdout)['pack_digest'] != config['pack_digest']:
        raise ValueError('role Pack changed')
    out = pathlib.Path(args.out)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    user = ('In your preceding answer you said "The source is not live". Which public OO tool '
            'result proves that the source process is stopped? Re-read freshness using the tool, '
            'then correct the claim. Distinguish publication and freshness status from whether the '
            'source process is running. If the public tool gives no process-liveness evidence, say '
            'explicitly that you cannot determine that from OO. Preserve the target-queryability '
            'and freshness caveats supported by the tools; do not infer process state from not_ready '
            'or STALE_DATA.')
    messages = [{'role': 'system', 'content': 'Use actual public OO evidence. Correct unsupported claims plainly. Do not use direct source or database credentials.'},
                *prior, {'role': 'user', 'content': user}]
    tool = {'type': 'function', 'function': {'name': 'freshness',
            'description': 'Read current public OO Gateway freshness for this role.',
            'parameters': {'type': 'object', 'properties': {}, 'required': [],
                           'additionalProperties': False}}}
    evidence = {'protocol': 'oo.mongodb.identity-correction/v1',
                'previous_transcript_sha256': config['previous_transcript_sha256'],
                'pack_digest': config['pack_digest'], 'model': config['model'],
                'responses': [], 'tool': None, 'answer': None}
    save(out / 'transcript.json', evidence)

    def call(choice):
        payload = {'model': config['model'], 'reasoning_effort': 'low',
                   'messages': messages, 'tools': [tool], 'tool_choice': choice}
        request = urllib.request.Request(
            config['model_origin'] + '/v1/chat/completions',
            data=json.dumps(payload).encode(),
            headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + bearer})
        with urllib.request.urlopen(request, timeout=120) as response:
            result = json.load(response)
        evidence['responses'].append(result)
        save(out / 'transcript.json', evidence)
        return result['choices'][0]['message']

    first = call({'type': 'function', 'function': {'name': 'freshness'}})
    calls = first.get('tool_calls') or []
    if len(calls) != 1 or calls[0]['function']['name'] != 'freshness' or json.loads(calls[0]['function']['arguments']) != {}:
        raise ValueError('one actual freshness tool call required')
    result = subprocess.run([str(CLI), 'freshness', *common], capture_output=True,
                            text=True, timeout=30, check=True)
    body = json.loads(result.stdout)
    evidence['tool'] = {'name': 'freshness', 'exit': 0, 'response': body}
    save(out / 'transcript.json', evidence)
    messages.append(first)
    messages.append({'role': 'tool', 'tool_call_id': calls[0]['id'],
                     'content': json.dumps(body, sort_keys=True)})
    second = call('none')
    if second.get('tool_calls'):
        raise ValueError('second response must be an answer')
    answer = second.get('content') or ''
    evidence['answer'] = answer
    save(out / 'transcript.json', evidence)
    print(json.dumps({'transcript_sha256': hashlib.sha256((out / 'transcript.json').read_bytes()).hexdigest(),
                      'response_ids': [item['id'] for item in evidence['responses']],
                      'usage': {key: sum(item['usage'][key] for item in evidence['responses'])
                                for key in ('prompt_tokens', 'completion_tokens', 'total_tokens')},
                      'answer': answer}))


if __name__ == '__main__':
    main()
