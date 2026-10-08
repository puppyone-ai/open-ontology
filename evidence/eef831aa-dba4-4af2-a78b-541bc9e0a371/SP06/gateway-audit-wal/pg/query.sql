CREATE SCHEMA IF NOT EXISTS _oo;

CREATE TABLE IF NOT EXISTS _oo.pt_entry (
    pc_entry_id int8 PRIMARY KEY,
    pc_amount numeric(18,4),
    pc_wide numeric(38,4),
    hidden int8 NOT NULL
);

INSERT INTO _oo.pt_entry (pc_entry_id, pc_amount, pc_wide, hidden)
SELECT seed.pc_entry_id, seed.pc_amount, seed.pc_wide, seed.hidden
FROM (VALUES
    (1::int8, 12345678901234.5678::numeric(18,4), 1234567890123456789012345678901234.5678::numeric(38,4), 1::int8),
    (2::int8, -12345678901234.5678::numeric(18,4), -1234567890123456789012345678901234.5678::numeric(38,4), 1::int8),
    (3::int8, 0.0001::numeric(18,4), 0.0001::numeric(38,4), 1::int8),
    (4::int8, -0.0001::numeric(18,4), -0.0001::numeric(38,4), 1::int8),
    (5::int8, 0.0000::numeric(18,4), 0.0000::numeric(38,4), 1::int8),
    (6::int8, NULL::numeric(18,4), NULL::numeric(38,4), 1::int8)
) AS seed(pc_entry_id, pc_amount, pc_wide, hidden)
WHERE NOT EXISTS (SELECT 1 FROM _oo.pt_entry);

REVOKE ALL ON SCHEMA _oo FROM PUBLIC;
GRANT USAGE ON SCHEMA _oo TO oo_r_clerk;
GRANT SELECT (pc_entry_id, pc_amount, pc_wide) ON _oo.pt_entry TO oo_r_clerk;
