SELECT name, value
FROM system.settings
WHERE name IN (
    'readonly',
    'max_execution_time',
    'max_result_rows',
    'max_result_bytes',
    'result_overflow_mode',
    'function_sleep_max_microseconds_per_block',
    'cancel_http_readonly_queries_on_client_close',
    'output_format_arrow_compression_method',
    'output_format_arrow_string_as_string',
    'output_format_arrow_low_cardinality_as_dictionary',
    'log_queries',
    'log_comment'
)
ORDER BY name
FORMAT JSONEachRow
