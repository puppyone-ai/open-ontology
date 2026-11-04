"""Adapt sealed metadata to lane G's candidate-only three-pass Builder."""
import threading


def foreign_keys(table):
    result = []
    for fk in table.get('foreign_keys', []):
        columns = fk['columns']
        targets = {c['ref_schema'] + '.' + c['ref_table'] for c in columns}
        if len(targets) != 1:
            raise ValueError('COMMUNITY_FOREIGN_KEY_SHAPE')
        result.append(dict(referenced_dataset=targets.pop(),
                           columns=[c['name'] for c in columns],
                           referenced_columns=[c['ref_name'] for c in columns]))
    return result


def inventory(cards, catalog):
    if cards.get('protocol') != 'oo.pg-builder.cards/v1':
        raise ValueError('COMMUNITY_CARDS_PROTOCOL')
    physical = {t['schema'] + '.' + t['name']: t for t in catalog['tables']}
    tables = []
    for card in cards['tables']:
        table = physical[card['dataset']]
        if {c['name'] for c in card['columns']} != {c['name'] for c in table['columns']}:
            raise ValueError('COMMUNITY_COLUMN_COVERAGE')
        tables.append(dict(
            dataset=card['dataset'],
            primary_key=[c['name'] for c in table['primary_key']],
            comment=card.get('untrusted_table_comment'),
            columns=[dict(name=c['name'], data_type=c['type_name'],
                          nullable=not c['not_null'], comment=c.get('untrusted_comment'))
                     for c in card['columns']],
            foreign_keys=foreign_keys(table)))
    if len(tables) != len(physical) or len({t['dataset'] for t in tables}) != len(tables):
        raise ValueError('COMMUNITY_TABLE_COVERAGE')
    return tables


def review_input(tables, draft, project, source_hash):
    versions = dict(project=project, ontology_version='draft-v1', catalog_version='draft-v1',
                    policy=dict(id='owner', version='draft-v1'),
                    knowledge=dict(id='owner', version='draft-v1'))
    effective = dict(versions, protocol='oo.builder.effective-oos/v1',
        ontology=dict(spec_version='oos/v1', name=project, locale='en-US',
            include_schemas=sorted({t['dataset'].split('.')[0] for t in tables}),
            default_tier_preference=['file'], publish=dict(include_unreviewed=[]),
            classification=dict(auto_accept='none'),
            lifecycle=dict(deprecation_min_releases=2),
            compile=dict(per_role=True, budgets=dict(readme_tokens=2000, table_file_tokens=3000),
                         sample_values=dict(default='masked', max_per_column=0))),
        datasets=[dict(schema_version='oos/v1', dataset=t['dataset'], tier='file',
            label=t['dataset'], primary_key=t['primary_key'],
            columns=[dict(name=c['name'], pii='unknown', semantic_type='free_text')
                     for c in t['columns']]) for t in tables])
    # This is a local compilation proposal, never an authenticated role grant.
    visibility = dict(versions, protocol='oo.builder.role-visibility/v1', principal='owner',
        role='oo_r_owner', issued_for_compiler='oo-builder/role-pack/v1',
        datasets=[dict(dataset=t['dataset'], row_security='none', global_stats='denied',
                       columns=[c['name'] for c in t['columns']]) for t in tables],
        limits=dict(max_file_bytes=262144, max_total_bytes=1048576, max_entries=128))
    return dict(protocol='oo.builder.reviewed-community/v1', draft=draft,
                source_draft_sha256=source_hash, effective=effective, visibility=visibility,
                orphan_threshold=.2, ceilings={})


def generate(root, stack, binaries, read, save):
    import selfbuild_budget as builder
    tables = inventory(read(root/'cards.json'), read(root/'catalog.json'))
    config = dict(model=stack['model']['model'], endpoint='http://127.0.0.1:8317/v1',
                  transport='model-egress', stream=False, request_timeout_seconds=180)
    lock = threading.Lock()
    counter = root/'community-call-count.json'
    count = read(counter)['calls'] if counter.exists() else 0

    def transport(request):
        nonlocal count
        with lock:
            if count >= 6:
                raise ValueError('COMMUNITY_CALL_BUDGET')
            count += 1
            # Reserve before dispatch: interrupted requests still consume the limit.
            save(counter, dict(calls=count))
        return builder.model_call(config, request)

    runner = builder.Runner(config, root/'community', transport)
    graph = builder.bridge('graph', budget=runner.budget, tables=tables, candidates=[])
    delta = builder.bridge('incremental', tables=tables, graph=graph, previous={})
    preflight = runner.preflight(graph, delta['work'], {t['dataset']: t for t in tables})
    if preflight['batches'] > 4:
        raise ValueError('COMMUNITY_BATCH_BUDGET')
    draft = runner.run(tables, [])
    if draft['unresolved'] or draft.get('published') is not False:
        raise ValueError('COMMUNITY_INCOMPLETE')
    save(root/'draft.json', draft)
    # Rust serde_json serializes sorted object keys with UTF-8, as does this hash.
    command = review_input(tables, draft, stack['sources'][0]['id'], builder.digest(draft))
    save(root/'review-input.json', command)
