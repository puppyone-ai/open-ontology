#!/usr/bin/env python3
"""Current native collection, full typed target oracle and fresh sealed bundle."""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from connection_target import Target
import fresh_capture as f
sys.path.insert(0,str(f.ROOT/'tools/pg-builder/scripts'))
from export_native_collection import collection

def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def write(path,value):path.write_text(json.dumps(value,indent=2,ensure_ascii=False)+'\n')

def main():
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--run',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--gold',type=Path,required=True);a=p.parse_args()
    run=a.run.resolve();capture=a.capture.resolve();config=json.loads(a.config.read_text());t=Target(config,run)
    loaded=json.loads((capture/'load-manifest.json').read_text())['results'][-1]
    binding=json.loads(Path(loaded['registry']).read_text())['bindings'][0]
    target=json.loads((run/'loader-target.json').read_text())
    source=config['source'];database=source['database'];table_name=source['table'];dataset=database+'.'+table_name
    generation=loaded['generation']
    request={key:binding[key] for key in ('source_id','source_generation','config_digest','schema_digest','type_digest','input_digest','snapshot')}
    request.update(protocol='oo.pipeline.native-sealed-catalog/v1',run_id=loaded['run_id'],owner=target['owner'],target_generation=generation,
        target={key:target[key] for key in ('host','port','database','runtime_principal','password_file','ca_file')})
    entry=t.pipeline('catalog-g'+str(generation),'catalog-import-native',request,Path(loaded['registry']))
    body=collection([entry],[(database,table_name)])
    out=run/'materialized';out.mkdir(mode=0o700,exist_ok=False)
    write(out/'collection.json',body)
    verify=Path(__file__).with_name('verify_current_c4_target.py')
    t.exec('target-full-typed',['python3',str(verify),'--gold',str(a.gold),'--generation',str(generation),'--connection-config',str(a.config),'--output',str(out/'typed-diff.json')])
    diff=json.loads((out/'typed-diff.json').read_text())
    if not diff['diff_zero']:raise RuntimeError('full target typed mismatch')
    cycle_path=run/'cycle-receipt.json'
    cycle=json.loads(cycle_path.read_text())
    cycle.update(typed_diff_zero=True, typed_diff_sha256=digest(out/'typed-diff.json'),
        typed_cells_compared=diff['row_count']*len(diff['columns']), target_generation=generation,
        physical=diff['table'])
    write(cycle_path,cycle)
    gold=json.loads(a.gold.read_text());columns=[json.loads(x) for x in (capture/'columns.body').read_text().splitlines()]
    profiles=[]
    for col in columns:
        name=col['name'];values=[r[name] for r in gold];nonnull=[v for v in values if v is not None]
        encoded=[json.dumps(v,ensure_ascii=False,sort_keys=True) if isinstance(v,list) else str(v) for v in nonnull]
        ordered=sorted(encoded,key=Decimal) if col['type'].startswith(('UInt','Int','Decimal')) else sorted(encoded)
        profiles.append({'name':name,'source_type':col['type'],'nullable':col['type'].startswith(('Nullable(','LowCardinality(Nullable(')),
            'aggregation':'full_unsampled','min_max_encoding':'source_text','rows':len(values),'nulls':len(values)-len(nonnull),
            'distinct':len(set(encoded)),'null_count':len(values)-len(nonnull),'distinct_count':len(set(encoded)),
            'min':ordered[0] if ordered else None,'max':ordered[-1] if ordered else None})
    profile={'protocol':'oo.source.clickhouse.profile/v1','source_id':binding['source_id'],'source_generation':generation,
        'samples':'none','sampled':False,'generation_semantics':'C4 partition-vector high-water mark',
        'tables':[{'dataset':dataset,'schema':database,'table':table_name,'rows':len(gold),'full_scan':True,'columns':profiles,'source_generation':generation,'target_generation':generation}],
        'provenance':{'current_capture':str(capture),'full_source_oracle_sha256':digest(a.gold),'target_full_typed_diff_sha256':digest(out/'typed-diff.json')}}
    write(out/'profile.json',profile)
    # Engine semantics are unchanged; all source identity and evidence fields are current.
    original=f.ROOT/'evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/native-20261013-c4-current-semantics-v3/source-semantics.json'
    if not original.exists():
        matches=list((f.ROOT/'evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run').glob('native-20261013-c4*semantics*/source-semantics.json'))
        if len(matches)!=1:raise RuntimeError('immutable semantics template unavailable')
        original=matches[0]
    semantics=json.loads(original.read_text())
    semantics.update(source_id=binding['source_id'],source_generation=generation,target_generation=generation,
        collection_sha256=digest(out/'collection.json'),binding_vector_sha256=hashlib.sha256(json.dumps(body['binding_vector'],sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest(),
        table_generations={dataset:{'source_generation':generation,'target_generation':generation,'run_id':loaded['run_id']}},
        continuous_capture=json.loads((run/'cycle-receipt.json').read_text()) if (run/'cycle-receipt.json').exists() else config.get('cycle_evidence',{'schedule_periods_tested_seconds':[],'status':'not_yet_tested'}))
    semantics['quality'].update(rows=len(gold),columns=len(columns),typed_cells_compared_each_side=len(gold)*len(columns))
    semantics['source_contract']['reader']=source['user'];semantics['source_contract']['source_reader_query_log_proof']=str(run/'source-reader-audit.json')
    write(out/'semantics.json',semantics)
    bundle=run/'bundle'
    t.exec('bundle',['python3',str(Path(__file__).with_name('build_current_c4_bundle.py')),'--collection',str(out/'collection.json'),'--profile',str(out/'profile.json'),'--connection-config',str(a.config),'--semantics',str(out/'semantics.json'),'--output',str(bundle)])
    write(run/'materialize-manifest.json',{'source_id':binding['source_id'],'source_uuid':json.loads((capture/'source-observation.json').read_text())['partitions'][0]['table_epoch'],
        'generation':generation,'current_run_id':loaded['run_id'],'collection_sha256':digest(out/'collection.json'),'bundle_manifest_sha256':digest(bundle/'manifest.json'),'typed_diff_zero':True})

if __name__=='__main__':main()
