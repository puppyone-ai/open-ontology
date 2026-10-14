#!/usr/bin/env python3
"""Private connection → pinned source-owner recipe, with immutable recovery."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys

ENGINES = {'postgresql', 'mysql', 'oracle', 'mariadb', 'sqlserver', 'clickhouse', 'doris', 'starrocks', 'mongodb', 'opensearch'}
FORBIDDEN = {'password', 'passwd', 'secret', 'token', 'api_key', 'authorization'}

class Refused(ValueError): pass

def require(ok, token):
    if not ok: raise Refused(token)

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def private(path):
    p = Path(path)
    require(p.is_absolute(), 'private_path')
    m = p.lstat()
    require(stat.S_ISREG(m.st_mode) and m.st_uid == os.getuid() and not m.st_mode & 0o077, 'private_file')
    return p

def pairs(items):
    out = {}
    for k,v in items:
        require(k not in out, 'duplicate_key'); out[k] = v
    return out

def load(path):
    require(Path(path).stat().st_size <= 1048576, 'document_budget')
    return json.loads(Path(path).read_text(), object_pairs_hook=pairs)

def reject_secrets(value):
    if isinstance(value, dict):
        require(not FORBIDDEN.intersection(value), 'inline_secret')
        for x in value.values(): reject_secrets(x)
    elif isinstance(value, list):
        for x in value: reject_secrets(x)

def save(path, value):
    tmp = path.with_suffix('.tmp')
    with tmp.open('w') as f:
        os.chmod(tmp, 0o600); json.dump(value, f, indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
    os.replace(tmp, path)

def pointer(doc, key, value):
    require(key.startswith('/') and '~' not in key, 'config_pointer')
    keys = key[1:].split('/')
    for k in keys[:-1]:
        require(isinstance(doc, dict) and k in doc, 'config_pointer'); doc = doc[k]
    require(isinstance(doc, dict) and keys[-1] in doc, 'config_pointer')
    doc[keys[-1]] = value

def run(input_path, resume=False):
    source = private(input_path); spec = load(source); reject_secrets(spec)
    require(spec['protocol'] == 'oo.source-onboarding/v1', 'protocol')
    c = spec['connection']
    require(set(c) == {'type','database','host','port','user','secret_ref'}, 'connection_shape')
    require(c['type'] in ENGINES, 'engine')
    require(type(c['port']) is int and 1 <= c['port'] <= 65535, 'port')
    for key in ('database','host','user'):
        require(isinstance(c[key], str) and 0 < len(c[key]) <= 253 and not any(x in c[key] for x in '\r\n\0'), 'connection_value')
    require(re.fullmatch(r'secret://[A-Za-z0-9_./-]{1,128}', c['secret_ref']) is not None, 'secret_ref')
    recipe_path = private(spec['recipe']); recipe = load(recipe_path); reject_secrets(recipe)
    require(recipe['protocol'] == 'oo.source-onboarding.recipe/v1' and recipe['engine'] == c['type'], 'recipe_engine')
    mode = recipe.get('mode', 'whole_source')
    require(mode in {'whole_source', 'source_capture', 'metadata_sample'}, 'recipe_mode')
    if recipe.get('owned_fixture') is not True:
        require(mode == 'metadata_sample' and recipe.get('read_only') is True and type(recipe.get('max_rows')) is int and 1 <= recipe['max_rows'] <= 5, 'customer_full_import_not_authorized')
    registry = load(private(spec['secret_registry']))
    require(c['secret_ref'] in registry, 'secret_unresolved')
    password_file = private(registry[c['secret_ref']])
    pins = recipe['pins']
    require(pins and recipe['steps'], 'empty_recipe')
    for pin in pins:
        require(digest(pin['path']) == pin['sha256'], 'pin_drift')
    out = Path(spec['out']); require(out.is_absolute(), 'output_path')
    template_path = private(recipe['config_template'])
    identity = {'input_sha256':digest(source), 'recipe_sha256':digest(recipe_path), 'template_sha256':digest(template_path), 'pins':pins}
    if resume:
        require(out.is_dir() and not out.is_symlink(), 'resume_directory')
    else:
        out.mkdir(mode=0o700, parents=False, exist_ok=False)
    require(out.stat().st_uid == os.getuid() and not out.stat().st_mode & 0o077, 'run_permissions')
    with (out/'lock').open('a') as lock:
        os.chmod(out/'lock', 0o600); fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        journal_path = out/'journal.json'
        if resume:
            journal = load(journal_path); require(journal['identity'] == identity, 'resume_drift')
            require(digest(out/'driver-config.json') == journal['config_sha256'], 'resume_config_drift')
        else:
            template_path = private(recipe['config_template']); config = load(template_path)
            for p, field in recipe['connection_map'].items():
                require(field in c or field == 'password_file', 'connection_mapping')
                pointer(config, p, str(password_file) if field == 'password_file' else c[field])
            require(set(recipe['connection_map'].values()) >= {'host','port','database','user','password_file'}, 'connection_not_consumed')
            save(out/'driver-config.json', config)
            journal = {'protocol':'oo.source-onboarding.run/v1','identity':identity,'config_sha256':digest(out/'driver-config.json'),'state':'running','steps':[]}
            save(journal_path,journal)
        done = [x for x in journal['steps'] if x['state'] == 'succeeded']
        for record in done:
            for artifact in record['artifacts']:
                require(digest(out/artifact['path']) == artifact['sha256'], 'resume_artifact_drift')
        for index, step in enumerate(recipe['steps']):
            require(re.fullmatch(r'[a-z][a-z0-9_-]{0,63}', step['name']) is not None, 'step_name')
            if index < len(done):
                require(done[index]['name'] == step['name'], 'resume_step_drift'); continue
            attempt = len(journal['steps'])
            argv = [a.replace('{config}',str(out/'driver-config.json')).replace('{run}',str(out)) for a in step['argv']]
            require(argv and Path(argv[0]).is_absolute(), 'executable_path')
            require(any(p['path'] == argv[0] for p in pins), 'executable_not_pinned')
            record = {'name':step['name'],'state':'running','attempt':attempt,'artifacts':[]}
            journal['steps'].append(record); journal['state']='running'; save(journal_path,journal)
            with (out/f'{attempt}.stdout').open('xb') as stdout, (out/f'{attempt}.stderr').open('xb') as stderr:
                os.chmod(stdout.name,0o600); os.chmod(stderr.name,0o600)
                try:
                    deadline = step.get('deadline_seconds',1800)
                    require(type(deadline) is int and 1 <= deadline <= 86400, 'deadline')
                    child = subprocess.Popen(argv, stdout=stdout, stderr=stderr, cwd=out, start_new_session=True)
                    record['pid'] = child.pid; save(journal_path,journal)
                    try:
                        code = child.wait(timeout=deadline)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL); child.wait(); raise
                    record['exit_code']=code
                    require(code == 0, 'child_failed')
                    for item in step['artifacts']:
                        p = Path(item['path']); require(not p.is_absolute() and '..' not in p.parts, 'artifact_path')
                        artifact = out/p
                        require(artifact.is_file() and not artifact.is_symlink() and artifact.resolve().is_relative_to(out.resolve()), 'artifact_missing')
                        sha = digest(artifact)
                        require('sha256' not in item or item['sha256'] == sha, 'artifact_mismatch')
                        record['artifacts'].append({'path':str(p),'sha256':sha})
                    require(record['artifacts'], 'no_fresh_artifacts')
                    record['state']='succeeded'
                except (Refused, subprocess.TimeoutExpired, OSError):
                    record['state']='failed'; journal['state']='failed'; save(journal_path,journal); raise Refused('stage_failed')
            save(journal_path,journal)
        journal['state']='recipe_completed'; save(journal_path,journal)
        return {'protocol':journal['protocol'],'state':journal['state'],'steps':len(recipe['steps']),'mode':mode,'published':False}

def main():
    p=argparse.ArgumentParser();p.add_argument('--input',required=True);p.add_argument('--resume',action='store_true');a=p.parse_args()
    try: print(json.dumps(run(a.input,a.resume))); return 0
    except (Refused, OSError, KeyError, TypeError, json.JSONDecodeError):
        print('source-onboarding: refused',file=sys.stderr);return 2

if __name__ == '__main__': sys.exit(main())
