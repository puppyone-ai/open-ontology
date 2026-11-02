from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import sys
import time
from threading import Event

import library_selfbuild as lib
import library_metrics
import library_assets
import library_read_grants
from oos_tree import write_source_tree


def save(path, value):
    text = lib.canonical(value) + '\n'
    if path.exists():
        if path.read_text() != text:
            raise ValueError('artifact_drift')
    else:
        with path.open('x') as stream:
            stream.write(text)
        path.chmod(0o600)
    return hashlib.sha256(text.encode()).hexdigest()


def drafts(facts, config, output):
    if config.get('data_only_origin') is not None:
        return lib.data_only_drafts(facts, config), []
    lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'),
                               dictionary_config=config.get('dictionary_evidence_input'))
    batches = lib.configured_batches(facts, config)
    if len(batches) > config.get('max_draft_batches', 1):
        raise ValueError('explicit_draft_batch_budget')
    concurrency=config.get('s5_concurrency',1)
    if type(concurrency) is not int or not 1<=concurrency<=32:
        raise ValueError('s5_concurrency_budget')
    retained_hashes = set()
    if config.get('retained_drafts'):
        retained_hashes = {lib.digest(r) for r in lib.retained_document(config)['receipts']}
    provider_stopped = Event()
    def execute(batch):
        cache = lib.draft_cache_path(output, batch, config)
        if cache.exists():
            return lib.reuse_draft(json.loads(cache.read_text()), batch, config), None
        legacy = output / ('s5-' + lib.digest(config['model']) + '-' + lib.digest(batch) + '.json')
        if legacy.exists():
            receipt = json.loads(legacy.read_text())
            if (receipt['facts_sha256'] != facts['facts_sha256'] or receipt['model'] != config['model']
                    or not library_metrics.current_draft(receipt)):
                raise ValueError('draft_cache_binding')
            receipt_config = dict(config, _validation_feedback=receipt.get('validation_feedback'))
            if (receipt.get('request_sha256') != hashlib.sha256(lib.draft_request(batch, receipt_config)[1]).hexdigest()
                    and lib.digest(receipt) not in retained_hashes):
                raise ValueError('draft_cache_request_drift')
            return receipt,None
        failure = lib.draft_cache_path(output, batch, config, failure=True)
        prior = json.loads(failure.read_text()) if failure.exists() else {}
        attempts = prior.get('attempts', 0)
        if prior.get('retryable') is False:
            if prior.get('stops_run'):
                provider_stopped.set()
            return None, prior
        while attempts < 3:
            if provider_stopped.is_set():
                return None, {'batch': batch['index'], 'attempts': attempts,
                              'reason': 's5_provider_blocked_by_peer',
                              'model': config['model'], 'facts_sha256': facts['facts_sha256']}
            time.sleep(max(0, prior.get('retry_not_before', 0) - time.time()))
            if provider_stopped.is_set():
                continue
            attempts += 1
            try:
                receipt = lib.call_model(batch, dict(config, _validation_feedback=prior.get('validation_feedback')))
                save(cache, receipt)
                return receipt,None
            except (RuntimeError, ValueError, KeyError, TimeoutError, OSError) as error:
                reason=str(error)
                if not reason.startswith('s5_') or not all(c.islower()or c.isdigit()or c=='_'for c in reason):reason=type(error).__name__
                feedback = getattr(error, 'feedback', prior.get('validation_feedback'))
                prior = {'batch': batch['index'], 'attempts': attempts,
                         'reason': reason, 'retry_not_before': time.time() + 65,
                         'retryable': getattr(error, 'retryable', True),
                         'stops_run': getattr(error, 'stops_run', False),
                         'model':config['model'],'facts_sha256': facts['facts_sha256']}
                if feedback is not None:
                    prior['validation_feedback'] = feedback
                lib.write(failure, prior)
                failure.chmod(0o600)
                if prior['stops_run']:
                    provider_stopped.set()
                if not prior['retryable']:
                    return None, prior
                time.sleep(65)
        return None,prior
    with ThreadPoolExecutor(max_workers=concurrency)as pool:
        outcomes=list(pool.map(execute,sorted(batches,key=lambda b:b['index'])))
    return [receipt for receipt,failure in outcomes if receipt is not None],[failure for receipt,failure in outcomes if failure is not None]


