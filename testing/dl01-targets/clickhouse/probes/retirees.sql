/* oo-probe:retirees */
SELECT id, payload
FROM oo_dl01.retirees
ORDER BY id
FORMAT JSONEachRow
