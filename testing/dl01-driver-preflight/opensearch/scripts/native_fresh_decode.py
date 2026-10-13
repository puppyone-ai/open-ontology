#!/usr/bin/env python3
"""Independent decoder prepares full typed expected cells, never target success."""
import hashlib
import importlib.util
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('target', HERE / 'native_fresh_target.py')
t = importlib.util.module_from_spec(spec)
spec.loader.exec_module(t)


def main():
    elf = t.BASE.parent / 'native-20261012/bin/os-decode'
    expected = '7cb5a706267893f76e2b9df965c650b81283d0f2d781b4624de9b9c0cb4d6fa2'
    if hashlib.sha256(elf.read_bytes()).hexdigest() != expected:
        raise SystemExit('independent decoder ELF changed')
    out = t.BASE / 'oracle/decode'
    out.mkdir(parents=True, exist_ok=False)
    for name in ('fast-1', 'fast-3', 'fast-5', 'fast-8', 'fast-10'):
        t.phase('decode-' + name, ['docker', 'run', '--rm', '--pull', 'never', '--network', 'none',
            '--user', '0:0', '--entrypoint', '/opt/os-decode', '-v', str(elf) + ':/opt/os-decode:ro',
            '-v', str(t.BASE) + ':/work', 'oo-a395-selfbuild:local',
            '--ipc', '/work/capture/' + name + '/batch.arrow', '--out', '/work/oracle/decode/' + name + '.json'])
    print('FULL_ARROW_DECODED: physical target cell comparison is not run')


if __name__ == '__main__':
    main()