def retained_drafts(facts, config, output):
    if config.get('data_only_origin') is not None:
        if config.get('retained_drafts') is not None:
            raise ValueError('data_only_retained_conflict')
        return
    retained = config.get('retained_drafts')
    if not retained:
        return
    lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'),
                               dictionary_config=config.get('dictionary_evidence_input'))
    document = lib.retained_document(config)
    batches = lib.configured_batches(facts, config)
    seen = set()
    audit = []
    for receipt in document['receipts']:
        if 'cache_identity' in receipt:
            names = {t['dataset'] for t in receipt['draft']['datasets']}
            matches = [b for b in batches if {t['dataset'] for t in b['tables']} == names]
            if (config.get('semantic_scope') is None or len(matches) != 1 or receipt['cache_identity'] !=
                    lib.semantic_binding(matches[0], config)['cache_identity']):
                audit.append({'receipt_sha256': lib.digest(receipt), 'state': 'invalidated',
                              'reason': 'semantic_input_or_context_changed'})
                continue
            batch = matches[0]
            current = lib.reuse_draft(receipt, batch, config)
            save(lib.draft_cache_path(output, batch, config), current)
            audit.append({'batch': batch['index'], 'receipt_sha256': lib.digest(receipt),
                          'state': 'reused', 'current_facts_sha256': facts['facts_sha256'],
                          'semantic_input_digest': batch['semantic_input_digest']})
            continue
        if receipt['facts_sha256'] != facts['facts_sha256'] or receipt['model'] != config['model']:
            raise ValueError('retained_draft_binding')
        if not library_metrics.current_draft(receipt):
            raise ValueError('retained_draft_contract')
        names = {t['dataset'] for t in receipt['draft']['datasets']}
        matches = [b for b in batches if {t['dataset'] for t in b['tables']} == names]
        if len(matches) != 1 or matches[0]['index'] in seen:
            raise ValueError('retained_batch_binding')
        batch = matches[0]
        current_request = hashlib.sha256(lib.draft_request(batch, config)[1]).hexdigest()
        if receipt.get('batch_sha256') not in (None, lib.digest(batch)):
            raise ValueError('retained_draft_batch_drift')
        if not isinstance(receipt.get('request_sha256'), str) or len(receipt['request_sha256']) != 64:
            raise ValueError('retained_draft_request_missing')
        seen.add(batch['index'])
        expected = {t['dataset']: {c['name'] for c in t['columns']} for t in batch['tables']}
        for table in receipt['draft']['datasets']:
            columns = [c['name'] for c in table['columns']]
            if len(columns) != len(set(columns)) or set(columns) != expected[table['dataset']]:
                raise ValueError('retained_column_binding')
        lib.merge_drafts({'tables': batch['tables']}, [receipt['draft']])
        save(output / ('s5-' + lib.digest(config['model']) + '-' + lib.digest(batch) + '.json'), receipt)
        audit.append({'batch': batch['index'], 'receipt_sha256': lib.digest(receipt),
                      'actual_request_sha256': receipt['request_sha256'],
                      'current_request_sha256': current_request,
                      'same_request': receipt['request_sha256'] == current_request,
                      'current_output_contract_validated': True})
    save(output / 'retained-drafts-audit.json', {
        'input_sha256': retained['sha256'], 'receipts': audit,
        'business_owner_signed': False})


