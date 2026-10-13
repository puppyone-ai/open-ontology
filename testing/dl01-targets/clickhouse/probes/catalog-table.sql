/* oo-probe:catalog-table */
SELECT
    database,
    name,
    engine,
    engine_full,
    partition_key,
    sorting_key,
    primary_key
FROM system.tables
WHERE database = 'oo_dl01' AND name = 'readings'
FORMAT JSONEachRow
