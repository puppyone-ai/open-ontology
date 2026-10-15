#!/usr/bin/env python3
"""Owned fixture CLI alias follows only fully published, preflighted Gateway cuts."""
import argparse
import json
import os
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--runtime', required=True)
    options, args = parser.parse_known_args()
    runtime = Path(options.runtime)
    assert not runtime.is_symlink() and runtime.stat().st_uid == os.getuid() and runtime.stat().st_mode & 0o077 == 0
    config = json.loads(runtime.read_bytes())
    paths = [Path(config['initial_ready']), *Path(config['follow_root']).glob('generation-*/gateway-ready.json')]
    ready = []
    for path in paths:
        item = json.loads(path.read_bytes())
        if item.get('ready') is True and item.get('role') == config['role']:
            ready.append(item)
    current = max(ready, key=lambda item: item['source_generation'])
    private = Path(current['private_root'])
    address = (private/'listen.addr').read_text().strip()
    assert address.startswith('127.0.0.1:')
    defaults = {'--auth': 'external', '--token-file': '/run-current/current-client.token', '--gateway': 'http://'+address, '--transport': 'local-development'}
    for flag, value in defaults.items():
        if not any(arg == flag or arg.startswith(flag+'=') for arg in args):
            args += [flag, value]
    os.execvp('docker', ['docker', 'exec', current['gateway_container'], config['cli_binary_container'], *args])


if __name__ == '__main__':
    main()
