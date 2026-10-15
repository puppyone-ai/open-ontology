#!/usr/bin/env python3
"""Private connection → pinned source-owner recipe, with immutable recovery."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time
import uuid

ENGINES = {'postgresql', 'mysql', 'oracle', 'mariadb', 'sqlserver', 'clickhouse', 'doris', 'starrocks', 'mongodb', 'opensearch'}
INSTALL_ROOT = Path.home()/'.cache/oo-source-onboarding'
FORBIDDEN = {'password', 'passwd', 'secret', 'token', 'api_key', 'authorization'}

class Refused(ValueError): pass

def require(ok, token):
    if not ok: raise Refused(token)

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def private(path):
    p = Path(path)
    require(p.is_absolute(), 'private_path')
    m = p.lstat()
    require(stat.S_ISREG(m.st_mode) and m.st_uid == os.getuid() and not m.st_mode & 0o077, 'private_file')
    return p

def pairs(items):
    out = {}
    for k,v in items:
        require(k not in out, 'duplicate_key'); out[k] = v
    return out

def load(path):
    require(Path(path).stat().st_size <= 1048576, 'document_budget')
    return json.loads(Path(path).read_text(), object_pairs_hook=pairs)

def reject_secrets(value):
    if isinstance(value, dict):
        require(not FORBIDDEN.intersection(value), 'inline_secret')
        for x in value.values(): reject_secrets(x)
    elif isinstance(value, list):
        for x in value: reject_secrets(x)

def save(path, value):
    tmp = path.with_suffix('.tmp')
    with tmp.open('w') as f:
        os.chmod(tmp, 0o600); json.dump(value, f, indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
    os.replace(tmp, path)

def pointer(doc, key, value):
    require(key.startswith('/') and '~' not in key, 'config_pointer')
    keys = key[1:].split('/')
    for k in keys[:-1]:
        require(isinstance(doc, dict) and k in doc, 'config_pointer'); doc = doc[k]
    require(isinstance(doc, dict) and keys[-1] in doc, 'config_pointer')
    doc[keys[-1]] = value

def lookup(doc, pointer_key):
    require(pointer_key.startswith('/') and '~' not in pointer_key,'config_pointer')
    for segment in pointer_key[1:].split('/'):
        require(isinstance(doc,dict) and segment in doc,'config_pointer');doc=doc[segment]
    return doc

def selected_binding(installed, recipe_path, password_file):
    # whole/awaiting_run are not the functional binding. Upgrade when a journal must freeze those flags.
    body = {'password_file':str(password_file), 'recipe':str(recipe_path), 'recipe_sha256':digest(recipe_path), 'secret_registry':installed['secret_registry']}
    return hashlib.sha256(json.dumps(body, sort_keys=True, separators=(',', ':')).encode()).hexdigest()

def resume_identity_ok(journal, identity, config_path, recipe, password_file):
    stored = journal['identity']
    for key in ('input_sha256', 'recipe_sha256', 'template_sha256', 'pins'):
        if stored.get(key) != identity[key]: return False
    if 'selected_engine_sha256' not in identity:
        return stored.get('engine_registry_sha256') == identity.get('engine_registry_sha256') and 'selected_engine_sha256' not in stored
    if stored.get('selected_engine_sha256') == identity['selected_engine_sha256']: return True
    if 'selected_engine_sha256' in stored: return False
    pointer_key = next((p for p, field in recipe['connection_map'].items() if field == 'password_file'), None)
    if pointer_key is None: return False
    return lookup(load(config_path), pointer_key) == str(password_file)

PRODUCT_FLOW = Path(__file__).resolve().with_name('product_flow.py')
DIALOGUE = Path(__file__).resolve().parents[1]/'agent-product-verifier'/'dialogue.py'
OPERATOR = Path(__file__).resolve().parents[1]/'selfbuild-control-api'/'recipes'/'operator.py'
CONSUMPTION = 'oo.source-onboarding.maintenance-consumption/v1'
S5_MODELS = {'gpt-6.1-sol','grok-4.7','grok-4.7-build'}
DIALOGUE_MODELS = {'gpt-6.1-sol','grok-4.7'}

def rel_ok(out, rel):
    p = Path(rel)
    require(not p.is_absolute() and '..' not in p.parts, 'completion_path')
    artifact = out/p
    require(artifact.is_file() and not artifact.is_symlink() and artifact.resolve().is_relative_to(out.resolve()), 'completion_reference_path')
    return artifact

def file_sha(path, token):
    require(path.is_file() and not path.is_symlink(), token)
    return digest(path)

def launched_program(argv):
    require(argv, 'executable_path')
    if Path(argv[0]).name.startswith('python') and len(argv) > 1:
        return str(Path(argv[1]).resolve())
    return str(Path(argv[0]).resolve())

def observe_children(pgid):
    """Record pinned whole programs actually spawned in this step. A copied receipt has no live command."""
    found = []
    try:
        proc = subprocess.run(['ps', '-ax', '-o', 'pgid=,command='], capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return found
    prefix = str(pgid) + ' '
    for raw in proc.stdout.splitlines():
        line = raw.strip()
        if not line.startswith(prefix): continue
        cmd = line[len(prefix):]
        for program in (PRODUCT_FLOW, DIALOGUE, OPERATOR):
            path = str(program)
            if path in cmd and path not in found: found.append(path)
    return found

def direct_step(out, recipe, journal, program, rel):
    program = str(Path(program).resolve())
    require(any(p['path'] == program and p['sha256'] == digest(program) for p in recipe['pins']), 'shared_program_unpinned')
    for step in recipe['steps']:
        rec = next((s for s in journal['steps'] if s['name'] == step['name'] and s['state'] == 'succeeded'), None)
        if not rec: continue
        live = launched_program(step['argv']) == program or program in rec.get('observed_programs', [])
        if live and any(a['path'] == rel and a['sha256'] == digest(out/rel) for a in rec['artifacts']):
            return
    raise Refused('historical_completion_attached')

def hex64(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value) is not None

def json_bytes(raw, token):
    try:
        value = json.loads(raw.decode(), object_pairs_hook=pairs)
    except (UnicodeError, json.JSONDecodeError, AttributeError):
        raise Refused(token)
    return value

COLLECTION='oo.pipeline.sealed-import-collection/v1'

def bound_schema(catalog, rebind, identity):
    """Singleton catalogs carry one schema_digest. A collection carries the producer vector, not a collapsed digest."""
    if catalog.get('protocol')==COLLECTION:
        rows, entries, echoed=catalog.get('binding_vector'), catalog.get('entries'), rebind.get('binding_vector')
        require(isinstance(rows, list) and rows and isinstance(entries, list) and isinstance(echoed, list) and len(rows)==len(entries)==len(echoed), 'maintenance_schema')
        require('schema_digest' not in catalog and hex64(catalog.get('collection_sha256')) and catalog.get('collection_sha256')==identity, 'maintenance_schema')
        source=generation=None
        digests=[]
        for row, entry, other in zip(rows, entries, echoed):
            require(isinstance(row, dict) and isinstance(entry, dict) and isinstance(other, dict), 'maintenance_schema')
            digest=row.get('schema_digest')
            require(hex64(digest) and entry.get('schema_digest')==digest==other.get('schema_digest'), 'maintenance_schema')
            require(isinstance(row.get('dataset'), str) and row['dataset'] and row.get('dataset')==other.get('dataset'), 'maintenance_schema')
            sid, gen=row.get('source_id'), row.get('source_generation')
            require(isinstance(sid, str) and sid and type(gen) is int and gen>=0 and other.get('source_id')==sid and other.get('source_generation')==gen, 'maintenance_source')
            require(source in (None, sid), 'maintenance_source')
            source, generation=sid, gen if generation is None else max(generation, gen)
            digests.append(digest)
        return source, generation, digests
    require(hex64(catalog.get('schema_digest')) and 'binding_vector' not in catalog and rebind.get('binding_vector') is None, 'maintenance_schema')
    source, generation=catalog.get('source_id'), catalog.get('source_generation')
    require(isinstance(source, str) and source and type(generation) is int and generation>=0, 'maintenance_source')
    return source, generation, None

def maintenance_consumption(product, receipt_sha, s5_pack_sha, pack_bytes, command, visibility, role_response_bytes, maintenance_response_bytes, role_exit, maintenance_exit, overlay_bytes):
    """Bind one immutable S5 receipt to this call's maintenance and role-pack responses. Three pack ids stay distinct."""
    require(role_exit == 0 and maintenance_exit == 0, 'maintenance_not_invoked')
    require(product.get('producer') == 'tools/source-onboarding/product_flow.py' and hex64(product.get('producer_sha256')), 'maintenance_not_invoked')
    role_envelope = json_bytes(role_response_bytes, 'maintenance_not_invoked')
    maintenance_envelope = json_bytes(maintenance_response_bytes, 'maintenance_not_invoked')
    overlay = json_bytes(overlay_bytes, 'maintenance_not_invoked')
    require(isinstance(role_envelope, dict) and role_envelope.get('status') == 200, 'maintenance_not_invoked')
    role = role_envelope.get('response')
    native = role.get('native_maintenance') if isinstance(role, dict) else None
    require(isinstance(role, dict) and role.get('contract') == 'oo.selfbuild-control-api/role-pack/v1' and role.get('operation') == 'compileRolePack' and isinstance(native, dict), 'maintenance_not_invoked')
    require(native.get('protocol') == 'oo.selfbuild-control-api/native-maintenance/v1', 'maintenance_not_invoked')
    require(type(native.get('llm_calls')) is int and native['llm_calls'] == 0 and type(role.get('llm_calls')) is int and role['llm_calls'] == 0, 'maintenance_not_invoked')
    require(native.get('publication') == 'not_published', 'maintenance_not_invoked')
    build = product.get('build_id')
    require(isinstance(build, str) and build and role.get('build_id') == build == native.get('build_id'), 'maintenance_not_invoked')
    require(hex64(receipt_sha) and hex64(s5_pack_sha) and s5_pack_sha == product.get('native_role_pack_sha256'), 'maintenance_not_invoked')
    try:
        pack = json.loads(pack_bytes.decode(), object_pairs_hook=pairs)
    except (UnicodeError, json.JSONDecodeError):
        raise Refused('role_pack_not_current')
    file_sha256 = hashlib.sha256(pack_bytes).hexdigest()
    digest_value = pack.get('pack_digest') if isinstance(pack, dict) else None
    require(isinstance(pack, dict) and pack.get('protocol') == 'oo.builder.role-pack/v1' and hex64(digest_value) and digest_value == native.get('pack_digest'), 'role_pack_not_current')
    http_pack = role.get('pack_sha256')
    require(hex64(http_pack), 'role_pack_not_current')
    role_name = role.get('role')
    role_request = role_envelope.get('request')
    require(isinstance(role_request, dict) and role_request.get('role') == role_name and role_request.get('policy_version') == 'oos/v1', 'role_pack_not_current')
    catalog = command.get('catalog') if isinstance(command, dict) else None
    require(isinstance(catalog, dict), 'maintenance_schema')
    require(isinstance(visibility, dict) and isinstance(role_name, str) and role_name and visibility.get('role') == role_name and role.get('policy_version') == 'oos/v1', 'role_pack_not_current')
    identity = visibility.get('catalog_version')
    rebind = native.get('source_rebind')
    revision = native.get('artifact_revision')
    require(hex64(identity) and isinstance(rebind, dict) and hex64(rebind.get('catalog_sha256')) and hex64(revision), 'maintenance_not_invoked')
    source_semantics_sha = rebind.get('source_semantics_sha256')
    if source_semantics_sha is not None:
        require(hex64(source_semantics_sha) and isinstance(rebind.get('capture_methods'), dict) and rebind['capture_methods'], 'maintenance_source_semantics')
    source, generation, digests = bound_schema(catalog, rebind, identity)
    require(isinstance(maintenance_envelope, dict) and maintenance_envelope.get('status') == 200, 'maintenance_not_invoked')
    maintained = maintenance_envelope.get('response')
    maintained_native = maintained.get('native_maintenance') if isinstance(maintained, dict) else None
    require(isinstance(maintained, dict) and maintained.get('operation') == 'postMaintenance' and maintained.get('trigger') == 'source_generation_advanced' and isinstance(maintained_native, dict), 'maintenance_not_invoked')
    require(maintained.get('build_id') == build == maintained_native.get('build_id'), 'maintenance_not_invoked')
    require(maintained_native.get('protocol') == native.get('protocol') and maintained_native.get('publication') == 'not_published', 'maintenance_not_invoked')
    require(type(maintained.get('llm_calls')) is int and maintained['llm_calls'] == 0 and type(maintained_native.get('llm_calls')) is int and maintained_native['llm_calls'] == 0, 'maintenance_not_invoked')
    require(maintained_native.get('pack_digest') == digest_value and maintained_native.get('artifact_revision') == revision, 'role_pack_not_current')
    maintained_rebind = maintained_native.get('source_rebind')
    require(isinstance(maintained_rebind, dict) and maintained_rebind.get('catalog_sha256') == rebind.get('catalog_sha256'), 'maintenance_source')
    maintained_request = maintenance_envelope.get('request')
    require(isinstance(maintained_request, dict) and maintained_request.get('trigger') == 'source_generation_advanced' and maintained_request.get('catalog_sha256') == rebind['catalog_sha256'], 'maintenance_not_invoked')
    if source_semantics_sha is not None:
        require(maintained_rebind.get('source_semantics_sha256') == source_semantics_sha and maintained_request.get('source_semantics_sha256') == source_semantics_sha and maintained_rebind.get('capture_methods') == rebind.get('capture_methods'), 'maintenance_source_semantics')
    require(isinstance(overlay, dict) and overlay.get('contract') == 'oo.selfbuild-control-api/overlay/v1' and overlay.get('operation') == 'getOverlay' and overlay.get('build_id') == build, 'maintenance_not_invoked')
    require(type(overlay.get('llm_calls')) is int and overlay['llm_calls'] == 0, 'maintenance_not_invoked')
    schema={'schema_digests':digests} if digests is not None else {'schema_digest':catalog['schema_digest']}
    result={'protocol':CONSUMPTION,'product_receipt_sha256':receipt_sha,'product_producer':product['producer'],'product_producer_sha256':product['producer_sha256'],'build_id':build,'s5_role_pack_sha256':s5_pack_sha,'role_pack_file_sha256':file_sha256,'pack_digest':digest_value,'http_pack_sha256':http_pack,'source_id':source,'source_generation':generation,**schema,'catalog_sha256':rebind['catalog_sha256'],'catalog_version':identity,'role':role_name,'policy_version':'oos/v1','artifact_revision':revision,'maintenance_llm_calls':0,'publication':'not_published','role_exit':0,'maintenance_exit':0,'role_response_sha256':hashlib.sha256(role_response_bytes).hexdigest(),'maintenance_response_sha256':hashlib.sha256(maintenance_response_bytes).hexdigest(),'prepared_overlay_sha256':hashlib.sha256(overlay_bytes).hexdigest()}
    if source_semantics_sha is not None:
        result['source_semantics_sha256']=source_semantics_sha
        result['capture_methods']=rebind['capture_methods']
    return result

