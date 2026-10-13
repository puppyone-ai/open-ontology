SELECT database, name, engine, engine_full, partition_key, sorting_key, primary_key, metadata_modification_time
FROM system.tables
WHERE database = 'oo_dl01w40' AND name = 'readings'
FORMAT JSONEachRow
