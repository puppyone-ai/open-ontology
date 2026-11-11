"""Opt-in live acceptance for relationship discovery (V3-G-01).

    OO_RELATIONSHIP_LIVE=1 python3 -m unittest test_relationship_live

Starts one throwaway `postgres:17.11-bookworm` container on loopback with its
own label, seeds a few hundred synthetic rows, runs discovery, adjudication,
the confirmed compiler and the Pack query paths against it, and removes the
container afterwards. It never touches any other container. Needs docker,
psql and a built `target/debug/selfbuild-budget`.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
import unittest

import adjudication as a
import library_observation as observation
import library_selfbuild as lib
import relation_keys
import retained_build
import selfbuild_budget

IMAGE = 'postgres:17.11-bookworm'
LABEL = 'oo.test=relationship-live'

SEED = r"""
create schema app;
create table app.regions(id bigint primary key, name text not null);
insert into app.regions select g, 'Region ' || g from generate_series(1, 6) g;
create table app.customers(id bigint primary key, customer_code text not null, region_name text);
insert into app.customers select g, 'C' || lpad(g::text, 4, '0'), 'Region ' || (1 + g % 6)
  from generate_series(1, 40) g;
create table app.warehouses(wh_code text primary key, label text);
insert into app.warehouses values ('001', 'North'), ('002', 'South');
create table app.branches(branch_code text, city text);
insert into app.branches values ('B1', 'x'), ('B1', 'y'), ('B2', 'z');
create table app.brands(brand_code text primary key);
insert into app.brands values ('ACME'), ('acme '), ('Globex');
create table app.products(product_code text primary key);
insert into app.products values ('P1'), ('P2');
create table app.sales_targets(region_id bigint not null, period text not null, target numeric(12,2));
insert into app.sales_targets select r, p, r * 100 from generate_series(1, 6) r, (values ('2026-01'), ('2026-02')) v(p);
create table app.orders(
  id bigint primary key,
  customer_id bigint references app.customers(id),
  origin_region_id bigint, dest_region_id bigint,
  wh_code text, branch_code text, brand_code text, product_code text,
  amount numeric(12,2), created_at timestamp);
insert into app.orders select g, 1 + g % 40, 1 + g % 6, 1 + (g + 3) % 6,
  case when g % 10 = 0 then '1' when g % 10 = 1 then '' when g % 10 = 2 then null else '001' end,
  case when g % 2 = 0 then 'B1' else 'B2' end,
  case when g % 3 = 0 then 'ACME' else 'Globex' end,
  case when g <= 180 then 'P1' else 'X' || g end,
  g * 1.25, timestamp '2026-01-01' + g * interval '1 minute'
  from generate_series(1, 200) g;
create table app.payments(amount numeric(12,2), paid_at timestamp, note text);
insert into app.payments select g * 2.5, timestamp '2026-02-01' + g * interval '1 hour', 'n'
  from generate_series(1, 30) g;
