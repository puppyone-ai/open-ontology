#!/usr/bin/env python3
"""Local-only cosign proof: ephemeral key, offline verification, tamper refusal."""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cosign', type=Path, required=True)
parser.add_argument('--manifest', type=Path, required=True)
parser.add_argument('--evidence', type=Path, required=True)
args = parser.parse_args()
args.evidence.mkdir(exist_ok=True)
os.umask(0o077)
with tempfile.TemporaryDirectory(prefix='local-signing-', dir=args.evidence) as directory:
    root = Path(directory)
    key = root / 'local'
    env = dict(os.environ, COSIGN_PASSWORD=secrets.token_hex(32))
    def run(argv, expected=0):
        result = subprocess.run([str(args.cosign.resolve()), *map(str, argv)],
                                env=env, capture_output=True)
        with (args.evidence / 'cosign.log').open('ab') as stream:
            stream.write(result.stdout + result.stderr)
        assert (result.returncode == 0) == (expected == 0), 'cosign result; see cosign.log'
        return result.returncode
    run(['generate-key-pair', '--output-key-prefix', key])
    signed = root / 'signature.json'
    run(['sign-blob', '--yes', '--key', str(key) + '.key', '--tlog-upload=false',
         '--bundle', signed, args.manifest])
    verify = ['verify-blob', '--key', str(key) + '.pub', '--bundle', signed,
              '--offline', '--insecure-ignore-tlog']
    run([*verify, args.manifest])
    tampered = root / 'tampered.json'
    tampered.write_bytes(args.manifest.read_bytes() + b'\n')
    rejected = run([*verify, tampered], expected=1)
    # Public evidence only. The private key and password disappear on exit.
    (args.evidence / 'local.pub').write_bytes(Path(str(key) + '.pub').read_bytes())
    (args.evidence / 'signature.json').write_bytes(signed.read_bytes())
    (args.evidence / 'result.json').write_text(json.dumps({
        'local_key_only': True, 'registry_push': False, 'transparency_upload': False,
        'offline_verification': 'passed', 'tampered_manifest_exit': rejected,
        'ci_keyless_signing': 'not_run'}))
