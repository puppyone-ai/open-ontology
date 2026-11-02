"""Read-only CLI preview of a sealed role Pack against the unchanged live Gateway.

Knowledge reads authenticate through the real CLI and pin its active manifest.
All data commands go to that CLI unchanged. This adapter never publishes or grants.
"""
import argparse
from difflib import get_close_matches
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()


def safe_path(path):
    return (isinstance(path, str) and 0 < len(path) <= 256 and not path.startswith('/')
            and '\\' not in path and '\0' not in path
            and all(part and part not in {'.', '..'} for part in path.split('/')))


def load_pack(path, expected_sha):
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 67_108_864:
        raise ValueError('PREVIEW_PACK_FILE')
    raw = path.read_bytes()
    if digest(raw) != expected_sha:
        raise ValueError('PREVIEW_PACK_CHANGED')
    value = json.loads(raw)
    if value.get('protocol') != 'oo.builder.role-pack/v1':
        raise ValueError('PREVIEW_PACK_SCHEMA')
    files, index = {}, []
    for item in value['files']:
        name, body = item['path'], item['text'].encode()
        if (not safe_path(name) or name in files or len(body) != item['bytes']
                or digest(body) != item['sha256']):
            raise ValueError('PREVIEW_PACK_SEAL')
        files[name] = item
        index.append({k: item[k] for k in ('bytes', 'path', 'sha256')})
    if (value['pack_digest'] != digest(canonical(sorted(index, key=lambda item: item['path'])))
            or value['entry_count'] != len(files)
            or value['total_bytes'] != sum(x['bytes'] for x in files.values())
            or files.get('manifest.json', {}).get('sha256') != value['manifest_sha256']):
        raise ValueError('PREVIEW_PACK_SEAL')
    return value, files, json.loads(files['manifest.json']['text'])


def check_scope(manifest, active, files, active_files):
    for field in ('project', 'role'):
        if manifest.get(field) != active.get(field):
            raise ValueError('PREVIEW_ROLE_BINDING')
    # Match the Gateway's column extraction from sealed table pages.
    def grants(entries):
        return {path[7:-3]: set(re.findall(r'^\s*\|\s*`([^`|\n]+)`', item['text'], re.M))
                for path, item in entries.items() if path.startswith('tables/') and path.endswith('.md')}
    before, after = grants(active_files), grants(files)
    if any(table not in before or not columns <= before[table] for table, columns in after.items()):
        raise ValueError('PREVIEW_WOULD_EXPAND_SCOPE')


def parse_argv(argv):
    if argv[:2] not in (['knowledge', 'cat'], ['knowledge', 'search']):
        return None
    options, positional, index = {}, [], 2
    valued = {'--gateway', '--transport', '--auth', '--token-file', '--ca-file', '--client-pem',
              '--path-prefix', '--offset', '--limit'}
    while index < len(argv):
        arg = argv[index]
        if arg == '--':
            positional.extend(argv[index + 1:])
            break
        if arg == '--json':
            options[arg] = True
        elif arg in valued:
            index += 1
            if index >= len(argv) or arg in options:
                raise ValueError('PREVIEW_ARGUMENTS')
            options[arg] = argv[index]
        elif arg.startswith('-'):
            raise ValueError('PREVIEW_ARGUMENTS')
        else:
            positional.append(arg)
        index += 1
    if len(positional) != 1 or not options.get('--json'):
        raise ValueError('PREVIEW_JSON_REQUIRED')
    auth_argv = ['knowledge', 'cat', 'manifest.json', '--json']
    for flag in ('--gateway', '--transport', '--auth', '--token-file', '--ca-file', '--client-pem'):
        if flag in options:
            auth_argv += [flag, options[flag]]
    return argv[1], positional[0], options, auth_argv