"""

MODEL_EDGES = [  # What a drafting model might propose; adjudication decides.
    ('m-branch', 'app.orders', ['branch_code'], 'app.branches', ['branch_code']),
    ('m-brand', 'app.orders', ['brand_code'], 'app.brands', ['brand_code']),
    ('m-target', 'app.orders', ['origin_region_id'], 'app.sales_targets', ['region_id']),
]


@unittest.skipUnless(os.environ.get('OO_RELATIONSHIP_LIVE') == '1', 'opt-in live PostgreSQL acceptance')
class RelationshipLive(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.name = 'oo-relconv-live-' + secrets.token_hex(4)
        password = secrets.token_hex(16)
        subprocess.run(['docker', 'run', '-d', '--rm', '--name', cls.name, '--label', LABEL,
                        '--memory', '256m', '--cpus', '0.5', '-e', 'POSTGRES_PASSWORD=' + password,
                        '-p', '127.0.0.1::5432', IMAGE], check=True, capture_output=True)
        try:
            port = subprocess.run(['docker', 'port', cls.name, '5432/tcp'], check=True,
                                  capture_output=True, text=True).stdout.split(':')[-1].strip()
            os.environ['PGPASSWORD'] = password
            cls.url = ['psql', 'postgresql://postgres@127.0.0.1:%s/postgres' % port]
            for _ in range(120):
                if subprocess.run(cls.url + ['-Atc', 'select 1'], capture_output=True).returncode == 0:
                    break
                time.sleep(.5)
            else:
                raise RuntimeError('live_postgres_not_ready')
            cls.admin(SEED)
            cls.db = lib.ReadOnlyPostgres(cls.url, 30000)
        except BaseException:
            cls.tearDownClass()
            raise

    @classmethod
    def tearDownClass(cls):
        # Only the container this class started, by its own name.
        subprocess.run(['docker', 'rm', '-f', cls.name], capture_output=True)

    @classmethod
    def admin(cls, sql):
        subprocess.run(cls.url + ['-X', '-q', '-v', 'ON_ERROR_STOP=1'], input=sql, text=True,
                       check=True, capture_output=True)

    def scalar(self, sql):
        return self.db.query('select json_build_object(\'v\',(' + sql + '))')['v']

    def tables(self):
        meta = self.db.query(
            "select json_agg(json_build_object('t',c.relname,'c',a.attname,'type',"
            "format_type(a.atttypid,a.atttypmod),'n',a.attnum) order by c.relname,a.attnum) "
            "from pg_attribute a join pg_class c on c.oid=a.attrelid join pg_namespace n "
            "on n.oid=c.relnamespace where n.nspname='app' and c.relkind='r' and a.attnum>0 "
            "and not a.attisdropped")
        keys = self.db.query(
            "select json_agg(json_build_object('t',c.relname,'type',k.contype,'cols',"
            "(select json_agg(a.attname order by x.ord) from unnest(k.conkey) with ordinality x(n,ord) "
            "join pg_attribute a on a.attrelid=k.conrelid and a.attnum=x.n),'ref',r.relname,'refcols',"
            "(select json_agg(a.attname order by x.ord) from unnest(k.confkey) with ordinality x(n,ord) "
            "join pg_attribute a on a.attrelid=k.confrelid and a.attnum=x.n))) from pg_constraint k "
            "join pg_class c on c.oid=k.conrelid left join pg_class r on r.oid=k.confrelid "
            "join pg_namespace n on n.oid=c.relnamespace where n.nspname='app'")
        result = []
        for name in sorted({m['t'] for m in meta}):
            columns = [dict(name=m['c'], physical_name=m['c'], data_type=m['type'])
                       for m in meta if m['t'] == name]
            observed = self.db.query(observation.profile_sql('"app"."%s"' % name, columns))
            profiles = observation.profiles(observed, [c['name'] for c in columns])
            for c in columns:
                c['profile'] = profiles[c['name']]
            pk = next((k['cols'] for k in keys if k['t'] == name and k['type'] == 'p'), [])
            fks = [dict(name='fk_%s_%d' % (name, i), columns=k['cols'],
                        referenced_dataset='app.' + k['ref'], referenced_columns=k['refcols'])
                   for i, k in enumerate(k for k in keys if k['t'] == name and k['type'] == 'f')]
            result.append(dict(dataset='app.' + name, physical='"app"."%s"' % name, primary_key=pk,
                               foreign_keys=fks, columns=columns))
        return result

    def adjudicate(self, budget=65536, cache=None):
        tables = self.tables()
        edges = [dict(id=i, **{'from': f, 'to': t}, columns={'from': fc, 'to': tc},
                      provenance='inferred', confidence=0.5) for i, f, fc, t, tc in MODEL_EDGES]
        command = retained_build.review_input(tables, {'graph': {'edges': edges}}, 'live')
        a.discover_candidates(command, tables)
        raw = lib.canonical(command)
        document = a.collect(self.db, command, tables, hashlib.sha256(raw.encode()).hexdigest(),
                             budget=budget, cache_path=cache)
        return tables, raw, document

    def relation(self, document, child, columns, parent):
        ends = {e['relation']: (e['from'], e['to']) for e in document['graph']['edges']}
        for item in document['items']:
            if (item['class'] == 'relation' and item['evidence']['from_columns'] == columns
                    and ends[item['id']] == ('entity/' + child, 'entity/' + parent)):
                return item
        self.fail('missing relation %s.%s -> %s' % (child, columns, parent))

    def test_matrix(self):
        tables, raw, document = self.adjudicate()
        rel = lambda *k: self.relation(document, *k)
        results = {}
        # 1. Declared FK unchanged.
        fk = rel('app.orders', ['customer_id'], 'app.customers')
        self.assertEqual((fk['decision'], fk['evidence']['kind']), ('adopted', 'declared_fk'))
        results['declared_fk'] = fk['rule_id']
        # 2. Unique name mapping with different field names.
        mapping = rel('app.customers', ['region_name'], 'app.regions')
        self.assertEqual(mapping['evidence']['to_columns'], ['name'])
        self.assertEqual((mapping['decision'], mapping['evidence']['kind']), ('adopted', 'snapshot_name_mapping'))
        results['name_mapping'] = mapping['rule_id']
        # 3. Unique amount/timestamp is not a key.
        keys = {i['subject']['table']: i for i in document['items'] if i['class'] == 'entity_key'}
        self.assertEqual((keys['app.payments']['decision'], keys['app.payments']['rule_id']),
                         ('rejected', 'entity_key.no_candidate'))
        self.assertEqual(keys['app.orders']['evidence']['key'], ['id'])
        self.assertEqual(keys['app.sales_targets']['rule_id'], 'entity_key.no_candidate')
        # 4. Duplicate parent key is not adopted.
        branch = rel('app.orders', ['branch_code'], 'app.branches')
        self.assertEqual((branch['decision'], branch['rule_id']), ('rejected', 'relation.parent_not_unique'))
        self.assertEqual(branch['evidence']['max_fanout'], 2)
        # 5. Normalized collision is blocked.
        brand = rel('app.orders', ['brand_code'], 'app.brands')
        self.assertEqual((brand['decision'], brand['rule_id']), ('rejected', 'relation.normalized_collision'))
        # 6. Long tail: both coverages shown, unmatched kept.
        tail = rel('app.orders', ['product_code'], 'app.products')
        e = tail['evidence']
        self.assertEqual((e['containment'], round(e['distinct_containment'], 4), e['unmatched_rows']),
                         (.9, round(1 / 21, 4), 20))
        self.assertEqual(tail['decision'], 'rejected')
        # 7. NULL, blank and leading zeros: no false matches.
        wh = rel('app.orders', ['wh_code'], 'app.warehouses')
        e = wh['evidence']
        self.assertEqual((e['rows_checked'], e['matched_rows'], e['empty_rows'], e['unmatched_distinct']),
                         (160, 140, 20, 1))
        # 8. Two roles to one parent stay apart.
        origin = rel('app.orders', ['origin_region_id'], 'app.regions')
        dest = rel('app.orders', ['dest_region_id'], 'app.regions')
        self.assertEqual((origin['evidence']['role'], dest['evidence']['role']), ('origin_region', 'dest_region'))
        self.assertTrue(origin['decision'] == dest['decision'] == 'adopted')
        # 9. Scoped parent.
        scoped = rel('app.orders', ['origin_region_id'], 'app.sales_targets')
        self.assertEqual((scoped['rule_id'], scoped['evidence']['scope_columns'], scoped['evidence']['kind']),
                         ('relation.scope_required', ['period'], 'scoped'))
        # 10. JOIN row count unchanged for every adopted edge.
        inventory = {t['dataset']: t for t in tables}
        ends = {e['relation']: (e['from'][7:], e['to'][7:]) for e in document['graph']['edges']}
        adopted = [i for i in document['items'] if i['class'] == 'relation' and i['decision'] == 'adopted']
        for item in adopted:
            child, parent = (inventory[n] for n in ends[item['id']])
            on = ' and '.join('p."%s"::text=c."%s"::text' % (k, c) for c, k in
                              zip(item['evidence']['from_columns'], item['evidence']['to_columns']))
            joined = self.scalar('select count(*) from %s c left join %s p on %s'
                                 % (child['physical'], parent['physical'], on))
            self.assertEqual(joined, self.scalar('select count(*) from ' + child['physical']), item['id'])
        self.assertTrue(all(e['cardinality'] != 'many_to_many' for e in document['graph']['edges'] if e['adopted']))
        # 13. Pack query paths execute and match a grain-correct reference.
        summary = document['summary_sha256']
        overrides = {'protocol': 'oo.adjudication-override/v1', 'summary_sha256': summary, 'items': {}}
        envelope = dict(adjudication_input=raw, adjudication=document, overrides=overrides,
                        confirmation={'protocol': 'oo.publish-confirm/v1', 'summary_sha256': summary,
                                      'overrides_sha256': lib.digest(overrides), 'reason': 'Live acceptance',
                                      'confirmed_at': '2026-11-11T12:00:00Z'})
        compiled = selfbuild_budget.bridge('reviewed', command=envelope)
        files = {f['path']: f['text'] for f in compiled['producer']['artifact']['files']}
        paths = json.loads(files['relationships/query-paths.json'])
        self.assertEqual(paths['evidence_sha256'],
                         hashlib.sha256(files['relationships/evidence.json'].encode()).hexdigest())
        self.assertEqual(len(paths['paths']), len(adopted))
        for path in paths['paths']:
            total = self.scalar('select coalesce(sum(child_rows),0) from (' + path['sql'] + ') q')
            reference = self.scalar('select count(*) from ' + inventory[path['start']]['physical'])
            self.assertEqual(total, reference, path['relationship'])
            # Each parent key appears once: the example never repeats a parent row.
            keys = ','.join('q.parent_key_%d' % (i + 1) for i in range(len(path['join']['keys'])))
            self.assertEqual(self.scalar('select count(*) - count(distinct (%s)) from (%s) q where '
                                         'q.parent_key_1 is not null' % (keys, path['sql'])), 0)
        self.assertIn('relation.scope_required', {w['rule'] for w in paths['withheld']})
        results['pack_paths'] = len(paths['paths'])
        print(json.dumps({'live_matrix': results, 'adopted': sorted(i['subject']['from'] + '->' + i['subject']['to']
                                                                for i in adopted),
                          'relations': sum(i['class'] == 'relation' for i in document['items'])}))

    def test_budget_resume_and_key_quality(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / 'adjudication-measurements.json'
            _, _, full = self.adjudicate()
            total = full['discovery']['checked']
            _, _, first = self.adjudicate(budget=3, cache=cache)
            self.assertEqual((first['discovery']['status'], first['discovery']['checked']), ('incomplete', 3))
            rounds = 1
            document = first
            while document['discovery']['status'] == 'incomplete':
                _, _, document = self.adjudicate(budget=3, cache=cache)
                rounds += 1
                self.assertLessEqual(rounds, total)
            self.assertEqual(document['summary_sha256'], full['summary_sha256'])
            self.assertEqual(document['discovery']['checked'], total)
        # 12. Key-quality change invalidates; plain growth does not. Adopted
        # relationship ends join the key columns of the data-only boundary.
        mapping = self.relation(full, 'app.customers', ['region_name'], 'app.regions')
        ends = [{'from': {'dataset': 'app.customers', 'columns': ['region_name']},
                 'to': {'dataset': 'app.regions', 'columns': ['name']}}]
        quality = lambda tables: relation_keys.key_quality({'tables': tables, 'relationship_candidates': ends})
        before = quality(self.tables())
        self.admin("insert into app.regions values (7, 'Region 7');"
                   "insert into app.customers values (41, 'C0041', 'Region 7');")
        grown = self.tables()
        self.assertEqual(quality(grown), before)
        self.assertEqual(a.relationship_quality_drift(self.db, full, grown), [])
        self.admin("insert into app.regions values (8, 'Region 1');")
        changed = self.tables()
        self.assertNotEqual(quality(changed)['app.regions'], before['app.regions'])
        self.assertEqual(a.relationship_quality_drift(self.db, full, changed), [mapping['id']])
        self.admin("delete from app.regions where id in (7, 8); delete from app.customers where id = 41;")


if __name__ == '__main__':
    unittest.main()
