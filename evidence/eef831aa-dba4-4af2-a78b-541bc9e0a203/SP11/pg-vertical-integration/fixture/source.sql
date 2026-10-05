-- a203 enterprise fixture. Applied statement-by-statement by the harness.
-- Types stay in the executor set: bool, int8, text.
CREATE SCHEMA ent;
CREATE TABLE ent.party (
    party_id int8 PRIMARY KEY,
    display_name text,
    active bool NOT NULL,
    credit_limit int8 NOT NULL,
    internal_note text
);
COMMENT ON TABLE ent.party IS '交易方主数据';
COMMENT ON COLUMN ent.party.display_name IS '可空中文名称';
COMMENT ON COLUMN ent.party.internal_note IS '不对主体角色公开';
CREATE TABLE ent.invoice (
    invoice_id int8 PRIMARY KEY,
    party_id int8 NOT NULL,
    memo text,
    posted bool NOT NULL,
    amount int8,
    clerk_note text,
    CONSTRAINT invoice_party_fk FOREIGN KEY (party_id) REFERENCES ent.party (party_id)
);
COMMENT ON TABLE ent.invoice IS '发票单据';
COMMENT ON COLUMN ent.invoice.memo IS '可空中文摘要';
COMMENT ON COLUMN ent.invoice.clerk_note IS '不对发票角色公开';
INSERT INTO ent.party (party_id, display_name, active, credit_limit, internal_note) VALUES
    (1, '华能机电', true, 9223372036854775807, '限额'),
    (2, NULL, false, -9223372036854775808, NULL),
    (3, '李四', true, 0, '空额');
INSERT INTO ent.invoice (invoice_id, party_id, memo, posted, amount, clerk_note) VALUES
    (100, 1, '首张发票', true, 9223372036854775807, '已核'),
    (101, 1, NULL, false, NULL, NULL),
    (102, 2, '负数边界', true, -9223372036854775808, '边界');
GRANT USAGE ON SCHEMA ent TO oo_a203_reader;
GRANT SELECT ON ent.party, ent.invoice TO oo_a203_reader;
