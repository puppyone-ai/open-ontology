CREATE SCHEMA shop;
CREATE TABLE shop.party (
party_key text PRIMARY KEY,
display_name text,
active boolean NOT NULL,
phone text NOT NULL,
national_id text NOT NULL
);
CREATE TABLE shop.account (
account_id int8 PRIMARY KEY,
party_key text NOT NULL,
opened boolean NOT NULL,
label text,
CONSTRAINT account_party_fk FOREIGN KEY (party_key) REFERENCES shop.party (party_key)
);
CREATE TABLE shop.entry (
entry_id int8 PRIMARY KEY,
account_id int8 NOT NULL,
qty int8 NOT NULL,
amount numeric(18,4),
memo text,
CONSTRAINT entry_account_fk FOREIGN KEY (account_id) REFERENCES shop.account (account_id)
);
COMMENT ON TABLE shop.party IS '甲注评名录';
REVOKE ALL ON SCHEMA shop FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;
GRANT USAGE ON SCHEMA shop TO oo_a328_reader;
GRANT SELECT ON shop.party, shop.account, shop.entry TO oo_a328_reader;