def reviewed_documents(facts, receipts, review, knowledge_budgets=None, include_candidate_metrics=False):
    """Project a trusted review; the compile entrypoint separately verifies read grants."""
    effective, visibility, audit = lib.effective_documents(facts, [r['draft'] for r in receipts],
        include_candidate_metrics=include_candidate_metrics, metric_amendments=review.get('metric_amendments'))
    # Read access must include semantic clues, not only opaque column names.
    # The compiler still marks these descriptions UNREVIEWED and applies role visibility closure.
    effective['ontology']['publish']['include_unreviewed'].append('column_descriptions')
    if knowledge_budgets is not None:
        if set(knowledge_budgets) != {'readme_tokens', 'table_file_tokens'} or any(type(v) is not int or not 1 <= v <= 1_000_000 for v in knowledge_budgets.values()):
            raise ValueError('knowledge_budget')
        effective['ontology']['compile']['budgets'] = dict(knowledge_budgets)
    for document in [effective, visibility]:
        document['project'] = review['project']
    visibility['role'] = review['role']
    visibility['principal'] = review['principal']
    visible = []
    labels = review.get('labels', {})
    hidden = set(review['hidden_columns'])
    merged = lib.merge_drafts(facts, [r['draft'] for r in receipts])
    table_facts = {t['dataset']: t for t in merged['tables']}
    grain_projection = []
    pii_read_grants = []
    for table in effective['datasets']:
        name = table['dataset']
        table['label'] = labels.get(name, lib.declared_table_label(table_facts[name]))
        dataset_review = review.get('datasets', {}).get(name, {})
        if dataset_review.get('label') and table_facts[name].get('label_candidate'):
            table['label'] = table_facts[name]['label_candidate']
        if dataset_review.get('description') and table_facts[name].get('description_candidate'):
            table['description'] = table_facts[name]['description_candidate']
        grain = table_facts[name].get('grain_candidate')
        grain_approved = dataset_review.get('grain') is True
        if grain_approved:
            if not isinstance(grain, str) or not grain.strip() or not dataset_review.get('reviewer'):
                raise ValueError('grain_review_incomplete')
            table['grain'] = grain
        grain_projection.append({
            'dataset': name,
            'state': 'projected' if grain_approved else 'pending' if grain else 'absent',
            'candidate_sha256': lib.digest(grain) if grain else None,
        })
        selected = []
        for column in table['columns']:
            identity = name + '.' + column['name']
            decision = review['columns'].get(identity)
            if not decision or identity in hidden:
                continue
            source = next(c for c in table_facts[name]['columns'] if c['name'] == column['name'])
            classification = source['classification_candidate']
            source_pii, pii = classification['pii'], decision.get('pii')
            if source_pii == 'unknown':
                if (pii != 'sensitive' or not isinstance(decision.get('reason'), str)
                        or not decision['reason'].strip()):
                    raise ValueError('pii_conservative_review_required')
            elif pii != source_pii or pii not in {'none', 'personal', 'sensitive', 'restricted'}:
                raise ValueError('classification_conflict')
            reviewer = decision.get('reviewer')
            if not isinstance(reviewer, str) or not reviewer.strip():
                raise ValueError('classification_reviewer_required')
            explicit_allow = decision.get('explicit_allow', False)
            if type(explicit_allow) is not bool or explicit_allow != (pii != 'none'):
                raise ValueError('pii_read_grant_required')
            semantic = classification['semantic_type']
            if semantic == 'money':
                semantic = 'quantity'
            if decision['semantic_type'] != semantic:
                raise ValueError('classification_conflict')
            # A reviewed visibility entry is the artifact-bound read grant, not a PII downgrade.
            column['pii'] = pii
            column['semantic_type'] = semantic
            column['classification'] = {'state': 'confirmed', 'by': decision['reviewer']}
            if pii != 'none':
                grant = {'column': identity, 'source_pii': source_pii, 'pii': pii,
                         'reviewer': reviewer, 'explicit_allow': True}
                if source_pii == 'unknown':
                    grant['reason'] = decision['reason']
                pii_read_grants.append(grant)
            if decision.get('label_approved'):
                fields = decision.get('approved_fields', {})
                if fields.get('label') and source.get('label_candidate'):
                    column['label'] = source['label_candidate']
                if identity in labels:
                    column['label'] = labels[identity]
                if fields.get('description') and source.get('description_candidate'):
                    column['description'] = (source.get('comment', '') + '\nDraft supplement: ' + source['description_candidate']).strip()[:4000]
                column['review'] = {'state': 'approved', 'by': decision['reviewer']}
            selected.append(column['name'])
        if selected:
            visible.append({'dataset': name, 'row_security': 'none', 'global_stats': 'denied', 'columns': selected})
    visibility['datasets'] = visible
    approved_relations = set(review.get('relationships', []))
    if approved_relations and not review.get('reviewer'):
        raise ValueError('relationship_reviewer_required')
    for relation in effective['relationships']:
        if relation['name'] in approved_relations:
            relation['review'] = {'state': 'approved', 'by': review['reviewer']}
    for term in effective['glossary']:
        dataset = term['maps_to']['dataset']
        decision = review.get('datasets', {}).get(dataset, {})
        candidate_sha = lib.digest({'term':term['term'], 'definition':term['definition'], 'dataset':dataset})
        if candidate_sha in decision.get('glossary_terms', []) and decision.get('reviewer'):
            term['status'] = 'approved'
            term['owner'] = decision['reviewer']
    audit['ledger_bound'] = review.get('review_origin') != 'operator_pinned_file'
    audit['business_owner_signed'] = False
    audit['review_document_sha256'] = lib.digest(review)
    audit['grain_projection'] = grain_projection
    audit['pii_read_grants'] = pii_read_grants
    amended, _ = library_metrics.amend_candidates(facts['tables'], merged['metric_candidates'],
                                                 review.get('metric_amendments', {}))
    audit['metric_projection'] = library_metrics.project_metrics(amended, effective, review.get('metrics', {}))
    audit['metric_ledger_bound'] = False
    audit['semantic_projection'] = library_assets.project(
        facts, [r['draft'] for r in receipts], effective, review.get('semantic_assets', {}))
    audit['semantic_ledger_bound'] = False
    return effective, visibility, audit