def prove_whole(out, recipe, journal):
    """Whole means this run's pinned product_flow and dialogue outputs, not a boolean."""
    require(recipe.get('mode') == 'whole_source' and recipe.get('completion_artifact'), 'whole_completion_required')
    complete = load(rel_ok(out, recipe['completion_artifact']))
    require(complete.get('full_product_chain_complete') is True, 'whole_chain_incomplete')
    require(complete.get('periodic_insert_update_delete_verified') is True and complete.get('recovery_verified') is True, 'capture_lifecycle_incomplete')
    product_rel, dialogue_rel = complete.get('product_receipt'), complete.get('dialogue_verdict')
    require(isinstance(product_rel, str) and isinstance(dialogue_rel, str), 'completion_path')
    direct_step(out, recipe, journal, PRODUCT_FLOW, product_rel)
    parent = Path(dialogue_rel).parent
    transcript_rel = 'transcript.json' if parent == Path('.') else str(parent/'transcript.json')
    direct_step(out, recipe, journal, DIALOGUE, dialogue_rel)
    direct_step(out, recipe, journal, DIALOGUE, transcript_rel)
    product = load(rel_ok(out, product_rel))
    require(product.get('protocol') == 'oo.source-onboarding.product/v1' and product.get('fixture_review') is True, 'actual_product_receipt')
    # The pinned invoker is direct_step. A reused receipt keeps the producer hash of the writer that saved it.
    require(product.get('source_fresh') is True and product.get('producer') == 'tools/source-onboarding/product_flow.py' and re.fullmatch(r'[a-f0-9]{64}', product.get('producer_sha256') or '') is not None, 'source_not_fresh')
    require(isinstance(product.get('stages'), list) and {x['stage'] for x in product['stages'] if x.get('status') == 'succeeded'} >= {f'S{i}' for i in range(11)}, 'incomplete_product_stages')
    base = rel_ok(out, product_rel).parent
    s5 = product.get('s5')
    require(isinstance(s5, dict), 'not_real_s5')
    require(file_sha(base/'catalog.json', 'source_not_fresh') == product.get('catalog_sha256') == s5.get('evidence_hash'), 'source_not_fresh')
    require(s5.get('outcome') == 'validated' and s5.get('builder_spawned') is True and s5.get('structural_draft') is False, 'not_real_s5')
    require(s5.get('model') in S5_MODELS and re.fullmatch(r'[a-f0-9]{64}', s5.get('input_sha256', '')) is not None, 'not_real_s5')
    require(type(s5.get('llm_calls')) is int and s5['llm_calls'] >= 0 and isinstance(s5.get('cache_hit'), bool), 'not_real_s5')
    require(file_sha(base/'effective.json', 'standard_oos_mismatch') == product.get('effective_oos_sha256'), 'standard_oos_mismatch')
    require(load(base/'effective.json').get('protocol') == 'oo.builder.effective-oos/v1', 'standard_oos_mismatch')
    require(isinstance(product.get('standard_oos'), dict) and product['standard_oos'], 'standard_oos_mismatch')
    for name, sha in product['standard_oos'].items():
        require(name in {'ontology.yaml','effective-oos.yaml','source.json'} and file_sha(base/'oos'/name, 'standard_oos_mismatch') == sha, 'standard_oos_mismatch')
    source = load(base/'oos'/'source.json')
    require(source.get('actual_core_effective_sha256') == product['effective_oos_sha256'] and source.get('publication') == 'not_claimed', 'standard_oos_mismatch')
    dialogue = load(rel_ok(out, dialogue_rel))
    require(dialogue.get('outcome') == 'verified' and type(dialogue.get('turns')) is int and dialogue['turns'] >= 2 and dialogue.get('query_ids'), 'actual_dialogue_missing')
    require(dialogue.get('model') in DIALOGUE_MODELS and dialogue.get('reasoning_effort') == 'low', 'dialogue_not_accurate')
    require(isinstance(dialogue.get('tools'), list) and {'query','freshness','describe','knowledge_cat'} <= set(dialogue['tools']), 'dialogue_not_accurate')
    require(all(re.fullmatch(r'[a-f0-9]{64}', dialogue.get(k, '')) for k in ('audit_sha256','truth_sha256')), 'dialogue_not_accurate')
    blob = json.dumps(load(rel_ok(out, transcript_rel)))
    require(all(isinstance(q, str) and q in blob for q in dialogue['query_ids']), 'dialogue_not_accurate')
    # Legacy verified and replay do not count. The manifest is the dialogue step's selection.json.
    require(dialogue.get('selection') == 'random' and dialogue.get('counts_as_random') is True, 'random_not_current')
    sel_rel = 'selection.json' if parent == Path('.') else str(parent/'selection.json')
    direct_step(out, recipe, journal, DIALOGUE, sel_rel)
    selection = load(rel_ok(out, sel_rel))
    require(selection.get('mode') == 'random' and selection.get('counts_as_random') is True, 'random_not_current')
    ids = selection.get('semantic_ids')
    require(isinstance(ids, list) and len(ids) >= 2 and len(set(ids)) == len(ids) and all(isinstance(x, str) and x for x in ids), 'random_not_current')
    require(selection.get('seed') == dialogue.get('seed') and ids == dialogue.get('semantic_ids') and selection.get('gold_sha256') == dialogue.get('gold_sha256'), 'random_manifest_drift')
    identity = selection.get('identity')
    require(isinstance(identity, dict) and type(identity.get('source_generation')) is int and identity['source_generation'] >= 0, 'role_pack_not_current')
    pack = identity.get('pack_sha256')
    require(isinstance(pack, str) and re.fullmatch(r'[a-f0-9]{64}', pack) is not None, 'role_pack_not_current')
    # S5 provenance stays on the original role-pack file. Current random binds the maintenance file, not that hash.
    require(product.get('native_role_pack_sha256') == file_sha(base/'role-pack.json', 'role_pack_not_current'), 'role_pack_not_current')
    parent_rel = Path(product_rel).parent
    cons_rel = 'maintenance-consumption.json' if parent_rel == Path('.') else str(parent_rel/'maintenance-consumption.json')
    direct_step(out, recipe, journal, PRODUCT_FLOW, cons_rel)
    current_pack = base/'current-role-pack.json'
    require(current_pack.is_file() and not current_pack.is_symlink(), 'role_pack_not_current')
    for name in ('current-command.json','current-visibility.json','current-role-response.json','current-maintenance-response.json','current-role-exit.json','current-maintenance-exit.json','current-prepared-overlay.json'):
        require((base/name).is_file() and not (base/name).is_symlink(), 'maintenance_not_invoked')
    role_exit = load(base/'current-role-exit.json')
    maintenance_exit = load(base/'current-maintenance-exit.json')
    require(isinstance(role_exit, dict) and isinstance(maintenance_exit, dict), 'maintenance_not_invoked')
    fresh = maintenance_consumption(product, digest(base/'product-receipt.json'), product['native_role_pack_sha256'], current_pack.read_bytes(), load(base/'current-command.json'), load(base/'current-visibility.json'), (base/'current-role-response.json').read_bytes(), (base/'current-maintenance-response.json').read_bytes(), role_exit.get('exit'), maintenance_exit.get('exit'), (base/'current-prepared-overlay.json').read_bytes())
    stored = load(rel_ok(out, cons_rel))
    require(stored.get('schema_digest') == fresh.get('schema_digest') and stored.get('schema_digests') == fresh.get('schema_digests'), 'maintenance_schema')
    require(stored.get('source_id') == fresh['source_id'] and stored.get('source_generation') == fresh['source_generation'], 'maintenance_source')
    require(all(stored.get(k) == fresh[k] for k in ('role_pack_file_sha256','pack_digest','http_pack_sha256','role','policy_version')), 'role_pack_not_current')
    require(stored == fresh, 'maintenance_not_invoked')
    require(identity.get('source_id') == fresh['source_id'] and identity.get('source_generation') == fresh['source_generation'], 'maintenance_source')
    require(identity.get('role') == fresh['role'] and pack == fresh['role_pack_file_sha256'], 'role_pack_not_current')
    return complete

