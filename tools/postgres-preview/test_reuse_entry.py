import whole


def test_reuse_keeps_the_pack_split_and_the_generation_vector():
    facts = {
        'generations': dict(whole.GENERATIONS),
        'slot_flush': '0/40715A8',
        'product_pack_sha256': whole.PRODUCT_PACK_SHA,
        'served_pack_sha256': whole.SERVED_PACK_SHA,
        'served_pack_digest': whole.SERVED_PACK_DIGEST,
        'product_role': 'oo_r_pg46_full',
        'served_role': 'oo_r_pg46_continuous_fixture',
        'table_bytes_same': True,
        'gold': dict(whole.GOLD_ROWS),
        'dialogue_gold_sha256': whole.DIALOGUE_GOLD_SHA,
        'full_target_columns': 38,
        'full_target_tables': 0,
        'full_dst_columns': 38,
        'continuous_target_columns': 38,
        'continuous_target_tables': 0,
        'full_member': 'oo_pg46_preview_login',
        'continuous_member': 'oo_pg46_continuous_login',
        'numeric_scale': {'customer_east.delivery_notes.amount': 4, 'public.shipments.cost_amount': 4},
        'receipt': {
            'build_id': 'b_ca53577181af9bb17a34a862644e900f',
            'native_role_pack_sha256': whole.PRODUCT_PACK_SHA,
            's5': {'outcome': 'validated', 'cache_hit': False, 'llm_calls': 1, 'model': 'grok-4.7-build'},
            'stages': [{'stage': f'S{i}', 'status': 'succeeded'} for i in range(11)],
        },
        'receipt_sha256': 'receipt',
        'public_gold_sha256': 'gold',
        'cancel_query_id': 'q_zvNH7ogR40uFLCERJ7g2UkYQsV',
        'dialogue_runs': [
            {'outcome': 'verified', 'seed': 1, 'pack_sha256': whole.SERVED_PACK_SHA, 'role': 'oo_r_pg46_continuous_fixture', 'source_generation': 5},
            {'outcome': 'verified', 'seed': 2, 'pack_sha256': whole.SERVED_PACK_SHA, 'role': 'oo_r_pg46_continuous_fixture', 'source_generation': 5},
        ],
        'slot_active': False,
        'slot_restart': '0/406DDA8',
        'current_wal': '0/40716E8',
        'slot_status': 'reserved',
    }
    body = whole.reuse_decision(facts)
    assert body['whole'] is False and body['reuse'] is True and body['no_overwrite'] is True
    assert body['full_product_chain_complete'] is False and body['copied_private_verdict'] is False
    assert body['source_context']['generations']['public.hubs'] == 4
    assert body['source_context']['generations']['customer_east.delivery_notes'] == 5
    assert body['source_context']['empty_database_fill_executed'] is True
    assert body['source_context']['approved_role_privileges']['pg46_finish_target_v2']['column_select'] == 38
    facts['served_role'] = 'oo_r_pg46_full'
    try:
        whole.reuse_decision(facts)
    except RuntimeError as error:
        assert str(error) == 'pack_collapsed'
    else:
        raise AssertionError('collapsed role was accepted')


if __name__ == '__main__':
    test_reuse_keeps_the_pack_split_and_the_generation_vector()
    print('ok')