def data_only_documents(facts, receipts, config, effective, visibility):
    origin = lib.data_only_origin(config)
    if receipts != lib.data_only_drafts(facts, config):
        raise ValueError('data_only_use_drift')
    lib.verify_pack_artifact(origin['pack']['artifact'])
    if (lib.digest(origin['effective']) != origin['pack']['effective_sha256']
            or lib.digest(origin['visibility']) != origin['pack']['visibility_sha256']):
        raise ValueError('data_only_origin_pack_binding')

    def meaning(document):
        result = json.loads(lib.canonical(document))
        # Keep historical frequencies in the old artifact; new counts stay in facts.
        for enum in result['enums']:
            for value in enum['values']:
                value.get('evidence', {}).pop('rows', None)
        return result

    if meaning(effective) != meaning(origin['effective']) or visibility != origin['visibility']:
        raise ValueError('data_only_effective_drift')
    return origin['effective'], origin['visibility']


def select_tables(tables, manifest):
    selected = lib.select_library(tables, manifest)
    owners = {}
    for source in manifest['sources']:
        for name in source['tables']:
            dataset = source['schema'] + '.' + name
            if dataset in owners:
                raise ValueError('library_source_binding')
            owners[dataset] = source['source_id']
    if set(owners) != {t['dataset'] for t in selected} or any(owners[t['dataset']] != t['source_id'] for t in selected):
        raise ValueError('library_source_binding')
    return selected


