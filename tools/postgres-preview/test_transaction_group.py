import transaction_group


def _note(generation, parent):
    return {
        'logical_schema': 'customer_east',
        'logical_table': 'delivery_notes',
        'source_generation': generation,
        'parent_source_generation': parent,
    }


def _scope(generation, tables):
    return {'source_id': 'oo_pg46_finish', 'end_lsn': '0/40715A8', 'xid': 878, 'tables': [
        {'logical_schema': schema, 'logical_table': table, 'source_generation': generation}
        for schema, table in tables
    ]}


def test_one_table_group_keeps_the_other_tables_unmentioned():
    notes6 = _note(6, 5)
    notes5 = _note(5, 4)
    hubs4 = {'logical_schema': 'public', 'logical_table': 'hubs', 'source_generation': 4, 'parent_source_generation': 3}
    entries = [notes5, notes6, hubs4]
    scopes = [
        _scope(4, [('customer_east', 'delivery_notes'), ('public', 'hubs'), ('public', 'shipments'), ('public', 'tracking_events')]),
        _scope(5, [('customer_east', 'delivery_notes')]),
        _scope(6, [('customer_east', 'delivery_notes')]),
    ]
    vector = [
        {'logical_schema': 'customer_east', 'logical_table': 'delivery_notes', 'registry_path': '/seal/customer_east.delivery_notes/5/registry.json'},
        {'logical_schema': 'customer_east', 'logical_table': 'delivery_notes', 'registry_path': '/seal/customer_east.delivery_notes/6/registry.json'},
        {'logical_schema': 'public', 'logical_table': 'hubs', 'registry_path': '/seal/public.hubs/4/registry.json'},
    ]
    chosen, scope, vec = transaction_group.assemble(entries, scopes, vector, 6)
    assert [row['source_generation'] for row in chosen] == [6]
    assert [row['logical_table'] for row in scope['tables']] == ['delivery_notes']
    assert [item['registry_path'] for item in vec] == ['/seal/customer_east.delivery_notes/6/registry.json']
    four = [
        _note(4, 3),
        {'logical_schema': 'public', 'logical_table': 'hubs', 'source_generation': 4},
        {'logical_schema': 'public', 'logical_table': 'shipments', 'source_generation': 4},
        {'logical_schema': 'public', 'logical_table': 'tracking_events', 'source_generation': 4},
    ]
    full_vector = [
        {'logical_schema': row['logical_schema'], 'logical_table': row['logical_table'], 'registry_path': '/seal/%s.%s/4/registry.json' % (row['logical_schema'], row['logical_table'])}
        for row in four
    ]
    full_scope = _scope(4, [(row['logical_schema'], row['logical_table']) for row in reversed(four)])
    chosen, scope, vec = transaction_group.assemble(four, [full_scope], full_vector, 4)
    assert len(chosen) == len(scope['tables']) == len(vec) == 4
    assert [(row['logical_schema'], row['logical_table']) for row in chosen] == [
        (row['logical_schema'], row['logical_table']) for row in scope['tables']]


def test_group_refuses_a_registry_from_another_generation():
    notes6 = _note(6, 5)
    scope = _scope(6, [('customer_east', 'delivery_notes')])
    vector = [{'logical_schema': 'customer_east', 'logical_table': 'delivery_notes', 'registry_path': '/seal/customer_east.delivery_notes/5/registry.json'}]
    try:
        transaction_group.assemble([notes6], [scope], vector, 6)
    except RuntimeError as error:
        assert str(error) == 'admitted transaction group mismatch'
    else:
        raise AssertionError('wrong registry generation was accepted')
    extra = dict(notes6)
    hubs = {'logical_schema': 'public', 'logical_table': 'hubs', 'source_generation': 6}
    try:
        transaction_group.assemble(
            [extra, hubs],
            [scope],
            [{'logical_schema': 'customer_east', 'logical_table': 'delivery_notes', 'registry_path': '/seal/customer_east.delivery_notes/6/registry.json'}],
            6,
        )
    except RuntimeError as error:
        assert str(error) == 'admitted transaction group mismatch'
    else:
        raise AssertionError('table outside the transaction was accepted')


def test_same_commit_lsn_is_already_applied():
    scope = _scope(6, [('customer_east', 'delivery_notes')])
    acknowledged = _scope(5, [('customer_east', 'delivery_notes')])
    assert transaction_group.already_applied(scope, acknowledged)
    assert not transaction_group.already_applied(scope, _scope(5, [('customer_east', 'delivery_notes')]) | {'end_lsn': '0/4085240'})
    assert not transaction_group.already_applied(scope, None)
    try:
        transaction_group.already_applied(scope, _scope(5, [('public', 'hubs')]))
    except RuntimeError as error:
        assert str(error) == 'commit LSN conflicts with acknowledged transaction'
    else:
        raise AssertionError('same LSN with different transaction tables was accepted as replay')


if __name__ == '__main__':
    test_one_table_group_keeps_the_other_tables_unmentioned()
    test_group_refuses_a_registry_from_another_generation()
    test_same_commit_lsn_is_already_applied()
    print('ok')
