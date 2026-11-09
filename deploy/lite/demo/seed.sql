-- Public synthetic OO-Logistics subset, adapted from testing/engines/seed-common.
-- Fixture administration only. Product readers never execute this file.
\set ON_ERROR_STOP on
BEGIN;
CREATE SCHEMA demo;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
CREATE TABLE demo.regions (id bigint PRIMARY KEY, name text NOT NULL);
CREATE TABLE demo.areas (id bigint PRIMARY KEY, region_id bigint NOT NULL REFERENCES demo.regions, name text NOT NULL);
CREATE TABLE demo.sites (id bigint PRIMARY KEY, area_id bigint NOT NULL REFERENCES demo.areas, name text NOT NULL);
CREATE TABLE demo.codes (code bigint PRIMARY KEY, label text NOT NULL);
CREATE TABLE demo.customers (id bigint PRIMARY KEY, site_id bigint NOT NULL REFERENCES demo.sites, name text NOT NULL, phone_synthetic text NOT NULL);
CREATE TABLE demo.orders (id bigint PRIMARY KEY, customer_id bigint NOT NULL REFERENCES demo.customers, status_code bigint NOT NULL REFERENCES demo.codes, region_id bigint NOT NULL REFERENCES demo.regions, amount numeric(18,4) NOT NULL, qty bigint NOT NULL, ordered_at timestamp NOT NULL);
CREATE TABLE demo.shipments (id bigint PRIMARY KEY, order_id bigint NOT NULL REFERENCES demo.orders, site_id bigint NOT NULL REFERENCES demo.sites, shipped_at timestamp NOT NULL);
CREATE TABLE demo.calendar (day_id bigint PRIMARY KEY, day_date date NOT NULL, month_no bigint NOT NULL);
CREATE TABLE demo.order_lines (order_id bigint REFERENCES demo.orders, line_no bigint, qty bigint NOT NULL, PRIMARY KEY(order_id,line_no));
CREATE TABLE demo.events (id bigint PRIMARY KEY, order_id bigint NOT NULL REFERENCES demo.orders, event_no bigint NOT NULL);
INSERT INTO demo.regions SELECT n, 'Region ' || n FROM generate_series(1,2) n;
INSERT INTO demo.areas SELECT n, 1+(n-1)/2, 'Area ' || n FROM generate_series(1,4) n;
INSERT INTO demo.sites SELECT n, 1+(n-1)/2, 'Site ' || n FROM generate_series(1,8) n;
INSERT INTO demo.codes VALUES (1,'New'),(2,'Shipped'),(3,'Closed');
INSERT INTO demo.customers SELECT n, 1+(n-1)%8, 'Customer ' || n, 'SYN-PHONE-' || n FROM generate_series(1,24) n;
INSERT INTO demo.orders SELECT n, 1+(n-1)%24, 1+(n-1)%3, 1+((n-1)%8)/4,
  n+0.1250, 1+n%5, TIMESTAMP '2026-01-01 12:34:56' + ((n-1)/60)*INTERVAL '1 month' + ((n-1)%28)*INTERVAL '1 day'
  FROM generate_series(1,120) n;
INSERT INTO demo.shipments SELECT id,id,1+(id-1)%8,ordered_at FROM demo.orders WHERE id%4<>0;
INSERT INTO demo.calendar SELECT n, DATE '2026-01-01'+n-1,1 FROM generate_series(1,31) n;
INSERT INTO demo.order_lines SELECT id,n,n FROM demo.orders CROSS JOIN generate_series(1,2) n;
INSERT INTO demo.events SELECT n,1+(n-1)%120,n%7 FROM generate_series(1,20000) n;
CREATE ROLE oo_demo_reader LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS;
CREATE ROLE oo_r_analyst NOLOGIN NOINHERIT NOBYPASSRLS;
CREATE ROLE oo_r_ops NOLOGIN NOINHERIT NOBYPASSRLS;
GRANT USAGE ON SCHEMA demo TO oo_demo_reader,oo_r_analyst,oo_r_ops;
GRANT SELECT ON ALL TABLES IN SCHEMA demo TO oo_demo_reader;
GRANT SELECT ON demo.regions,demo.areas,demo.sites,demo.codes,demo.orders,demo.shipments,demo.calendar,demo.order_lines,demo.events TO oo_r_analyst,oo_r_ops;
GRANT SELECT (id,site_id,name) ON demo.customers TO oo_r_analyst,oo_r_ops;
ALTER TABLE demo.orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE demo.orders FORCE ROW LEVEL SECURITY;
CREATE POLICY source_capture ON demo.orders FOR SELECT TO oo_demo_reader USING (true);
CREATE POLICY analyst_region ON demo.orders FOR SELECT TO oo_r_analyst USING (region_id=1);
CREATE POLICY ops_region ON demo.orders FOR SELECT TO oo_r_ops USING (region_id=2);
ALTER ROLE oo_demo_reader SET default_transaction_read_only=on;
COMMENT ON SCHEMA demo IS 'local_fixture: public synthetic OO-Logistics demo, not customer data';
COMMIT;
