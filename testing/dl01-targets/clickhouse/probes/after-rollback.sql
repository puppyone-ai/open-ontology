/* oo-probe:after-rollback */
SELECT count() AS remaining
FROM oo_dl01.readings
FORMAT JSONEachRow
