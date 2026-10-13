-- Fixture DBA only. The reader account never runs this file.
CREATE DATABASE IF NOT EXISTS oo_dl01;

CREATE TABLE oo_dl01.readings
(
    reading_id UInt64,
    version UInt64,
    amount Decimal(76, 10),
    note Nullable(String),
    code LowCardinality(String),
    code_or_null LowCardinality(Nullable(String)),
    ts DateTime64(9, 'UTC'),
    tags Array(Int64),
    labels Array(Nullable(String)),
    qty Int64 DEFAULT 0,
    label String DEFAULT '',
    wide_u64 UInt64
)
ENGINE = ReplacingMergeTree(version)
PARTITION BY toYYYYMM(ts)
ORDER BY reading_id;

SYSTEM STOP MERGES oo_dl01.readings;

INSERT INTO oo_dl01.readings
    (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64)
VALUES
(
    101,
    1,
    CAST('1.0000000000' AS Decimal(76, 10)),
    'stale',
    'old',
    'old',
    toDateTime64('2024-03-15 12:00:00.000000001', 9, 'UTC'),
    [1],
    ['stale'],
    1,
    'stale',
    CAST('1' AS UInt64)
);

INSERT INTO oo_dl01.readings
    (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64)
VALUES
(
    102,
    1,
    CAST('0.0000000000' AS Decimal(76, 10)),
    '',
    'alpha',
    'beta',
    toDateTime64('2024-01-02 00:00:00.000000000', 9, 'UTC'),
    CAST([] AS Array(Int64)),
    [CAST(NULL AS Nullable(String))],
    0,
    '',
    CAST('0' AS UInt64)
);

INSERT INTO oo_dl01.readings
    (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, wide_u64)
VALUES
(
    103,
    1,
    CAST('-42.5000000000' AS Decimal(76, 10)),
    'present',
    'gamma',
    '',
    toDateTime64('2024-01-31 23:59:59.999999999', 9, 'UTC'),
    [toInt64('9223372036854775807'), toInt64('-9223372036854775808')],
    CAST([] AS Array(Nullable(String))),
    CAST('42' AS UInt64)
);

INSERT INTO oo_dl01.readings
    (reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64)
VALUES
(
    101,
    2,
    CAST('1234567890123456789012345678901234567890.1234567890' AS Decimal(76, 10)),
    CAST(NULL AS Nullable(String)),
    'alpha',
    CAST(NULL AS Nullable(String)),
    toDateTime64('2024-03-15 12:34:56.123456789', 9, 'UTC'),
    [1, -2, 0],
    ['alpha', CAST(NULL AS Nullable(String)), ''],
    7,
    'kept',
    CAST('18446744073709551615' AS UInt64)
);

CREATE TABLE oo_dl01.retirees
(
    id UInt64,
    payload String
)
ENGINE = MergeTree
ORDER BY id;

INSERT INTO oo_dl01.retirees VALUES (9, 'visible-before-delete'), (10, 'stays');

DELETE FROM oo_dl01.retirees WHERE id = 9 SETTINGS mutations_sync = 0;
