| amount |
| --- |
| 12.50 |
| null |
| true |
-- query_id: q_abc123
-- returned_rows: 3
-- duration_ms: 12
-- truncated: false
-- truncation_reasons:
-- freshness: table=logistics.receipt as_of=2026-10-01T01:12:03Z lag_ms=0 state=live
-- source: source=tms_pg consistency=transactional cross_table_consistent=true applied_pos=18446744073709551615
-- warning: code=STALE_DATA rows=2 message=lag
-- hints:
-- knowledge_version: oos/v1
-- policy_version: type-policy/v1
-- star_expanded: none
