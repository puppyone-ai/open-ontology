SELECT user_name, access_type, database, `table`
FROM system.grants
WHERE user_name = 'oo_dl01w1_40_reader'
ORDER BY access_type, database, `table`
FORMAT JSONEachRow