def search(files, needle, options):
    if not needle or len(needle.encode()) > 200 or '\0' in needle:
        raise ValueError('PREVIEW_SEARCH')
    offset, limit = int(options.get('--offset', '0')), int(options.get('--limit', '20'))
    prefix = options.get('--path-prefix', '')
    if (not 0 <= offset <= 1_048_576 or not 1 <= limit <= 100
            or prefix and not safe_path(prefix.removesuffix('/'))):
        raise ValueError('PREVIEW_SEARCH')
    found = []
    for path, item in files.items():
        body = item['text']
        if not path.startswith(prefix) or needle not in path and needle not in body:
            continue
        heading = body.split('\n', 1)[0]
        score = (0 if path == needle else 1 if path.rsplit('/', 1)[-1].removesuffix('.md') == needle
                 else 2 if needle in heading else 3 if needle in path else 4)
        at = max(body.find(needle), 0)
        found.append((score, path, {'path': path, 'sha256': item['sha256'],
                                   'excerpt': body[max(0, at - 40):at + len(needle) + 40]}))
    found.sort(key=lambda row: (row[0], row[1]))
    end = min(offset + limit, len(found))
    return {'hits': [item for _, _, item in found[offset:end]], 'total': len(found),
            'offset': offset, 'limit': limit, 'has_more': end < len(found),
            'next_offset': end if end < len(found) else None}


def execute(config, argv, invoke=subprocess.run):
    parsed = parse_argv(argv)
    if parsed is None:
        return invoke(config['cli_command'] + argv, capture_output=True, timeout=180)
    operation, argument, options, auth_argv = parsed
    # Authentication and revocation remain upstream decisions, on every knowledge read.
    checked = invoke(config['cli_command'] + auth_argv, capture_output=True, timeout=45)
    if checked.returncode != 0:
        return checked
    active_body = json.loads(checked.stdout)
    if active_body.get('error') or active_body.get('code'):
        return subprocess.CompletedProcess([], 2, checked.stdout, checked.stderr)
    if (digest(active_body['text'].encode()) != config['active_manifest_sha256']
            or active_body['sha256'] != config['active_manifest_sha256']):
        raise ValueError('PREVIEW_ACTIVE_PACK_CHANGED')
    active = json.loads(active_body['text'])
    _, active_files, active_manifest = load_pack(config['active_pack_file'], config['active_pack_sha256'])
    if active_files['manifest.json']['sha256'] != config['active_manifest_sha256'] or active_manifest != active:
        raise ValueError('PREVIEW_ACTIVE_PACK_CHANGED')
    artifact, files, manifest = load_pack(config['pack_file'], config['pack_sha256'])
    check_scope(manifest, active, files, active_files)
    if operation == 'cat':
        if not safe_path(argument):
            raise ValueError('PREVIEW_PATH_ABSENT')
        if argument not in files:
            failure = {'code': 'PREVIEW_PATH_ABSENT', 'publication': 'preview',
                       'suggested_paths': get_close_matches(argument, files, n=3, cutoff=0.6),
                       'hint': 'Use an exact path returned by knowledge_search; do not append commentary or punctuation.'}
            return subprocess.CompletedProcess([], 2, canonical(failure) + b'\n', b'')
        output = dict(files[argument])
    else:
        output = search(files, argument, options)
    output.update(knowledge_version=manifest['knowledge']['version'],
                  policy_version=manifest['policy']['version'],
                  publication='preview',
                  knowledge_preview={
                      'pack_digest': artifact['pack_digest'],
                      'active_manifest_sha256': config['active_manifest_sha256'],
                      'active_knowledge_version': active_body['knowledge_version'],
                      'active_policy_version': active_body['policy_version'],
                      'data_query_route': 'unchanged_live_gateway',
                      'grants_changed': False,
                      'business_readiness_verified': False})
    return subprocess.CompletedProcess([], 0, canonical(output) + b'\n', b'')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('args', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        config_path = Path(args.config)
        if config_path.is_symlink() or config_path.stat().st_mode & 0o077:
            raise ValueError('PREVIEW_CONFIG_PRIVATE')
        config = json.loads(config_path.read_text())
        result = execute(config, args.args)
        sys.stdout.buffer.write(result.stdout)
        sys.stderr.buffer.write(result.stderr)
        return result.returncode
    except (ValueError, KeyError, OSError, subprocess.TimeoutExpired) as error:
        code = str(error)
        if not code.startswith('PREVIEW_') or len(code) > 80:
            code = 'PREVIEW_FAILED'
        print(json.dumps({'code': code, 'publication': 'preview', 'grants_changed': False}))
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
