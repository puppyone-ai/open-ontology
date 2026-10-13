#!/usr/bin/env python3
"""Run the fixed OO CLI in the MongoDB Gateway with a verified mounted token path."""
import os, pathlib, sys
args=sys.argv[1:]
try:
 index=args.index('--token-file')
 token=pathlib.Path(args[index+1])
except (ValueError, IndexError):
 raise SystemExit('external token-file required')
root=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview')
if token.parent!=root or not token.name.startswith('client-') or token.suffix!='.token':
 raise SystemExit('unexpected token path')
args[index+1]='/run41/'+token.name
os.execvp('docker',['docker','exec','oo-dl01w1-41-gateway','/fixed/oo-fb8073fd6e01822b6d42ded74ae3673cf202e778b54bdca0ca044c909bb56703',*args])
