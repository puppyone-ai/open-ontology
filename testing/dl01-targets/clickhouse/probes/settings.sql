/* oo-probe:settings */
SELECT name, value
FROM system.settings
WHERE name IN ('readonly', 'max_execution_time')
ORDER BY name
FORMAT JSONEachRow