def run(request):
    stage = request['stage']
    output = Path(request['work_dir'])
    output.mkdir(parents=True, exist_ok=True)
    config = json.loads(Path(request['config']).read_text())
    manifest = json.loads(Path(request['manifest']).read_text())
    db = lib.ReadOnlyPostgres(config['psql_command'], config.get('statement_timeout_ms', 30000))
    if stage == 'S0':
        if config.get('s0_input'):
            snapshot = Path(config['s0_input'])
            if hashlib.sha256(snapshot.read_bytes()).hexdigest() != config['s0_sha256']:
                raise ValueError('retained_s0_digest')
            tables = json.loads(snapshot.read_text())
            tables = select_tables(tables, manifest)
            lib.validate_retained_tables(db, tables, config.get('ddl_directory'),
                                         oracle_input=config.get('oracle_observation_input'))
        else:
            tables = lib.collect_tables(db, config.get('ddl_directory'), lib.inventory_names(manifest),
                                        output / ('observation-' + str(time.time_ns()) + '.json'),
                                        config.get('observation_checkpoint_dir'),
                                        oracle_input=config.get('oracle_observation_input'))
        tables = select_tables(tables, manifest)
        lib.require_complete_columns(tables)
        actual = sorted(t['dataset'] for t in tables)
        if actual != sorted(request['datasets']) or len(tables) > request['max_tables']:
            raise ValueError('library_project_inventory')
        artifact = 's0-s2.json'
        sha = save(output / artifact, tables)
    elif stage in ['S1', 'S2', 'S3', 'S4']:
        tables = json.loads((output / 's0-s2.json').read_text())
        lib.require_complete_columns(tables)
        if config.get('oracle_observation_input') is not None and (
                stage != 'S1' or config.get('facts_input')):
            lib.validate_retained_tables(
                db, tables, oracle_input=config['oracle_observation_input'])
        artifact = 'facts.json'
        if stage == 'S1' and config.get('facts_input'):
            path=Path(config['facts_input'])
            if hashlib.sha256(path.read_bytes()).hexdigest()!=config['facts_sha256']:raise ValueError('retained_facts_digest')
            facts=json.loads(path.read_text())
            lib.require_complete_facts(facts, tables, config.get('declared_enum_adapter'), config.get('report_definition_adapter'))
            vector=lambda items:{t['dataset']:(t['source_id'],t['source_generation'],t['target_generation'])for t in items}
            if vector(facts['tables'])!=vector(tables):raise ValueError('retained_facts_vector')
            sha=save(output / artifact,facts)
        elif stage == 'S1':
            facts = lib.observe_facts(db, tables, config)
            sha = save(output / artifact, facts)
            lib.require_complete_facts(facts, tables, config.get('declared_enum_adapter'), config.get('report_definition_adapter'))
        else:
            lib.require_complete_facts(json.loads((output / artifact).read_text()), tables, config.get('declared_enum_adapter'), config.get('report_definition_adapter'))
            sha = hashlib.sha256((output / artifact).read_bytes()).hexdigest()
    elif stage == 'S5':
        facts = json.loads((output / 'facts.json').read_text())
        lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'))
        config.update(request['llm'])
        retained_drafts(facts, config, output)
        receipts, failures = drafts(facts, config, output)
        artifact = 'drafts.json'
        if failures:
            save(output / ('drafts-failed-' + str(time.time_ns()) + '.json'),
                 {'receipts': receipts, 'failures': failures})
            raise ValueError('semantic_draft_batches_failed')
        library_assets.require_draft_coverage(facts, receipts)
        sha = save(output / artifact, {'receipts': receipts, 'failures': []})
    elif stage in ['S6', 'S7', 'S8']:
        facts = json.loads((output / 'facts.json').read_text())
        lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'))
        receipts = json.loads((output / 'drafts.json').read_text())['receipts']
        library_assets.require_draft_coverage(facts, receipts)
        merged = lib.merge_drafts(facts, [r['draft'] for r in receipts])
        artifact = 'merged.json'
        sha = save(output / artifact, merged)
        save(output / 'suggestions.json', {
            'protocol': 'oo.library.metric-candidates/v1', 'ready': True,
            'merged': True, 'certified': False, 'facts_sha256': facts['facts_sha256'],
            'drafts_sha256': merged['drafts_sha256'],
            'metric_candidates': merged['metric_candidates'],
            'example_candidates': merged['example_candidates'],
            'enum_candidates': facts['enum_candidates'],
            'glossary_candidates': [t for r in receipts for t in r['draft']['glossary']],
            'report_semantics': merged['report_semantics'],
        })
        if stage == 'S8':
            generated, _, audit = lib.effective_documents(facts, [r['draft'] for r in receipts])
            binding = write_source_tree(output / 'generated-ontology', generated, generated)
            save(output / 'generated-oos.json', generated)
            save(output / 'generated-oos-binding.json', binding)
            save(output / 'generation-review.json', audit)
    elif stage == 'compile':
        facts = json.loads((output / 'facts.json').read_text())
        lib.require_complete_facts(facts, declared_enum_config=config.get('declared_enum_adapter'),
                               report_config=config.get('report_definition_adapter'))
        receipts = json.loads((output / 'drafts.json').read_text())['receipts']
        library_assets.require_draft_coverage(facts, receipts)
        review = json.loads((output / 'ledger-review.json').read_text())
        merged_path = output / 'merged.json'
        # HTTP ledger membership binds S6's merged document. Offline callers may
        # compile directly from the already checked facts/drafts without S6.
        if review.get('review_origin') == 'review_ledger' or merged_path.exists():
            if (merged_path.is_symlink() or not merged_path.is_file()
                    or lib.merge_drafts(facts, [r['draft'] for r in receipts]) != json.loads(merged_path.read_text())):
                raise ValueError('review_generation_drift')
        review, authorization = library_read_grants.authorize(config, facts, review)
        if authorization is not None:
            review['read_authorization'] = authorization
        if review.get('metrics'):
            raise ValueError('metric_review_origin')
        if review.get('metric_amendments'):
            raise ValueError('metric_amendment_origin')
        review['metric_amendments'] = library_metrics.load_amendments(config, facts, review['project'])
        review['metrics'] = library_metrics.load_reviews(config, facts, review['project'])
        if review.get('semantic_assets'):
            raise ValueError('semantic_review_origin')
        review['semantic_assets'] = library_assets.load_reviews(config, facts, review['project'])
        effective, visibility, audit = reviewed_documents(facts, receipts, review, config.get('knowledge_budgets'),
                                                        config.get('include_candidate_metrics', False))
        audit['read_authorization'] = authorization
        library_assets.bind_release_versions(effective, visibility, config.get('release_versions'))
        if config.get('data_only_origin') is not None:
            effective, visibility = data_only_documents(facts, receipts, config, effective, visibility)
            audit['data_only_origin'] = config['data_only_origin']
            audit['observation_facts_sha256'] = facts['facts_sha256']
        if 'access_profile' in review:
            from library_access_profile import coverage_report
            save(output / 'access-profile-report.json',
                 coverage_report(facts, review, visibility, authorization))
        audit['metric_review_input_sha256'] = (config.get('metric_review_input') or {}).get('sha256')
        audit['metric_amendment_input_sha256'] = (config.get('metric_amendment_input') or {}).get('sha256')
        audit['semantic_review_input_sha256'] = (config.get('semantic_review_input') or {}).get('sha256')
        save(output / 'effective-review-input.json', review)
        generated, _, _ = lib.effective_documents(facts, [r['draft'] for r in receipts])
        tree_binding = write_source_tree(output / 'ontology', generated, effective)
        save(output / 'oos-tree-binding.json', tree_binding)
        save(output / 'effective-oos.json', effective)
        save(output / 'visibility.json', visibility)
        save(output / 'review.json', audit)
        save(output / 'required-metrics.json', library_metrics.requirements(config))
        save(output / 'required-assets.json', library_assets.requirements(config))
        artifact = 'compiler-request.json'
        sha = save(output / artifact, {'protocol': 'oo.builder.role-pack-command/v1', 'effective_oos': effective, 'visibility': visibility})
    else:
        raise ValueError('library_stage')
    result = {'protocol': 'oo.selfbuild.library-stage/v1', 'stage': stage, 'artifact': artifact,
              'artifact_sha256': sha, 'business_owner_signed': False, 'published': False}
    save(output / (stage + '-receipt.json'), result)
    return result


if __name__ == '__main__':
    if len(sys.argv) > 1:
        from library_access_profile import main
        main()
        sys.exit(0)
    try:
        print(lib.canonical(run(json.load(sys.stdin))))
    except Exception as error:
        failure = {'error': type(error).__name__, 'published': False}
        # Fixed codes only: never echo paths, credentials or arbitrary child errors.
        if str(error) in {
            'pii_read_grant_required', 'read_grant_scope', 'read_grant_hidden',
            'read_grant_classification', 'read_grant_reviewer', 'read_grant_expired',
            'column_review_required', 'column_review_file', 'column_review_digest',
            'column_review_binding', 'review_generation_drift',
            'classification_conflict', 'pii_conservative_review_required',
            'access_profile_invalid', 'access_profile_inventory', 'access_profile_coverage',
            'access_profile_system_isolation', 'access_profile_operator_input_required',
        }:
            failure['reason'] = str(error)
        print(lib.canonical(failure))
        sys.exit(2)