def install_engine(engine, recipe_path, secret_registry, metadata_recipe=None, whole_run=None):
    require(engine in ENGINES, 'engine')
    recipe = private(recipe_path); r = load(recipe); registry = private(secret_registry)
    require(r['engine'] == engine, 'engine_mismatch')
    for pin in r['pins']: require(digest(pin['path']) == pin['sha256'], 'pin_drift')
    mode = r.get('mode', 'whole_source')
    if mode == 'whole_source':
        require(r.get('completion_artifact'), 'whole_completion_required')
        for program in (PRODUCT_FLOW, DIALOGUE):
            require(any(p['path'] == str(program) and p['sha256'] == digest(program) for p in r['pins']), 'shared_program_unpinned')
        record = {'recipe':str(recipe),'recipe_sha256':digest(recipe),'secret_registry':str(registry),'whole':False,'awaiting_run':True}
        if whole_run:
            run_dir = Path(whole_run)
            st = run_dir.stat()
            require(run_dir.is_dir() and not run_dir.is_symlink() and st.st_uid == os.getuid() and not st.st_mode & 0o077, 'whole_run_permissions')
            journal = load(private(run_dir/'journal.json'))
            require(journal.get('state') == 'recipe_completed', 'whole_run_not_complete')
            require(journal.get('identity', {}).get('recipe_sha256') == digest(recipe), 'whole_run_recipe_drift')
            prove_whole(run_dir, r, journal)
            record = {'recipe':str(recipe),'recipe_sha256':digest(recipe),'secret_registry':str(registry),'whole':True,'whole_proof_sha256':digest(run_dir/r['completion_artifact'])}
    elif mode == 'metadata_sample':
        require(whole_run is None and r.get('read_only') is True and type(r.get('max_rows')) is int and 1 <= r['max_rows'] <= 5, 'metadata_cannot_claim_whole')
        record = {'recipe':str(recipe),'recipe_sha256':digest(recipe),'secret_registry':str(registry),'whole':False}
    else:
        raise Refused('capture_cannot_claim_whole')
    base = INSTALL_ROOT; base.mkdir(mode=0o700, parents=True, exist_ok=True)
    with (base/'registry.lock').open('a') as lock:
        os.chmod(base/'registry.lock', 0o600); fcntl.flock(lock, fcntl.LOCK_EX)
        target = base/'engines.json'
        entries = load(private(target)) if target.exists() else {'protocol':'oo.source-onboarding.engines/v1','engines':{}}
        entries['engines'][engine] = record
        if metadata_recipe:
            metadata = private(metadata_recipe); meta = load(metadata)
            require(meta['engine'] == engine and meta.get('mode') == 'metadata_sample' and meta.get('read_only') is True and type(meta.get('max_rows')) is int and 1 <= meta['max_rows'] <= 5, 'metadata_scope')
            for pin in meta['pins']: require(digest(pin['path']) == pin['sha256'], 'pin_drift')
            entries['engines'][engine].update(metadata_recipe=str(metadata), metadata_recipe_sha256=digest(metadata))
        save(target, entries)
    return record

