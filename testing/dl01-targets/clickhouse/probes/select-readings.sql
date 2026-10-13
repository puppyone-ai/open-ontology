/* oo-probe:batch */
SELECT
    reading_id,
    version,
    toDecimalString(amount, 10) AS amount,
    note,
    code,
    code_or_null,
    concat(
        formatDateTime(ts, '%Y-%m-%d %H:%i:%S', 'UTC'),
        '.',
        leftPad(toString(toUnixTimestamp64Nano(ts) % 1000000000), 9, '0')
    ) AS ts,
    tags,
    labels,
    qty,
    label,
    toString(wide_u64) AS wide_u64
FROM oo_dl01.readings
