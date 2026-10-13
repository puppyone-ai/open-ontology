#!/usr/bin/env python3
"""Actual C5 polling; consumer command receives immutable cycle directory.

No lease/receipt/Pack is fabricated. A missing or failed consumer leaves captured
changes unapplied. Config reload changes polling frequency between cycles.
"""
import argparse, hashlib, json, pathlib, subprocess, time

APPROVED = {'_id','code','flag','units','price','created','uid','blob','weight','warehouse','whCode','lane','memo','marks','pack'}

def canonical(value):
    return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False)

def reconcile(previous, approved, refused):
    refused_keys={row['key'] for row in refused}
    if len(refused_keys)!=len(refused) or refused_keys & set(approved):
        raise RuntimeError('ambiguous scope-refused keys')
    quarantined=sorted(set(previous)&refused_keys)
    diff={'inserts':sorted(set(approved)-set(previous)),
          'updates':sorted(k for k in set(approved)&set(previous) if canonical(approved[k])!=canonical(previous[k])),
          'deletes':sorted(set(previous)-set(approved)-refused_keys)}
    return diff, quarantined

def held_scope_keys(protected, newly_quarantined, refused):
    # Continue to hold a key across repeated refused reads. A following
    # approved row or actual absence resolves the hold through a full read.
    return (protected | set(newly_quarantined)) & {row['key'] for row in refused}

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--config',required=True)
    parser.add_argument('--out',required=True)
    parser.add_argument('--cycles',type=int,required=True)
    args=parser.parse_args()
    out=pathlib.Path(args.out); out.mkdir(parents=True,exist_ok=False)
    previous={}; applied={}; tombstones=set(); protected=set()
    for cycle in range(args.cycles):
        config=json.loads(pathlib.Path(args.config).read_text())
        every=config['every_ms']
        if not isinstance(every,int) or every<=0: raise ValueError('every_ms must be positive')
        container=config['container']
        if not container.startswith('oo-dl01w1-41-'): raise ValueError('owned resource required')
        started=time.monotonic(); directory=out/f'{cycle:04d}'; directory.mkdir()
        command=['docker','exec',container,'mongosh','--quiet','--norc','--file','/tmp/native-c5-capture.js']
        result=subprocess.run(command,capture_output=True)
        (directory/'capture.stdout').write_bytes(result.stdout)
        (directory/'capture.stderr').write_bytes(result.stderr)
        (directory/'capture.exit').write_text(str(result.returncode)+'\n')
        if result.returncode: raise RuntimeError('source capture failed')
        capture=json.loads(result.stdout)
        if capture['topology']!='standalone': raise ValueError('explicit standalone C5 fixture required')
        approved={}; refused=list(capture.get('scope_refused',[]))
        for row in capture['rows']:
            extra=set(row['document'])-APPROVED
            if extra:
                refused.append({'key':row['key'],'fields':sorted(extra),'loaded':False,'raw_doc_authorized':False})
            else: approved[row['key']]=row['document']
        # A row that is still present but fails field scope is not a physical
        # delete. Hold the applied version until an authorized classification
        # or a later full read proves that the key is actually absent.
        diff,newly_quarantined=reconcile(previous,approved,refused)
        protected=held_scope_keys(protected,newly_quarantined,refused)
        quarantined=sorted(protected)
        resurrected=sorted(set(approved)&tombstones)
        if resurrected: raise RuntimeError('tombstoned keys reappeared: '+str(resurrected))
        tombstones.update(diff['deletes'])
        (directory/'approved.json').write_text(canonical(approved)+'\n')
        (directory/'scope-refused.json').write_text(canonical(refused)+'\n')
        (directory/'diff.json').write_text(canonical(diff)+'\n')
        (directory/'quarantined.json').write_text(canonical(quarantined)+'\n')
        consumer=config.get('consumer_command')
        consumer_exit=None; applied_ok=False
        if consumer and not quarantined:
            if not isinstance(consumer,list) or not all(isinstance(x,str) for x in consumer): raise ValueError('consumer argv required')
            consume=subprocess.run(consumer+[str(directory.resolve())],capture_output=True)
            (directory/'consumer.stdout').write_bytes(consume.stdout)
            (directory/'consumer.stderr').write_bytes(consume.stderr)
            consumer_exit=consume.returncode
            (directory/'consumer.exit').write_text(str(consumer_exit)+'\n')
            # A process exit is only delivery evidence. The independent full
            # target oracle must exist and cover the exact input digest.
            oracle_path=directory/'target-full-oracle.json'
            if consumer_exit==0 and oracle_path.is_file():
                oracle=json.loads(oracle_path.read_text())
                applied_ok=(oracle.get('input_sha256')==hashlib.sha256((directory/'approved.json').read_bytes()).hexdigest() and oracle.get('full_unsampled') is True and oracle.get('rows_differ')==0 and oracle.get('actual_rows')==len(approved))
                if applied_ok: applied=dict(approved)
        status={'every_ms':every,'capture':'C5','source_rows':capture.get('source_rows',len(capture['rows'])),'approved_rows':len(approved),'scope_refused':len(refused),'quarantined_keys':quarantined,'consumer_blocked':bool(consumer and quarantined),'consumer_exit':consumer_exit,'applied':applied_ok,'source_acceptance':False,'approved_sha256':hashlib.sha256((directory/'approved.json').read_bytes()).hexdigest(),'started_monotonic':started,'elapsed_ms':int((time.monotonic()-started)*1000)}
        (directory/'status.json').write_text(canonical(status)+'\n')
        # Capture observations are allowed to move; publication truth only
        # moves after the independent target oracle has checked this cycle.
        previous=approved
        if cycle+1<args.cycles: time.sleep(max(0,every/1000-(time.monotonic()-started)))
    (out/'outcome.json').write_text(canonical({'captured_cycles':args.cycles,'product_applied':bool(applied),'source_acceptance':False,'consumer_required':not bool(applied)})+'\n')

if __name__=='__main__': main()
