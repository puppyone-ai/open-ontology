CREATE SCHEMA shop;
CREATE TABLE shop.customer (
  customer_id int8 PRIMARY KEY,
  display_name text,
  active bool NOT NULL,
  private_note text
);
CREATE TABLE shop.order_doc (
  order_id int8 PRIMARY KEY,
  customer_id int8 NOT NULL,
  memo text,
  posted bool NOT NULL,
  amount int8 NOT NULL,
  clerk_note text,
  CONSTRAINT order_customer_fk FOREIGN KEY (customer_id) REFERENCES shop.customer (customer_id)
);
COMMENT ON TABLE shop.customer IS '甲注评客户名录';
COMMENT ON COLUMN shop.customer.display_name IS '甲注评名称';
CREATE TABLE shop.write_audit (who text NOT NULL);
CREATE FUNCTION shop.note_write() RETURNS trigger LANGUAGE plpgsql AS $audit$
BEGIN
  INSERT INTO shop.write_audit (who) VALUES (current_user);
  RETURN NEW;
END;
$audit$;
CREATE TRIGGER customer_audit BEFORE INSERT OR UPDATE OR DELETE ON shop.customer
  FOR EACH ROW EXECUTE FUNCTION shop.note_write();
CREATE TRIGGER order_audit BEFORE INSERT OR UPDATE OR DELETE ON shop.order_doc
  FOR EACH ROW EXECUTE FUNCTION shop.note_write();
INSERT INTO shop.customer (customer_id, display_name, active, private_note) VALUES
  (41, '丙号', true, '内部甲'),
  (42, NULL, false, NULL),
  (43, '丁铺', true, '内部乙');
INSERT INTO shop.order_doc (order_id, customer_id, memo, posted, amount, clerk_note) VALUES
  (701, 41, '首单', true, -9223372036854775808, '柜员甲'),
  (702, 41, NULL, false, 9223372036854775807, NULL),
  (703, 43, '零单', true, 0, '柜员乙');
REVOKE ALL ON SCHEMA shop FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;
GRANT USAGE ON SCHEMA shop TO oo_a226_reader, oo_a226_writer;
GRANT SELECT ON shop.customer, shop.order_doc TO oo_a226_reader;
GRANT INSERT ON shop.customer, shop.order_doc, shop.write_audit TO oo_a226_writer;
