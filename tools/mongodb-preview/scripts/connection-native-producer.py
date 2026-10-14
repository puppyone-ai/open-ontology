#!/usr/bin/python3
"""Source-owned real C5 producer for native jobs; no leases or trust synthesis.

Run in the pinned Linux operator runtime sharing the owned Mongo network.
PostgreSQL JSONB roundtrips use verified TLS and SCRAM on the owned target.
All credentials stay in private files; public Agent never sees this operator
capability.
"""
import base64, hashlib, hmac, json, os, pathlib, socket, ssl, struct, subprocess, sys, uuid

def exact(connection,count):
    value=b''
    while len(value)<count:
        part=connection.recv(count-len(value))
        if not part:raise RuntimeError('target PostgreSQL disconnected')
        value+=part
    return value

def message(connection):
    tag=exact(connection,1); length=struct.unpack('>I',exact(connection,4))[0]
    if length<4 or length>32*1024*1024:raise RuntimeError('target protocol size')
    return tag,exact(connection,length-4)

def send(connection,tag,body):connection.sendall(tag+struct.pack('>I',len(body)+4)+body)

def pg_select(sql,target):
    connection=socket.create_connection((target['host'],target['port']),timeout=30)
    connection.sendall(struct.pack('>II',8,80877103))
    if exact(connection,1)!=b'S':raise RuntimeError('target TLS unavailable')
    context=ssl.create_default_context(cafile=target['ca_file'])
    connection=context.wrap_socket(connection,server_hostname=target['host'])
    user=target['runtime_principal'];password=pathlib.Path(target['password_file']).read_text().strip()
    startup=struct.pack('>I',196608)+b'user\0'+user.encode()+b'\0database\0'+target['database'].encode()+b'\0application_name\0oo-mongodb-c5-roundtrip\0\0'
    connection.sendall(struct.pack('>I',len(startup)+4)+startup)
    nonce=uuid.uuid4().hex;first='n='+user+',r='+nonce;server_signature=None;authenticated=False
    while True:
        tag,body=message(connection)
        if tag==b'E':raise RuntimeError('target authentication refused')
        if tag==b'R':
            auth=struct.unpack('>I',body[:4])[0]
            if auth==10:
                if b'SCRAM-SHA-256\0' not in body[4:]:raise RuntimeError('target SCRAM unavailable')
                initial=('n,,'+first).encode();send(connection,b'p',b'SCRAM-SHA-256\0'+struct.pack('>I',len(initial))+initial)
            elif auth==11:
                server_first=body[4:].decode();attributes=dict(item.split('=',1) for item in server_first.split(','))
                if not attributes['r'].startswith(nonce) or len(attributes['r'])<=len(nonce):raise RuntimeError('target SCRAM nonce')
                iterations=int(attributes['i'])
                if not 4096<=iterations<=1000000:raise RuntimeError('target SCRAM iteration bound')
                salted=hashlib.pbkdf2_hmac('sha256',password.encode(),base64.b64decode(attributes['s']),iterations)
                client_key=hmac.new(salted,b'Client Key',hashlib.sha256).digest();stored_key=hashlib.sha256(client_key).digest()
                final='c=biws,r='+attributes['r'];auth_message=(first+','+server_first+','+final).encode()
                signature=hmac.new(stored_key,auth_message,hashlib.sha256).digest();proof=bytes(a^b for a,b in zip(client_key,signature,strict=True))
                server_signature=hmac.new(hmac.new(salted,b'Server Key',hashlib.sha256).digest(),auth_message,hashlib.sha256).digest()
                send(connection,b'p',(final+',p='+base64.b64encode(proof).decode()).encode())
            elif auth==12:
                attributes=dict(item.split('=',1) for item in body[4:].decode().split(','))
                if server_signature is None or not hmac.compare_digest(base64.b64decode(attributes.get('v','')),server_signature):raise RuntimeError('target SCRAM server signature')
            elif auth==0:authenticated=True
            else:raise RuntimeError('target authentication method unsupported')
        if tag==b'Z':
            if not authenticated:raise RuntimeError('target authentication incomplete')
            break
    send(connection,b'Q',sql.encode()+b'\0');rows=[]
    while True:
        tag,body=message(connection)
        if tag==b'E':raise RuntimeError('target JSONB SELECT refused')
        if tag==b'D':
            count=struct.unpack('>H',body[:2])[0]
            if count!=1:raise RuntimeError('target JSONB row shape')
            length=struct.unpack('>i',body[2:6])[0]
            if length<0:raise RuntimeError('target JSONB null')
            rows.append(json.loads(body[6:6+length]))
        if tag==b'Z':break
    send(connection,b'X',b'');connection.close()
    if len(rows)!=1:raise RuntimeError('target JSONB result count')
    return rows[0]

