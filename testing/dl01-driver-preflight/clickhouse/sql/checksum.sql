SELECT
    count() AS physical_rows,
    toString(sum(cityHash64(reading_id, version, toString(amount), ifNull(note, '<null>'), label, qty))) AS row_hash
FROM oo_dl01w40.readings
FORMAT JSONEachRow