def run(input_path, resume=False):
    source = private(input_path); spec = load(source); reject_secrets(spec)
    if 'connection' not in spec:
        tables=spec.pop('approved_tables',None)
        spec={'protocol':'oo.source-onboarding/v1','connection':spec}
        if tables is not None:spec['approved_tables']=tables
    require(spec['protocol'] == 'oo.source-onboarding/v1', 'protocol')
    c = spec['connection']
    engine_registry_sha = None
    installed = None
    if 'recipe' not in spec:
        registry_path = private(INSTALL_ROOT/'engines.json')
        engine_registry = load(registry_path)
        require(engine_registry.get('protocol') == 'oo.source-onboarding.engines/v1', 'engine_registry')
        require(c.get('type') in engine_registry['engines'], 'engine_not_installed')
        installed = engine_registry['engines'][c['type']]
        spec['recipe'] = installed['recipe']
        require(digest(private(spec['recipe'])) == installed['recipe_sha256'], 'installed_recipe_drift')
        installed_recipe=load(spec['recipe'])
        if installed_recipe.get('owned_fixture') is True:
            template=load(private(installed_recipe['config_template']))
            matches=all(lookup(template,p)==c.get(field) for p,field in installed_recipe['connection_map'].items() if field in {'host','port','database','user'})
            if not matches:
                require('metadata_recipe' in installed,'owned_connection_scope')
                spec['recipe']=installed['metadata_recipe']
                require(digest(private(spec['recipe']))==installed['metadata_recipe_sha256'],'installed_recipe_drift')
        spec['secret_registry'] = installed['secret_registry']
        engine_registry_sha = digest(registry_path)
        runs = INSTALL_ROOT/'runs'
        runs.mkdir(mode=0o700,parents=True,exist_ok=True)
        if resume:
            selected = source.with_suffix(source.suffix+'.run.json')
            require(selected.exists(), 'run_pointer_missing')
            spec['out']=load(private(selected))['out']
        else: spec['out']=str(runs/uuid.uuid4().hex)
    require(set(c) == {'type','database','host','port','user','secret_ref'}, 'connection_shape')
    require(c['type'] in ENGINES, 'engine')
    require(type(c['port']) is int and 1 <= c['port'] <= 65535, 'port')
    for key in ('database','host','user'):
        require(isinstance(c[key], str) and 0 < len(c[key]) <= 253 and not any(x in c[key] for x in '\r\n\0'), 'connection_value')
    require(re.fullmatch(r'secret://[A-Za-z0-9_./-]{1,128}', c['secret_ref']) is not None, 'secret_ref')
    recipe_path = private(spec['recipe']); recipe = load(recipe_path); reject_secrets(recipe)
    require(recipe['protocol'] == 'oo.source-onboarding.recipe/v1' and recipe['engine'] == c['type'], 'recipe_engine')
    mode = recipe.get('mode', 'whole_source')
    require(mode in {'whole_source', 'source_capture', 'metadata_sample'}, 'recipe_mode')
    if recipe.get('owned_fixture') is not True:
        require(mode == 'metadata_sample' and recipe.get('read_only') is True and type(recipe.get('max_rows')) is int and 1 <= recipe['max_rows'] <= 5, 'customer_full_import_not_authorized')
    registry = load(private(spec['secret_registry']))
    require(c['secret_ref'] in registry, 'secret_unresolved')
    password_file = private(registry[c['secret_ref']])
    pins = recipe['pins']
    require(pins and recipe['steps'], 'empty_recipe')
    for pin in pins:
        require(digest(pin['path']) == pin['sha256'], 'pin_drift')
    out = Path(spec['out']); require(out.is_absolute(), 'output_path')
    template_path = private(recipe['config_template'])
    identity = {'input_sha256':digest(source), 'recipe_sha256':digest(recipe_path), 'template_sha256':digest(template_path), 'engine_registry_sha256':engine_registry_sha, 'pins':pins}
    if installed is not None: identity['selected_engine_sha256'] = selected_binding(installed, recipe_path, password_file)
    if resume:
        require(out.is_dir() and not out.is_symlink(), 'resume_directory')
    else:
        out.mkdir(mode=0o700, parents=False, exist_ok=False)
        if engine_registry_sha is not None:save(source.with_suffix(source.suffix+'.run.json'),{'out':str(out),'input_sha256':digest(source)})
    require(out.stat().st_uid == os.getuid() and not out.stat().st_mode & 0o077, 'run_permissions')
    with (out/'lock').open('a') as lock:
        os.chmod(out/'lock', 0o600); fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        journal_path = out/'journal.json'
        if resume:
            journal = load(journal_path); require(resume_identity_ok(journal, identity, out/'driver-config.json', recipe, password_file), 'resume_drift')
            require(digest(out/'driver-config.json') == journal['config_sha256'], 'resume_config_drift')
        else:
            template_path = private(recipe['config_template']); config = load(template_path)
            for p, field in recipe['connection_map'].items():
                if recipe.get('owned_fixture') is True and field in {'host','port','database','user'}:
                    require(lookup(config,p) == c[field],'owned_connection_scope')
                require(field in c or field == 'password_file', 'connection_mapping')
                pointer(config, p, str(password_file) if field == 'password_file' else c[field])
            require(set(recipe['connection_map'].values()) >= {'host','port','database','user','password_file'}, 'connection_not_consumed')
            if 'approved_tables' in spec:
                selected=spec['approved_tables']
                require(isinstance(selected,list) and 1 <= len(selected) <= 50,'approved_tables')
                require('approved_tables_pointer' in recipe,'table_selection_not_supported')
                pointer(config,recipe['approved_tables_pointer'],selected)
            save(out/'driver-config.json', config)
            journal = {'protocol':'oo.source-onboarding.run/v1','identity':identity,'config_sha256':digest(out/'driver-config.json'),'state':'running','steps':[]}
            save(journal_path,journal)
        done = [x for x in journal['steps'] if x['state'] == 'succeeded']
        for record in done:
            for artifact in record['artifacts']:
                require(digest(out/artifact['path']) == artifact['sha256'], 'resume_artifact_drift')
        for index, step in enumerate(recipe['steps']):
            require(re.fullmatch(r'[a-z][a-z0-9_-]{0,63}', step['name']) is not None, 'step_name')
            if index < len(done):
                require(done[index]['name'] == step['name'], 'resume_step_drift'); continue
            attempt = len(journal['steps'])
            argv = [a.replace('{config}',str(out/'driver-config.json')).replace('{run}',str(out)) for a in step['argv']]
            require(argv and Path(argv[0]).is_absolute(), 'executable_path')
            require(any(p['path'] == argv[0] for p in pins), 'executable_not_pinned')
            record = {'name':step['name'],'state':'running','attempt':attempt,'artifacts':[]}
            journal['steps'].append(record); journal['state']='running'; save(journal_path,journal)
            with (out/f'{attempt}.stdout').open('xb') as stdout, (out/f'{attempt}.stderr').open('xb') as stderr:
                os.chmod(stdout.name,0o600); os.chmod(stderr.name,0o600)
                child=None
                try:
                    deadline = step.get('deadline_seconds',1800)
                    require(type(deadline) is int and 1 <= deadline <= 86400, 'deadline')
                    child = subprocess.Popen(argv, stdout=stdout, stderr=stderr, cwd=out, start_new_session=True)
                    record['pid'] = child.pid; record['observed_programs']=[]; save(journal_path,journal)
                    seen=[]; limit=time.monotonic()+deadline
                    try:
                        while True:
                            for path in observe_children(child.pid):
                                if path not in seen: seen.append(path); record['observed_programs']=seen; save(journal_path,journal)
                            remaining=limit-time.monotonic()
                            if remaining<=0:
                                os.killpg(child.pid, signal.SIGKILL); child.wait(); raise subprocess.TimeoutExpired(argv, deadline)
                            try:
                                code=child.wait(timeout=min(0.5, remaining)); break
                            except subprocess.TimeoutExpired:
                                continue
                    except subprocess.TimeoutExpired:
                        raise
                    record['exit_code']=code
                    require(code == 0, 'child_failed')
                    for item in step['artifacts']:
                        p = Path(item['path']); require(not p.is_absolute() and '..' not in p.parts, 'artifact_path')
                        artifact = out/p
                        require(artifact.is_file() and not artifact.is_symlink() and artifact.resolve().is_relative_to(out.resolve()), 'artifact_missing')
                        sha = digest(artifact)
                        require('sha256' not in item or item['sha256'] == sha, 'artifact_mismatch')
                        record['artifacts'].append({'path':str(p),'sha256':sha})
                    if launched_program(argv) == str(PRODUCT_FLOW) or str(PRODUCT_FLOW) in seen:
                        have = {a['path'] for a in record['artifacts']}
                        for path in list(have):
                            if not path.endswith('product-receipt.json'): continue
                            sib = str(Path(path).with_name('maintenance-consumption.json'))
                            artifact = out/sib
                            if sib not in have and artifact.is_file() and not artifact.is_symlink() and artifact.resolve().is_relative_to(out.resolve()):
                                record['artifacts'].append({'path':sib,'sha256':digest(artifact)})
                    require(record['artifacts'], 'no_fresh_artifacts')
                    record['state']='succeeded'
                except (Refused, subprocess.TimeoutExpired, OSError):
                    if child is not None and child.poll() is None:
                        os.killpg(child.pid,signal.SIGKILL);child.wait()
                    record['state']='failed'; journal['state']='failed'; save(journal_path,journal); raise Refused('stage_failed')
            save(journal_path,journal)
        whole=False
        try:
            if mode == 'whole_source':
                prove_whole(out, recipe, journal); whole=True
            elif recipe.get('completion_artifact'):
                claimed=load(rel_ok(out, recipe['completion_artifact']))
                require(claimed.get('full_product_chain_complete') is not True, 'capture_cannot_claim_whole')
        except Refused:
            journal['state']='failed';journal.setdefault('validation_failures',[]).append('completion_refused');save(journal_path,journal);raise
        journal['state']='recipe_completed'; save(journal_path,journal)
        return {'protocol':journal['protocol'],'state':journal['state'],'steps':len(recipe['steps']),'mode':mode,'whole':whole,'published':False}

def main():
    p=argparse.ArgumentParser();p.add_argument('--input',required=True);p.add_argument('--resume',action='store_true');a=p.parse_args()
    try: print(json.dumps(run(a.input,a.resume))); return 0
    except (Refused, OSError, KeyError, TypeError, json.JSONDecodeError):
        print('source-onboarding: refused',file=sys.stderr);return 2

if __name__ == '__main__': sys.exit(main())
