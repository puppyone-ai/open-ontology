| n | p | d | u | flag | missing |
| --- | --- | --- | --- | --- | --- |
| 9007199254740993 | 123456789012345678901234567890.123456789012345678901234567890 | 2026-10-01T01:12:03.123456789+08:00 | 浣熊市警察局 | true | null |
-- query_id: q_extrem1
-- returned_rows: 1
-- duration_ms: 9007199254740993
-- truncated: true
-- truncation_reasons: rows, bytes
-- freshness: table=logistics.receipt as_of=2026-10-01T01:12:03.123456+08:00 lag_ms=9007199254740993 state=stale
-- source: source=tms_pg consistency=transactional cross_table_consistent=true applied_pos=18446744073709551615
-- warning: code=STALE_DATA rows=9007199254740993 message=lag
-- warning: code=CROSS_TABLE_SKEW message=skew
-- hint: keep the predicate
-- knowledge_version: oos/v1
-- policy_version: type-policy/v1
-- star_expanded: logistics.receipt
