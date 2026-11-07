#!/usr/bin/env python3
"""D15 product loop through the real Builder, owner review, publisher and mTLS Gateway.

    OO_TEST_LANE=p python3 tools/release-coordinator/tests/retained_cycle_live.py \
        --run-id RUN [--engine postgres|mysql|oracle] [--model-stack FILE]

oo-load sync -> oo-load publish (pending) -> retained Builder (bounded model run
through model-egress, secret_ref files only) -> `oo review --review-file`
(accept-all) -> `release-coordinator publish-retained` (fences + activation)
-> Gateway query; then one data-only source change -> data-only Builder with
zero model calls -> same reviewed registry published for the new generation.
Refusals are exercised against the same owned PostgreSQL. Containers are
labelled oo.lane=p and removed. Raw output stays under evidence/RUN.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
os.environ.setdefault('OO_TEST_LANE', 'p')
spec = importlib.util.spec_from_file_location('gateway_cycles', ROOT / 'tools/oo-load/test-gateway-cycles.py')
cycles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cycles)
sys.path.insert(0, str(ROOT / 'tools/pg-builder/scripts'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import retained_fixture  # noqa: E402

BIN = ROOT / 'target/debug'
ROLE, LOGIN = 'oo_r_owner', 'oo_f22_login'


def sha(data):
    return hashlib.sha256(data).hexdigest()


class Retained(cycles.Live):
    def __init__(self, out, engine, model_stack):
        super().__init__(out, engine)
        self.model_stack = model_stack
        self.results = {'engine': engine, 'refusals': {}, 'cycles': []}
        self.reviewed = None
        self.origin_facts = None
        self.document = None

    # -- product steps -------------------------------------------------
    def prepare(self, run):
        argv = [BIN / 'oo-load', 'publish', '--stack', self.stack, '--source', 'owned', '--target', self.target,
                '--secrets-dir', self.db.directory, '--generation', run, '--library', 'owned']
        result = subprocess.run([str(a) for a in argv], capture_output=True)
        (self.out / ('prepare-' + run + '.stderr')).write_bytes(result.stderr)
        # No reviewed fence is open yet: semantic admission must refuse activation.
        assert result.returncode != 0 and json.loads(result.stderr)['error'] == 'publication_semantic_admission_required'
        cut = json.loads(result.stdout)
        assert cut['state'] == 'pending'
        cycles.write_json(self.out / ('cut-' + run + '.json'), cut)
        return cut

    def builder(self, run, origin=None):
        config = {'protocol': 'oo.builder.retained-build/v1',
                  'psql_command': ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres'],
                  'library_id': 'owned', 'generation': run, 'source_engine': self.engine, 'project': 'owned',
                  'model': {'stack': str(self.model_stack), 'egress': str(ROOT / 'tools/model-egress/model_egress.py'),
                            'call_limit': getattr(self, 'builder_call_limit', 6)}}
        if origin:
            config['origin'] = origin
        path = self.root / ('build-' + run + '.json')
        cycles.private(path, json.dumps(config))
        output = self.out / ('builder-' + run)
        result = subprocess.run([sys.executable, str(ROOT / 'tools/pg-builder/scripts/retained_build.py'),
                                 '--config', str(path), '--output', str(output)], capture_output=True,
                                env=dict(os.environ, OO_SELFBUILD_BUDGET_BIN=str(BIN / 'selfbuild-budget')),
                                timeout=1800)
        (self.out / ('builder-' + run + '.stdout')).write_bytes(result.stdout)
        (self.out / ('builder-' + run + '.stderr')).write_bytes(result.stderr)
        report = json.loads(result.stdout.splitlines()[-1])
        assert result.returncode == 0, report
        return report, output

    def review(self, output):
        """Owner self-review from a decision file: every item and the G14 summary accepted."""
        return retained_fixture.owner_review(output / 'review-input.json', output / 'reviewed-registry.json')

    def publisher(self, run, reviewed, label, login=LOGIN, library='owned'):
        config = self.root / 'publisher.json'
        if not config.exists():
            cycles.private(config, json.dumps({'host': '127.0.0.1', 'port': self.db.port, 'database': 'postgres',
                'user': 'postgres', 'password_file': str(self.db.directory / 'password'),
                'ca_file': str(self.db.directory / 'source-ca')}))
        # Splice the reviewed bytes: no float or key re-serialization before the publisher.
        body = (b'{"protocol":"oo.release-coordinator/publish-retained/v1","library_id":' + json.dumps(library).encode()
                + b',"generation":' + json.dumps(run).encode() + b',"login":' + json.dumps(login).encode()
                + b',"reviewed_registry":' + reviewed + b'}')
        result = subprocess.run([str(BIN / 'release-coordinator'), 'publish-retained', '--config', str(config)],
                                input=body, capture_output=True, timeout=120)
        (self.out / ('publisher-' + label + '.stdout')).write_bytes(result.stdout)
        (self.out / ('publisher-' + label + '.stderr')).write_bytes(result.stderr)
        return result.returncode, json.loads(result.stdout) if result.stdout.strip() else None

    def state(self):
        return {'head': self.db.sql("SELECT state||':'||receipt_sha256 FROM _oo.native_library_head WHERE library_id='owned'").strip(),
                'open_fences': int(self.db.sql("SELECT count(*) FROM oo_publish_target.fence WHERE state='open'").strip()),
                'legacy_imports': int(self.db.sql('SELECT count(*) FROM _oo.full_import_run').strip()),
                'legacy_acks': int(self.db.sql('SELECT count(*) FROM _oo.apply_receipts').strip())}

    def refuse(self, name, run, reviewed, reason, **kwargs):
        before = self.state()
        code, receipt = self.publisher(run, reviewed, 'refuse-' + name, **kwargs)
        after = self.state()
        assert code == 2 and receipt['reason'] == reason and before == after, (name, code, receipt, before, after)
        self.results['refusals'][name] = {'exit': code, 'reason': reason, 'state_unchanged': True}

    def install_gateway_files(self, receipt):
        document = receipt['gateway_document']
        self.pack_artifact = document['envelope']['producer']['artifact']
        if (self.runtime / 'packs').exists():
            self.fixture.replace(self.runtime / 'packs/library.json', self.fixture.encode(self.pack_artifact) + b'\n')
        path = self.runtime / 'reviewed/library/library.json'
        path.parent.mkdir(parents=True, exist_ok=True)
        self.fixture.replace(path, self.fixture.encode(document))
        self.document = document

    def publish(self, run):
        """Override of the fixture shortcut: the real Builder/review/publisher chain."""
        cut = self.prepare(run)
        self.cuts.append(cut)
        if run == 'baseline':
            self.db.sql(f"CREATE ROLE {ROLE} NOLOGIN NOINHERIT; GRANT {ROLE} TO {LOGIN} WITH INHERIT FALSE, SET TRUE;")
            report, output = self.builder(run)
            assert report['stage'] == 'review_required' and report['model_calls'] >= 1, report
            self.reviewed = self.review(output)
            self.refusals(run, cut)
            self.origin_facts = output / 'facts.json'
        else:
            pins = {name: {'path': str(path), 'sha256': sha(path.read_bytes())} for name, path in
                    [('facts', self.origin_facts), ('reviewed_registry', self.out / 'builder-baseline/reviewed-registry.json')]}
            report, output = self.builder(run, pins)
            assert report['stage'] == 'data_only' and report['model_calls'] == 0, report
            assert not (output / 'usage.jsonl').exists() and not (output / 'model-call-count.json').exists()
        code, receipt = self.publisher(run, self.reviewed, 'publish-' + run)
        assert code == 0 and receipt['outcome'] == 'activated' and receipt['state'] == 'ready', receipt
        replay = self.publisher(run, self.reviewed, 'replay-' + run)
        assert replay[0] == 0 and replay[1]['outcome'] == 'replay', replay
        self.install_gateway_files(receipt)
        self.results['cycles'].append({'generation': run, 'builder_stage': report['stage'],
            'model_calls': report['model_calls'], 'data_only_digest': report['data_only_digest'],
            'receipt_sha256': receipt['receipt_sha256'], 'pack_digest': receipt['pack_digest'],
            'fences': [f['outcome'] for f in receipt['fences']], 'replay': replay[1]['outcome'],
            'state': self.state()})
        return receipt

    def refusals(self, run, cut):
        reviewed = json.loads(self.reviewed)
        independent = json.loads(self.reviewed)
        independent['review_decision']['mode'] = 'independent'
        self.refuse('review_mode', run, json.dumps(independent).encode(), 'review_mode_self_required')
        tampered = json.loads(self.reviewed)
        tampered['compiled']['producer']['pack_digest'] = '0' * 64
        self.refuse('compiled_pack', run, json.dumps(tampered).encode(), 'review_compiled_mismatch')
        rejected = json.loads(self.reviewed)
        item = next(iter(rejected['review_decision']['decisions']['items']))
        rejected['review_decision']['decisions']['items'][item] = {'action': 'reject', 'reason': 'owner'}
        self.refuse('rejected_item', run, json.dumps(rejected).encode(), 'review_rejected_item')
        self.refuse('unknown_generation', 'never-sealed', self.reviewed, 'retained_candidate_missing')
        self.refuse('login', run, self.reviewed, 'retained_login_binding', login='reader')
        # A relation changed after sealing fails the in-database D15 proof; nothing opens.
        physical = cut['data_vector'][0]['physical_name']
        original = self.db.sql(f"SELECT pg_get_viewdef('_oo.\"{physical}\"'::regclass,false)").strip()
        column = self.db.sql(f"SELECT attname FROM pg_attribute WHERE attrelid='_oo.\"{physical}\"'::regclass AND attnum=1").strip()
        kind = self.db.sql(f"SELECT format_type(atttypid,atttypmod) FROM pg_attribute WHERE attrelid='_oo.\"{physical}\"'::regclass AND attnum=1").strip()
        self.db.sql(f'CREATE OR REPLACE VIEW _oo."{physical}" AS SELECT NULL::{kind} AS "{column}";')
        self.refuse('relation_drift', run, self.reviewed, 'semantic_admission_refused')
        self.db.sql(f'CREATE OR REPLACE VIEW _oo."{physical}" AS {original}')
        assert reviewed['review_decision']['mode'] == 'self'

    def start_gateway(self):
        base = self.fixture.sha
        super_write = self.fixture.write

        def write(path, value):
            # The parent harness writes its fixture Pack; serve the owner-reviewed Pack instead.
            if path == self.runtime / 'packs/library.json':
                value = self.pack_artifact
            return super_write(path, value)
        self.fixture.write = write
        try:
            super().start_gateway()
        finally:
            self.fixture.write = super_write
        # Token for the reviewed role, project and versions (not the fixture's f22 identity).
        self.db.sql("DELETE FROM oo_token_control.token_record;")
        now = int(cycles.time.time() * 1000)
        self.db.sql(f"INSERT INTO oo_token_control.token_record VALUES('{base(self.token.encode())}',"
                    f"'client','owned','owner','{ROLE}','{{}}','draft-v1','draft-v1','{self.origin}/v1',"
                    f"NULL,NULL,{now},NULL,{now + 86400000},{now},0);")

    def cycles(self):
        first = self.http()
        assert first['status'] == 200 and first['body']['rows'] == [['10']], first
        self.source_sql('UPDATE app.items SET amount=11;')
        (self.root / 'c5.json').write_text(json.dumps({'cycle': 1, 'full_read_every': 1}))
        self.load(['sync', '--c5', self.root / 'c5.json'], 'cycle-1')
        self.publish('cycle-1')
        second = self.http()
        assert second['status'] == 200 and second['body']['rows'] == [['11']], second
        assert second['publication'] != first['publication'] and second['vector'] != first['vector']
        self.refuse('superseded', 'baseline', self.reviewed, 'publication_superseded')
        state = self.state()
        assert state['legacy_imports'] == 0 and state['legacy_acks'] == 0, state
        freshness = self.http('/v1/freshness')
        assert freshness['status'] == 200
        self.results.update(queries=[{k: q.get(k) for k in ('status', 'publication', 'vector')} | {
            'rows': q['body']['rows']} for q in (first, second)],
            freshness_generation=freshness['body']['target_observations'][0]['source_generation'],
            version=self.source_version if self.engine != 'postgres' else self.db.sql('SHOW server_version').strip(),
            legacy_imports=state['legacy_imports'], legacy_acks=state['legacy_acks'])
        cycles.write_json(self.out / 'live-result.json', self.results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--engine', choices=['postgres', 'mysql', 'oracle'], default='postgres')
    parser.add_argument('--model-stack', type=Path, required=True,
                        help='JSON with only a model object; base_url/key must be secret_ref names')
    args = parser.parse_args()
    assert args.run_id and all(c.isalnum() or c == '-' for c in args.run_id)
    model = json.loads(args.model_stack.read_text())['model']
    assert isinstance(model['key'], dict) and set(model['key']) == {'secret_ref'}
    os.umask(0o077)
    out = ROOT / 'evidence' / args.run_id
    out.mkdir(exist_ok=False)
    live = Retained(out, args.engine, args.model_stack.resolve())
    status = 'failed'
    try:
        live.start()
        live.cycles()
        status = 'passed'
    finally:
        live.close()
        cycles.write_json(out / 'result.json', {'status': status, 'containers_left': 0})
        cycles.checksums(out)


if __name__ == '__main__':
    main()
