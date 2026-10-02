SELECT json_build_object(
    'id', id,
    'bool_true_hex', encode(boolsend(flag), 'hex'),
    'int2_hex', encode(int2send(small), 'hex'),
    'int8_hex', encode(int8send(big), 'hex'),
    'float8_hex', encode(float8send(ratio), 'hex'),
    'float4_hex', encode(float4send(narrow), 'hex'),
    'hi_hex', encode(float8send(hi), 'hex'),
    'units_hex', encode(numeric_send(units), 'hex'),
    'fine_hex', encode(numeric_send(fine), 'hex'),
    'wide_hex', encode(numeric_send(wide), 'hex'),
    'huge_text', huge::text,
    'open_text', open_num::text,
    'fine_text', fine::text,
    'json_hex', encode(json_send(doc), 'hex'),
    'jsonb_hex', encode(jsonb_send(docb), 'hex'),
    'span_hex', encode(interval_send(span), 'hex'),
    'nums_hex', encode(array_send(nums), 'hex'),
    'grid_hex', encode(array_send(grid), 'hex'),
    'date_hex', encode(date_send(day), 'hex'),
    'uuid_hex', encode(uuid_send(ext), 'hex'),
    'bytea_hex', encode(byteasend(blob), 'hex'),
    'label', label,
    'bad_num_hex', CASE WHEN bad_num IS NULL THEN '' ELSE encode(numeric_send(bad_num), 'hex') END
)::text
FROM lab.sample
ORDER BY id;
