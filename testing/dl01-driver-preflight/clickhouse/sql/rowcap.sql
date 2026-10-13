SELECT toUInt64(number) AS reading_id FROM numbers(100)
FORMAT ArrowStream
