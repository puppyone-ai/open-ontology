/* oo-probe:mutations */
SELECT
    database,
    table,
    command,
    is_done,
    latest_fail_reason
FROM system.mutations
WHERE database = 'oo_dl01' AND table = 'retirees'
ORDER BY create_time
FORMAT JSONEachRow
