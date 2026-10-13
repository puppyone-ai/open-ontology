/* oo-probe:audit */
SELECT
    query_id,
    query_kind,
    type,
    exception_code,
    query
FROM system.query_log
WHERE user = 'oo_dl01w1_11_reader'
  AND type IN ('QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing')
  AND position(query, 'oo-probe:') > 0
ORDER BY event_time_microseconds, query_id, type
FORMAT JSONEachRow
