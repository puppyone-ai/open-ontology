#!/usr/bin/env python3
"""Follow one admitted MySQL cut through the existing maintenance and release APIs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO/'tools/source-onboarding'))
from export_native_collection import collection
from product_flow import Flow


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def load(path):
    return json.loads(Path(path).read_bytes())


def save(path, value):
    path = Path(path)
    data = canonical(value)+b'\n'
    if path.exists():
        if path.read_bytes() != data:
            raise ValueError('immutable_output_drift')
        return
    with path.open('xb') as f:
        os.chmod(path, 0o600)
        f.write(data)


class Follow:
    def __init__(self, config, out):
        self.c = config
        self.op = config['onboarding']
        self.p = self.op['product']
        self.out = Path(out)
        self.out.mkdir(mode=0o700, parents=True, exist_ok=True)
        if self.out.is_symlink() or self.out.stat().st_mode & 0o077:
            raise ValueError('private_output')
        self.n = len(list(self.out.glob('*.exit')))
        for binary in [self.op['worker'], self.op['loader'], self.p['api_binary'], self.p['release_binary']]:
            if Path(binary).parent.name != digest(binary):
                raise ValueError('binary_pin')

    def call(self, label, args, data=None):
        self.n += 1
        prefix = self.out/f'{self.n:04d}-{label}'
        result = subprocess.run(args, input=data, capture_output=True)
        for suffix, value in [('stdout', result.stdout), ('stderr', result.stderr), ('exit', str(result.returncode).encode()+b'\n')]:
            p = prefix.with_suffix('.'+suffix)
            p.write_bytes(value)
            p.chmod(0o600)
        if result.returncode:
            raise RuntimeError(label+'_failed')
        return result.stdout

    def worker(self, *args):
        return self.call(args[0], ['docker','exec',self.op['container'],self.op['worker'],'--config',self.op['worker_config'],*args])

    def replace_api(self, container, binary, runtime, label):
        # Only the single owned executable may be replaced; forwarders remain alive.
        code = '''import os,signal,sys,json
binaries=[a.encode() for a in sys.argv[1:]];hits=[]
for p in os.listdir('/proc'):
 if not p.isdigit():continue
 try:a=open('/proc/'+p+'/cmdline','rb').read().split(b'\\0')
 except OSError:continue
 if a and a[0] in binaries and b'serve' in a:hits.append(int(p))
assert len(hits)<=1,hits
for p in hits:os.kill(p,signal.SIGTERM)
print(json.dumps({'stopped_owned_api':hits}))
'''
        self.call(label+'-stop',['docker','exec',container,'python3','-c',code,binary,*([self.p['previous_'+label+'_binary_container']] if 'previous_'+label+'_binary_container' in self.p else [])])
        prefix=self.out/f'{self.n:04d}-{label}'
        proc = subprocess.Popen(['docker','exec',container,binary,'serve','--config',runtime],stdout=prefix.with_suffix('.serve.stdout').open('wb'),stderr=prefix.with_suffix('.serve.stderr').open('wb'))
        save(prefix.with_suffix('.handle.json'),{'docker_exec_pid':proc.pid,'container':container,'binary':binary,'runtime':runtime})
        time.sleep(1)
        if proc.poll() is not None:
            prefix.with_suffix('.serve.exit').write_text(str(proc.returncode)+'\n')
            raise RuntimeError(label+'_start')

    def publish(self, generation, catalog, result):
        product=self.out/'product'
        envelope=load(product/'current-role-response.json')['response']
        native=envelope['native_maintenance'];commit=native['artifact_revision']
        wrapper={'protocol':'oo.release-control.native-reviewed-pack/v1','config_commit':commit,'envelope':envelope,'effective':load(product/'current-effective-oos.json'),'visibility':load(product/'current-visibility.json'),'artifact':load(product/'current-role-pack.json')}
        private=Path(self.p['release_private'])
        save(private/'packs'/(commit+'.json'),wrapper)
        save(private/'packs'/(commit+'.gates.json'),{'protocol':'oo.release-control.gate-intake/v1','config_commit':commit,'gates':[],'external':[],'checkpoints':[],'skip':[]})
        tables=[]
        for e in catalog['entries']:
            for t in e['tables']:
                tables.append({'schema':t['logical_schema'],'table':t['logical_table'],'physical_name':t['stored_physical'],'accepted_rows':t['accepted_rows'],'canonical_digest':t['canonical_digest'],'rejects':t['rejects'],'run_id':e['run_id'],'source_generation':e['source_generation'],'target_generation':e['target_generation'],'applied_pos':result['applied_pos']})
        publication={'protocol':'oo.pipeline.native-publication-receipt/v1','source_id':result['source_id'],'source_generation':generation,'target_generation':result['target_generation'],'source_epoch':result['source_epoch'],'position_codec':'mysql-gtid/v1','position_lineage':result['source_instance']['measured_id'],'applied_pos':result['applied_pos'],'consistency':'transactional','consistency_grade':'T','cross_table_consistent':True,'run_id':tables[0]['run_id'],'run_ids':[t['run_id'] for t in tables],'tables':tables}
        save(self.out/'publication.json',publication)
        runtime=load(self.p['release_runtime'])
        runtime['native_mapping']={'datasets':[{'dataset':v['dataset'],'source':v['source_id'],'config_digest':v['config_digest'],'generation':v['source_generation']} for v in catalog['binding_vector']],'publication':{'protocol':publication['protocol'],'run_id':publication['run_id'],'receipt_sha256':hashlib.sha256(canonical(publication)).hexdigest(),'receipt':publication}}
        runtime_file=private/('runtime-gen'+str(generation)+'-'+self.out.name+'.json');save(runtime_file,runtime)
        self.replace_api(self.p['release_container'],self.p['release_binary_container'],'/owned/'+str(runtime_file.relative_to(Path(self.op['host_secret_root']))),'release')
        c=load(self.p['product_config']);c['endpoint']=self.p['release_origin'];c['origin']=c['endpoint']
        flow=Flow(c,self.out/'release')
        active=flow.call('admin','/control/v1/releases/active?project_id='+c['create']['project_id'],name='previous-active')
        version='mysql-c1-g'+str(generation)+'-'+self.p['release_suffix']
        created=flow.call('admin','/control/v1/releases',{'project_id':c['create']['project_id'],'config_commit':commit,'candidate_id':version,'version':version,'base_version':active['version'],'role':wrapper['visibility']['role'],'scope':'test_scope','expected_version':active['version']},'publish')
        deadline=time.monotonic()+90
        while True:
            status=flow.call('admin','/control/v1/releases/'+created['operation_id'],name='publish-status')
            if status['status'] in ['succeeded','failed']:
                break
            if time.monotonic()>deadline:
                raise RuntimeError('publish_deadline')
            time.sleep(1)
        save(self.out/'release-status.json',status)
        if status['status']!='succeeded' or status['half_published'] or status['pack_digest']!=native['pack_digest']:
            raise RuntimeError('formal_publication_refused')
        active=flow.call('admin','/control/v1/releases/active?project_id='+c['create']['project_id'],name='current-active')
        if active['version']!=version or active['pack_digest']!=native['pack_digest']:
            raise RuntimeError('formal_active_pack')
        save(self.out/'active-release.json',active)
        return native

    def gateway(self, generation, catalog, native):
        product=self.out/'product';visibility=load(product/'current-visibility.json');effective=load(product/'current-effective-oos.json')
        vector={v['dataset']:v for v in catalog['binding_vector']}
        role=visibility['role'];login=self.p['gateway_login']
        previous=Path(self.p['previous_gateway'])
        before=load(previous/'reviewed/mysql_full/native-input.json')['catalog']['binding_vector']
        if {v['source_id'] for v in before}!={v['source_id'] for v in catalog['binding_vector']}:
            raise ValueError('previous_gateway_source')
        for old in before:
            if old['stored_physical'] in {v['stored_physical'] for v in catalog['binding_vector']}:
                continue
            request={'protocol':'oo.release-coordinator/target-close/v1','schema_name':'_oo','relation_name':old['stored_physical'],'role_name':role}
            self.call('retire-previous-shadow',['docker','exec','-i',self.op['container'],self.p['coordinator_binary'],'target-close','--config',self.p['coordinator_config_container']],canonical(request))
        datasets={d['dataset']:d for d in effective['datasets']};policies=[]
        for d in visibility['datasets']:
            context=vector[d['dataset']];schema,table=d['dataset'].split('.');cols={c['name']:c for c in datasets[d['dataset']]['columns']}
            selected=[]
            for name in d['columns']:
                c=cols[name]
                if c['pii']!='none' or c['classification']['state']!='confirmed':
                    raise ValueError('reviewed_column')
                selected.append({'name':name,'status':'confirmed','semantic_type':c['semantic_type'],'pii':{'class':'none'}})
            policies.append({'project':context['source_id'],'config_digest':context['config_digest'],'generation':context['source_generation'],'role':role.removeprefix('oo_r_'),'login':login,'table':{'namespace':schema,'name':table,'columns':selected}})
        mapping=load(self.p['mapping_template']);mapping['policies']=policies
        save(self.out/'mapping-request.json',mapping)
        self.call('mapping-reader',['docker','exec','-i',self.op['container'],self.p['mapping_binary']],canonical(mapping))
        gateway=Path(self.p['bundle_parent'])/('gateway-gen'+str(generation)+'-'+self.out.name)
        gateway.mkdir(mode=0o700,exist_ok=False);(gateway/'packs').mkdir(mode=0o700);folder=gateway/'reviewed/mysql_full';folder.mkdir(mode=0o700,parents=True)
        env=load(product/'current-role-response.json')['response'];artifact=Path(load(self.p['product_config'])['work_dir_host'])/'builds'/env['build_id']/'native-maintenance'/native['artifact_revision']
        command=load(artifact/'command.json');reviewed=load(artifact/'reviewed.json');first=catalog['binding_vector'][0]
        binding={'project':visibility['project'],'ontology_version':effective['ontology_version'],'catalog_version':visibility['catalog_version'],'policy_id':visibility['policy']['id'],'policy_version':visibility['policy']['version'],'knowledge_id':visibility['knowledge']['id'],'knowledge_version':visibility['knowledge']['version'],'source_id':first['source_id'],'schema':visibility['datasets'][0]['dataset'].split('.')[0],'schema_hash':reviewed['provenance']['collection_sha256'],'artifact_sha256':hashlib.sha256(canonical(command['catalog'])).hexdigest(),'config_digest':first['config_digest'],'generation':generation,'role':role.removeprefix('oo_r_'),'login':login,'allow':[{'dataset':d['dataset'],'column':name,'explicit_allow':False} for d in visibility['datasets'] for name in d['columns']],'binding_vector':catalog['binding_vector']}
        shutil.copy2(artifact/'command.json',folder/'native-input.json');shutil.copy2(artifact/'reviewed.json',folder/'command.json');save(folder/'binding.json',binding);shutil.copy2(artifact/'role-pack.json',gateway/'packs/mysql_full.json')
        for name in ['ca.crt','runtime.pass','token-admin.pass','endpoint.json','data.pass','config.json']:
            shutil.copy2(previous/name,gateway/name)
        shutil.copy2(self.p['gateway_data_ca'],gateway/'data-ca.crt');save(gateway/'data-endpoint.json',self.p['gateway_data_endpoint'])
        (gateway/'wal').mkdir(mode=0o700)
        config=load(gateway/'config.json')
        save(gateway/'grant.json',{'kind':'client','project_id':visibility['project'],'principal_id':'fixture-admin','role':role,'policy_version':visibility['policy']['version'],'knowledge_version':visibility['knowledge']['version'],'audience_url':config['audience'],'claims':[],'client_ttl_ms':3600000})
        for file in gateway.rglob('*'):
            file.chmod(0o700 if file.is_dir() else 0o600)
        container=self.p['gateway_container_prefix']+str(generation)
        self.call('gateway-create',['docker','run','-d','--name',container,'--network',self.p['gateway_networks'][0],'--memory','2g','-v',str(gateway)+':/run-current','-v',self.p['fixed_bin_root']+':/fixed:ro',self.p['gateway_image'],'sleep','infinity'])
        for network in self.p['gateway_networks'][1:]:
            self.call('gateway-network',['docker','network','connect',network,container])
        gw_binary=Path(self.p['gateway_binary']);gw_bin='/fixed/'+gw_binary.parent.name+'/'+gw_binary.name
        token_binary=Path(self.p['token_binary']);token_bin='/fixed/'+token_binary.parent.name+'/'+token_binary.name
        self.call('issue-token',['docker','exec',container,token_bin,'issue','/run-current/endpoint.json','/run-current/token-admin.pass','/run-current/ca.crt','/run-current/grant.json','/run-current/current-client.token'])
        proc=subprocess.Popen(['docker','exec',container,gw_bin,'/run-current'],stdout=(self.out/'gateway.serve.stdout').open('wb'),stderr=(self.out/'gateway.serve.stderr').open('wb'))
        save(self.out/'gateway-handle.json',{'container':container,'docker_exec_pid':proc.pid,'binary':str(gw_binary),'private_root':str(gateway),'source_generation':generation,'pack_digest':native['pack_digest'],'data_endpoint':self.p['gateway_data_endpoint']})
        deadline=time.monotonic()+20
        while not (gateway/'listen.addr').exists():
            if proc.poll() is not None or time.monotonic()>deadline:
                raise RuntimeError('gateway_start')
            time.sleep(.1)
        self.preflight(container,gateway,generation,catalog,native)
        return gateway

    def preflight(self,container,gateway,generation,catalog,native):
        vector={v['dataset']:v for v in catalog['binding_vector']}
        role=load(self.out/'product/current-visibility.json')['role']
        host='http://'+(gateway/'listen.addr').read_text().strip();cli=Path(self.p['cli_binary']);base=['docker','exec',container,'/fixed/'+cli.parent.name+'/'+cli.name]
        flags=['--json','--auth','external','--token-file','/run-current/current-client.token','--gateway',host,'--transport','local-development']
        checks=[('tables',['tables']),('freshness',['freshness'])]
        for dataset in vector:
            if not all(part.replace('_','').isalnum() for part in dataset.split('.')):
                raise ValueError('dataset_identifier')
            checks += [('describe',['describe',dataset]),('count',['query','SELECT COUNT(*) AS n FROM '+dataset])]
        checks += [('current',['query',sql]) for sql in self.p['preflight_sql']]
        for name,args in checks:
            response=json.loads(self.call('preflight-'+name,base+args+flags))
            if any(w['code']=='STALE_DATA' for w in response.get('warnings',[])):
                raise RuntimeError('gateway_not_current')
            if name=='freshness' and (response['publication']!='live' or len(response['freshness'])!=len(vector) or not response['sources'] or any(r['state']!='live' for r in response['freshness'])):
                raise RuntimeError('gateway_not_ready')
        save(self.out/'gateway-ready.json',{'source_generation':generation,'pack_digest':native['pack_digest'],'role':role,'token_sha256':digest(gateway/'current-client.token'),'gateway_container':container,'private_root':str(gateway),'readonly_replica':True,'ready':True})

    def maintain(self):
        identity = load(Path(self.op['prepared_root'])/'prepared/identity.json')
        source = identity['source_id']
        exported = self.out/'native'
        receipt = json.loads(self.worker('native-artifacts','--binding',identity['binding_id'],'--out',str(exported)))
        request, result, ack = [load(exported/(name+'.json')) for name in ['request','load','ack']]
        generation = receipt['source_generation']
        if request['source_id'] != source or result['source_id'] != source or result['source_generation'] != generation:
            raise ValueError('cut_identity')
        entries = []
        for n,(spec, native, loaded) in enumerate(zip(request['entries'],request['native_requests'],result['entries'],strict=True)):
            if native['logical_schema'] != spec['schema'] or native['logical_table'] != spec['table'] or loaded['schema'] != spec['schema'] or loaded['table'] != spec['table']:
                raise ValueError('table_identity')
            query = {key:native[key] for key in ['source_id','source_generation','config_digest','schema_digest','type_digest','input_digest','snapshot','target']}
            query['target']={k:v for k,v in query['target'].items() if k not in ['owner','shard']}
            query.update(protocol='oo.pipeline.native-sealed-catalog/v1',run_id=loaded['run_id'],owner=identity['binding_id'],target_generation=result['target_generation'])
            if digest(spec['admission_registry_path']) != spec['admission_registry_sha256']:
                raise ValueError('registry_digest')
            raw = self.call('catalog-'+str(n),['docker','exec','-i','-e','OO_NATIVE_ADMISSION_BIN='+self.op['admission'],'-e','OO_NATIVE_BINDING_REGISTRY='+spec['admission_registry_path'],'-e','OO_NATIVE_BINDING_REGISTRY_SHA256='+spec['admission_registry_sha256'],self.op['container'],self.op['loader'],'catalog-import-native'],canonical(query))
            entry = json.loads(raw)
            for key in ['source_id','source_generation','target_generation','run_id','config_digest','type_digest','input_digest']:
                if entry[key] != query[key]:
                    raise ValueError('export_identity')
            entries.append(entry)
        catalog = collection(entries,[(s['schema'],s['table']) for s in request['entries']])
        save(self.out/'catalog.json',catalog)
        self.call('oracle',['python3',str(REPO/'tools/mysql-preview/audit-native-target.py'),'--generation',str(generation),'--source-id',source,'--source-container',self.p['source_container'],'--source-client-file',self.p['source_client_file'],'--schema',self.c['database']])
        self.call('profile',['python3',str(REPO/'tools/mysql-preview/profile-fixture.py'),'--container',self.p['source_container'],'--client-file',self.p['source_client_file'],'--out',str(self.out/'profile.json'),'--generation',str(generation),'--source-id',source,'--selection-config',str(Path(self.op['prepared_root'])/'prepared/native-config.json')])
        profile=load(self.out/'profile.json')
        if profile['source_gtid'] != result['l_end']:
            raise ValueError('source_changed_before_maintenance')
        seed = Path(self.p['seed_bundle'])
        semantics = load(seed/'source_semantics.json')
        semantics.update(source_generation=generation,target_generation=result['target_generation'],applied_pos=result['applied_pos'],source_epoch=result['source_epoch'],collection_sha256=digest(self.out/'catalog.json'),binding_vector_sha256=hashlib.sha256(canonical(catalog['binding_vector'])).hexdigest())
        semantics['quality'].update(rows=sum(t['accepted_rows'] for e in entries for t in e['tables']),tables=len(entries),columns=sum(len(t['columns']) for e in entries for t in e['tables']),full_typed_cell_differences=0,zero_date_rejects=sum(len(t['rejects']) for e in entries for t in e['tables']))
        semantics['evidence']={'oracle_stdout_sha256':digest(next(self.out.glob('*-oracle.stdout'))),'request_sha256':digest(exported/'request.json'),'load_sha256':digest(exported/'load.json'),'ack_sha256':digest(exported/'ack.json')}
        semantics['continuous_capture']['tested_gtid_transactions'].append(int(result['applied_pos']))
        product_config=load(self.p['product_config'])
        build=load(Path(self.p['reuse_product'])/'product-receipt.json')['build_id']
        evidence_id=hashlib.sha256(''.join(digest(exported/(n+'.json')) for n in ['request','load','ack']).encode()).hexdigest()
        evidence=Path(product_config['work_dir_host'])/'builds'/build/'source-evidence'/evidence_id
        evidence.mkdir(mode=0o700,parents=True,exist_ok=False)
        for name in ['request','load','ack']:
            shutil.copy2(exported/(name+'.json'),evidence/(name+'.json'))
        semantics['evidence_set_id']=evidence_id
        semantics['c1_evidence']={n:{'path':str(Path('source-evidence')/evidence_id/(n+'.json')),'sha256':digest(evidence/(n+'.json'))} for n in ['request','load','ack']}
        semantics['capture_methods']={v['dataset']:{'logical_table':v['dataset'],'capture_method':'c1_cdc','consistency':'T','delete_detection':'native','source_generation':v['source_generation'],'target_generation':v['target_generation']} for v in catalog['binding_vector']}
        save(self.out/'source-semantics.json',semantics)
        bundle=Path(self.p['bundle_parent'])/('product-bundle-gen'+str(generation)+'-'+self.out.name)
        bundle.mkdir(mode=0o700,exist_ok=False)
        for file in seed.iterdir():
            if file.name not in ['manifest.json','catalog.json','profile.json','source_semantics.json']:
                shutil.copy2(file,bundle/file.name)
        shutil.copy2(self.out/'catalog.json',bundle/'catalog.json');shutil.copy2(self.out/'profile.json',bundle/'profile.json');shutil.copy2(self.out/'source-semantics.json',bundle/'source_semantics.json')
        save(bundle/'manifest.json',{'contract':'oo.selfbuild-control-api/sealed-bundle/v1','files':{f.name:digest(f) for f in sorted(bundle.iterdir())}})
        runtime=load(self.p['api_runtime'])
        runtime['projects'][0]['sealed_bundle']='/owned/'+str(bundle.relative_to(Path(self.op['host_secret_root'])))
        runtime_file=Path(self.p['bundle_parent'])/('runtime-gen'+str(generation)+'-'+self.out.name+'.json');save(runtime_file,runtime)
        self.replace_api(self.p['api_container'],self.p['api_binary_container'],'/owned/'+str(runtime_file.relative_to(Path(self.op['host_secret_root']))),'selfbuild')
        product_config['sealed_bundle_host']=str(bundle)
        product_out=self.out/'product';product_out.mkdir(mode=0o700)
        product_config['_sha256']=digest(self.p['product_config'])
        flow=Flow(product_config,product_out)
        flow.run(reuse_product=self.p['reuse_product'],reuse_receipt=self.p['reuse_receipt'],current_source_semantics=str(self.out/'source-semantics.json'))
        save(self.out/'maintenance-complete.json',{'source_id':source,'source_generation':generation,'s5_reused':True,'maintenance_receipt_sha256':digest(product_out/'maintenance-consumption.json'),'catalog_sha256':digest(self.out/'catalog.json')})
        return generation, catalog, result


def run(config,out):
    return Follow(load(config),out).maintain()


if __name__=='__main__':
    run(*sys.argv[1:])
