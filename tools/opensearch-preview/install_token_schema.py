#!/usr/bin/env python3
"""Install the supported Gateway token schema in this source's token DB."""
import hashlib
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]
BASE=Path('/tmp/oo-dl01w1-44-gateway-preview')
EVID=ROOT/'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-product/token-setup'
ISSUE=BASE/'issue-token'
CA=Path('/tmp/oo-dl01w1-44-native-fresh-private-r2/certs/ca.pem')


def main():
    EVID.mkdir(parents=True,exist_ok=False)
    assert hashlib.sha256(ISSUE.read_bytes()).hexdigest()=='e74aa1072b941ab1c48552d8aa8aeac0f27022440dfcef57ff4910c2b3549f0a'
    cmd=['docker','run','--rm','--pull','never','--network','oo-dl01w1-71-net',
        '--entrypoint','/private/issue-token','-v',str(BASE)+':/private:ro',
        '-v',str(CA)+':/ca.pem:ro','oo-a395-selfbuild:local',
        'install','/private/token-endpoint.json','/private/token-admin.pass','/ca.pem']
    proc=subprocess.run(cmd,capture_output=True)
    (EVID/'install.stdout').write_bytes(proc.stdout)
    (EVID/'install.stderr').write_bytes(proc.stderr)
    (EVID/'install.exit').write_text(str(proc.returncode)+'\n')
    print('token schema install exit',proc.returncode)
    return proc.returncode


if __name__=='__main__':raise SystemExit(main())
