"""Renew an existing local control CA/server certificate without disabling TLS.

Outputs certificates only; installing them and reloading the server are separate
operator actions. Existing private keys and historical certificates are untouched.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess


def openssl(*args):
    return subprocess.check_output(['openssl', *map(str, args)], stderr=subprocess.PIPE)


def renew(ca, key, csr, extensions, output, days=90):
    if type(days) is not int or not 1 <= days <= 365:
        raise ValueError('certificate_lifetime')
    ca, key, csr, extensions = map(Path, (ca, key, csr, extensions))
    if openssl('x509', '-in', ca, '-pubkey', '-noout') != openssl('pkey', '-in', key, '-pubout'):
        raise ValueError('ca_key_mismatch')
    openssl('req', '-in', csr, '-verify', '-noout')
    output = Path(output).resolve()
    if any((p / '.git').exists() for p in (output, *output.parents)):
        raise ValueError('certificate_output_must_be_private')
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    renewed, server = output / 'ca.pem', output / 'server.pem'
    openssl('x509', '-in', ca, '-signkey', key, '-days', days,
            '-set_serial', '0x' + secrets.token_hex(16), '-out', renewed)
    openssl('x509', '-req', '-in', csr, '-CA', renewed, '-CAkey', key,
            '-set_serial', '0x' + secrets.token_hex(16), '-days', days,
            '-extfile', extensions, '-out', server)
    openssl('verify', '-CAfile', renewed, server)
    openssl('x509', '-in', renewed, '-checkend', 3600, '-noout')
    openssl('x509', '-in', server, '-checkend', 3600, '-noout')
    result = {'protocol': 'oo.local-control-tls-renewal/v1', 'installed': False,
              'private_keys_changed': False, 'days': days,
              'previous_ca_sha256': hashlib.sha256(ca.read_bytes()).hexdigest(),
              'ca_sha256': hashlib.sha256(renewed.read_bytes()).hexdigest(),
              'server_sha256': hashlib.sha256(server.read_bytes()).hexdigest()}
    (output / 'receipt.json').write_text(json.dumps(result, indent=2))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['ca', 'key', 'csr', 'extensions', 'output']:
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--days', type=int, default=90)
    args = parser.parse_args()
    os.umask(0o077)
    print(json.dumps(renew(**vars(args))))