def pg_jsonb(document,config):
    text=json.dumps(document,separators=(',',':'))
    sql="SELECT json_build_object('text',j::text,'hex',encode(jsonb_send(j),'hex')) FROM (SELECT '"+text.replace("'","''")+"'::jsonb j) t;"
    return pg_select(sql,config['target'])

def run(command,directory,name,env=None):
    result=subprocess.run(command,capture_output=True,env=env)
    (directory/(name+'.stdout')).write_bytes(result.stdout);(directory/(name+'.stderr')).write_bytes(result.stderr);(directory/(name+'.exit')).write_text(str(result.returncode)+'\n')
    if result.returncode:raise RuntimeError(name+' refused: '+str(result.returncode))

def write(path,value):
    path.write_text(json.dumps(value,sort_keys=True,separators=(',',':'))+'\n')

def acknowledged_generation(job):
    # A local file or occurrence pin is never proof that the target commit
    # completed. The worker supplies these values from its successful CAS.
    if 'acknowledged_source_generation' not in job or 'acknowledged_pin' not in job:
        raise RuntimeError('acknowledged watermark absent')
    generation=job['acknowledged_source_generation'];pin=job['acknowledged_pin']
    if generation is None and pin is None:return 0
    if type(generation) is not int or generation<1 or type(pin) is not int or pin<1:
        raise RuntimeError('acknowledged watermark malformed')
    if pin>=job['pin']:
        raise RuntimeError('acknowledged pin is not prior')
    return generation

def bson_field_types(hex_document):
    """Read top-level BSON wire tags without converting values to JSON types."""
    data=bytes.fromhex(hex_document.strip())
    if len(data)<5 or struct.unpack_from('<i',data)[0]!=len(data) or data[-1]!=0:
        raise RuntimeError('BSON framing')
    fields={};at=4;end=len(data)-1
    while at<end:
        tag=data[at];at+=1
        stop=data.find(b'\0',at,end)
        if stop<0:raise RuntimeError('BSON field name')
        name=data[at:stop].decode('utf-8');at=stop+1
        if not name or name in fields:raise RuntimeError('BSON duplicate field')
        item={'variant':f'{tag:02x}'}
        if tag in (0x01,0x09,0x11,0x12):at+=8
        elif tag in (0x07,):at+=12
        elif tag in (0x13,):at+=16
        elif tag in (0x08,):at+=1
        elif tag in (0x10,):at+=4
        elif tag in (0x0a,0x06,0x7f,0xff):pass
        elif tag in (0x02,0x0d,0x0e):
            if at+4>end:raise RuntimeError('BSON string length')
            length=struct.unpack_from('<i',data,at)[0]
            if length<1 or at+4+length>end or data[at+4+length-1]!=0:raise RuntimeError('BSON string')
            at+=4+length
        elif tag in (0x03,0x04):
            if at+4>end:raise RuntimeError('BSON nested length')
            length=struct.unpack_from('<i',data,at)[0]
            if length<5 or at+length>end or data[at+length-1]!=0:raise RuntimeError('BSON nested')
            at+=length
        elif tag==0x05:
            if at+5>end:raise RuntimeError('BSON binary length')
            length=struct.unpack_from('<i',data,at)[0]
            if length<0 or at+5+length>end:raise RuntimeError('BSON binary')
            item['subtype']=f'{data[at+4]:02x}';at+=5+length
        else:raise RuntimeError(f'unsupported BSON tag {tag:02x}')
        if at>end:raise RuntimeError('BSON overrun')
        fields[name]=item
    if at!=end:raise RuntimeError('BSON trailing bytes')
    return fields

def batch_id(source_id, generation):
    """Stable, generation-scoped native batch token accepted by the catalog."""
    if not isinstance(source_id,str) or not source_id or not isinstance(generation,int) or generation<1:
        raise RuntimeError('batch identity')
    return f"{uuid.uuid5(uuid.NAMESPACE_URL, source_id + ':' + str(generation))}_{generation}"

def diff_rows(rows):
    out=[]
    for row in rows:
        text=row['pg_jsonb_text']
        key=json.loads(text)['_id']['$oid']
        if not isinstance(key,str) or not key:raise RuntimeError('diff key')
        out.append({'key':key,'canonical':text})
    if len({item['key'] for item in out})!=len(out):raise RuntimeError('diff key')
    return out

