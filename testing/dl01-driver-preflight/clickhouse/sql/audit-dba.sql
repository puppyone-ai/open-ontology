SELECT query_kind, toString(count()) AS n
FROM system.query_log
WHERE user = 'oo_dl01w1_40_dba'
  AND is_initial_query = 1
  AND type IN ('QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing')
  AND event_date >= yesterday()
GROUP BY query_kind
ORDER BY query_kind
FORMAT JSONEachRow
