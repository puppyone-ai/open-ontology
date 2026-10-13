SELECT mutation_id, command, is_done, latest_fail_reason
FROM system.mutations
WHERE database = 'oo_dl01w40' AND `table` = 'retirees'
ORDER BY mutation_id
FORMAT JSONEachRow