def official_diff(config, directory, before_rows, after_rows):
    # Full-snapshot import refuses a nonempty tombstone list. This witness is the
    # official two-read diff; deletes are absent keys, and the loader list stays empty.
    request={'before':diff_rows(before_rows),'after':diff_rows(after_rows),'tombstones':[]}
    write(directory/'diff-sync-request.json', request)
    run([config['preview_bin'],'--diff-sync','--request',str(directory/'diff-sync-request.json'),'--out',str(directory/'diff-sync.json')], directory, 'diff-sync')
    witness=json.loads((directory/'diff-sync.json').read_text())
    if witness.get('protocol')!='oo.mongodb-preview.sync-diff/v1' or witness.get('converged') is not True or not isinstance(witness.get('deletes'),list):
        raise RuntimeError('official diff witness refused')
    if witness.get('import_invoked') is not False or witness.get('generation_created') is not False:
        raise RuntimeError('diff witness invoked import')
    return witness

def main():
    config=json.loads(pathlib.Path(sys.argv[1]).read_text()); job=json.load(sys.stdin)
    if job['source_id']!=config['source_id'] or job['method']!='C5':raise RuntimeError('source identity')
    directory=pathlib.Path(config['captures'])/job['binding']/str(job['pin'])
    if (directory/'request.json').is_file():
        print((directory/'request.json').read_text().strip());return
    parent=acknowledged_generation(job)
    generation=parent+1
    if directory.exists():directory.rename(directory.with_name(directory.name+'.failed-'+str(uuid.uuid4())))
    directory.mkdir(parents=True,exist_ok=False)
    customer=json.loads(pathlib.Path(config['customer']).read_text())
    database,collection=customer['batch_object'].split('.',1)
    if database!=customer['namespace']:raise RuntimeError('source namespace mismatch')
    environment=dict(os.environ,OO_C5_CAPTURE_ONLY='1',OO_OUT=str(directory),OO_W141_CUSTOMER=config['customer'],OO_W141_SECRET_REGISTRY=config['secret_registry'],OO_W141_OPMSG_LOG=str(directory/'opmsg-names.jsonl'),LD_PRELOAD=config['wire_library'])
    if config.get('connection'):environment['OO_MONGO_CONNECTION']=config['connection']
    run([config['driver_bin']],directory,'driver',environment)
    run([config['preview_bin'],'--decode-docs','--hex',str(directory/'full-approved.bson.hex'),'--out',str(directory/'decode.json')],directory,'decode')
    rows=json.loads((directory/'decode.json').read_text())['rows'];raw=(directory/'full-approved.bson.hex').read_text().splitlines(); seal=[];canonical=[]
    for i,(row,hex_bson) in enumerate(zip(rows,raw,strict=True)):
        pg=pg_jsonb(row['extended_json'],config)
        seal.append({'name':row['extended_json']['_id']['$oid'],'raw_bson_hex':hex_bson,'pg_jsonb_text':pg['text'],'pg_jsonb_hex':pg['hex']})
        canonical.append({'index':i,'pg_jsonb_text':pg['text']})
    write(directory/'seal-request.json',{'rows':seal});write(directory/'canonical.json',{'rows':canonical})
    run([config['preview_bin'],'--seal-docs','--request',str(directory/'seal-request.json'),'--out',str(directory/'seal.json')],directory,'seal')
    if any(row['outcome']!='sealed' for row in json.loads((directory/'seal.json').read_text())['rows']):raise RuntimeError('document seal refused')
    if parent:
        previous=directory.parent/str(job['acknowledged_pin'])/'canonical.json'
        if not previous.is_file():raise RuntimeError('parent canonical absent')
        before=json.loads(previous.read_text())['rows']
    else:before=[]
    official_diff(config, directory, before, canonical)
    arrow_environment=dict(os.environ,FROZEN_ARROW=str(directory/'typed-19.arrow'),CANONICAL_JSON=str(directory/'canonical.json'),OUT_ARROW=str(directory/'full-20.arrow'),OUT_STREAM=str(directory/'full-20.ipc'),OUT_IDENTITY=str(directory/'identity.json'))
    run([config['doc_arrow_bin']],directory,'arrow20',arrow_environment)
    schema=json.loads(pathlib.Path(config['schema']).read_text());types=json.loads(pathlib.Path(config['types']).read_text())
    observed=[bson_field_types(value) for value in raw]
    for row,tags in zip(rows,observed,strict=True):
        if set(row['keys'])!=set(tags):raise RuntimeError('BSON decoder field mismatch')
    for name,expected in {'_id':{'variant':'07'},'uid':{'variant':'05','subtype':'04'},'blob':{'variant':'05','subtype':'00'}}.items():
        if any(item.get(name)!=expected for item in observed):raise RuntimeError('mixed or unexpected BSON type: '+name)
    for column in schema[0]['columns']+types:
        seen=[item[column['name']] for item in observed if column['name'] in item]
        variants=sorted({json.dumps(item,sort_keys=True,separators=(',',':')) for item in seen})
        column['bson_types']=[json.loads(item) for item in variants]
        column['bson_present_rows']=len(seen)
        if len(variants)==1:column['bson_type']=json.loads(variants[0])
    write(directory/'schema.json',schema);write(directory/'types.json',types)
    snapshot=hashlib.sha256((directory/'full-approved.bson.hex').read_bytes()).hexdigest();census=json.loads((directory/'c5-census.json').read_text())
    commands=json.loads((directory/'commands.json').read_text())
    writes=sum(name in {'insert','update','delete','findAndModify','create','drop','createIndexes','collMod','dropDatabase'} for name in commands['names'])
    if writes:raise RuntimeError('source reader write attempt')
    tick={'protocol':'oo.mongodb.capture-tick/v1','engine':'mongodb','source_id':config['source_id'],'label':snapshot,'database':database,'logical_table':collection,'consistency':'B','route':'C5','delete_mode':'reconcile','quality':['DELETES_LAGGED'],'product_reader_write_attempts':writes,'snapshot_token':False,'cursor_safety':'no_cursor','arrow':'full-20.ipc','rows':[{'key':r['name']} for r in seal],'source_rows':census['source_rows'],'metadata':census['metadata'],'reader_commands_sha256':hashlib.sha256((directory/'commands.json').read_bytes()).hexdigest()}
    write(directory/'capturer.json',tick)
    witness={'protocol':'oo.source-admission.capture-witness/v1','source_engine':'mongodb','source_id':config['source_id'],'source_generation':generation,'snapshot':{'kind':'seal','value':snapshot},'run_id':str(uuid.uuid4()),'logical_schema':database,'logical_table':collection,'capturer_path':str(directory/'capturer.json'),'arrow_path':str(directory/'full-20.ipc'),'driver_path':config['driver_crate'],'schema_preimage_path':str(directory/'schema.json'),'type_preimage_path':str(directory/'types.json')};write(directory/'witness.json',witness)
    report=json.loads(pathlib.Path(config['report']).read_text());report['capture_witness']=str(directory/'witness.json');write(directory/'report.json',report)
    run([config['admission_bin'],'seal-native','--report',str(directory/'report.json'),'--config',config['admission_config'],'--out',str(directory/'admitted')],directory,'admission')
    registry=json.loads((directory/'admitted/registry.json').read_text())['bindings'][0]; receipt=directory/'admitted/admission-receipt.json'
    request={'protocol':'oo.pipeline.native-update/v1','source_engine':'mongodb','source_id':config['source_id'],'source_generation':generation,'capture_method':'c5_diff','delete_detection':'reconcile','consistency_grade':'B','cross_table_consistent':False,'position_codec':'snapshot-zero/v1','position_lineage':'native-update.batch','applied_pos':'0','batch_id':batch_id(config['source_id'],generation),'logical_schema':database,'logical_table':collection,'columns':schema[0]['columns'],'key_column':'_id','row_identity':'unique','snapshot':witness['snapshot'],'arrow_path':str(directory/'full-20.ipc'),'registry_path':str(directory/'admitted/registry.json'),'admission_path':str(receipt),'admission_expect':{'declared_receipt_sha256':hashlib.sha256(receipt.read_bytes()).hexdigest()},'audit_digest':registry['capture_receipt_sha256'],'config_digest':registry['config_digest'],'schema_digest':registry['schema_digest'],'type_digest':registry['type_digest'],'input_digest':registry['input_digest'],'target':config['target'],'tombstones':[],'mirror':True}
    if parent:request.update(parent_source_generation=parent,full_snapshot=True)
    write(directory/'request.json',request)
    print(json.dumps(request,separators=(',',':')))

def _self_check():
    before=[{'pg_jsonb_text':'{"_id":{"$oid":"aa"},"code":"A"}'},{'pg_jsonb_text':'{"_id":{"$oid":"bb"},"code":"B"}'}]
    after=[{'pg_jsonb_text':'{"_id":{"$oid":"bb"},"code":"C"}'},{'pg_jsonb_text':'{"_id":{"$oid":"cc"},"code":"D"}'}]
    request={'before':diff_rows(before),'after':diff_rows(after),'tombstones':[]}
    assert [item['key'] for item in request['before']]==['aa','bb']
    assert request['after'][1]['canonical'].endswith('"D"}') and request['tombstones']==[]
    try:diff_rows([{'pg_jsonb_text':'{"_id":{"$oid":"aa"}}'},{'pg_jsonb_text':'{"_id":{"$oid":"aa"}}'}])
    except RuntimeError:pass
    else:raise AssertionError('duplicate key')

if __name__=='__main__':
    if len(sys.argv)==2 and sys.argv[1]=='--self-check':
        _self_check();print('ok');raise SystemExit(0)
    main()
