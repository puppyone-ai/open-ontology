#!/usr/bin/env python3
"""Emit file-reference admission input from the fresh executed C5 capture.

Generation1 denotes the prospective first import in the new target, not capture
watermark8. This producer is not an admission authority or lease issuer.
"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

def module(name, file):
    spec = importlib.util.spec_from_file_location(name, file)
    obj = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(obj)
    return obj


def main():
    c = module('fresh_capture', HERE / 'native_fresh_capture.py')
    if not (c.OUT / 'receipt-input-2/request.json').exists():
        c.reader('receipt-input-2', ['c5-receipt',
        '--arrow', '/evidence/fast-10/batch.arrow', '--audit', '/evidence/fast-10/audit.jsonl',
        '--config', '/fixtures/c5-schedule.json', '--catalog', '/evidence/fast-10/live.json',
        '--registry', '/fixtures/customer-request.json', '--driver', '/bins/os-reader',
        '--snapshot', '/evidence/fast-10/snapshot.json', '--result', '/evidence/fast-10/result.json',
        '--generation', '1', '--capture-watermark', '10', '--batch-id', 'os44-native-fresh-g1',
        '--logical-schema', 'ledger', '--logical-table', 'ledger_rows', '--key-column', 'source_doc_id',
        '--host', '127.0.0.1', '--port', '5432', '--database', 'oo_dl01w1_44_native_fresh',
        '--runtime-principal', 'oo_dl01w1_44_runtime', '--password-file', '/private/native-runtime.pw',
        '--arrow-path', str(c.OUT / 'fast-10/batch.arrow'),
        '--admission-path', str(c.OUT / 'receipt-input-2/receipt.json'),
        '--out-receipt', '/evidence/receipt-input-2/receipt.json',
        '--out-request', '/evidence/receipt-input-2/request.json'])
    # Reuse the byte-level IPC parser and evidence projection, with explicit own inputs.
    e = module('capture_witness', HERE / 'emit_capture_witness.py')
    e.OUT = c.OUT.parent / 'witness-v2'
    e.ARROW = c.OUT / 'fast-10/batch.arrow'
    e.AUDIT = c.OUT / 'fast-10/audit.jsonl'
    e.PROOF = c.OUT / 'fast-10'
    e.DRIVER = c.BIN
    e.LOAD = c.OUT / 'receipt-input-2/request.json'
    receipt = json.loads((c.OUT / 'receipt-input-2/receipt.json').read_text())
    e.main(generation=1, run_id=receipt['run_id'], snapshot_identity=receipt['snapshot'])


if __name__ == '__main__':
    main()
