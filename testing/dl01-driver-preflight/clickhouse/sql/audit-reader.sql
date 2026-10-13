SELECT
    query_id,
    user,
    type,
    query_kind,
    exception_code,
    query_duration_ms,
    written_rows,
    written_bytes,
    log_comment
FROM system.query_log
WHERE user = 'oo_dl01w1_40_reader'
  AND is_initial_query = 1
  AND type IN ('QueryFinish', 'ExceptionBeforeStart', 'ExceptionWhileProcessing')
  AND event_date >= yesterday()
ORDER BY event_time_microseconds, query_id
FORMAT JSONEachRow
