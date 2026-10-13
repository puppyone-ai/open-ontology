SELECT user, type, session_id
FROM system.session_log
WHERE user = 'oo_dl01w1_40_reader'
  AND event_date >= yesterday()
ORDER BY event_time
FORMAT JSONEachRow
