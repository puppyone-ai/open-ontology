SELECT
    reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64
FROM oo_dl01w40.readings FINAL
ORDER BY reading_id, version
FORMAT ArrowStream
