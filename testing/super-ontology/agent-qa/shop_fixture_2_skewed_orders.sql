SELECT setseed(0.4217);
DROP TABLE order_items; DROP TABLE orders;
CREATE TABLE public.orders(id int PRIMARY KEY, customer_id int NOT NULL REFERENCES public.customers(id), status text NOT NULL, total numeric(12,2) NOT NULL DEFAULT 0, ordered_on date NOT NULL);
CREATE TABLE public.order_items(id int PRIMARY KEY, order_id int NOT NULL REFERENCES public.orders(id), product_id int NOT NULL REFERENCES public.products(id), quantity int NOT NULL, unit_price numeric(10,2) NOT NULL);
-- skewed customers: floor(200*r^2)+1
INSERT INTO orders SELECT g, 1+floor(200*power(random(),2))::int, (ARRAY['paid','paid','paid','shipped','cancelled'])[1+floor(random()*5)::int], 0, DATE '2026-01-01'+floor(random()*240)::int FROM generate_series(1,3000) g;
INSERT INTO order_items SELECT g, 1+((g-1)/3), p, 1+floor(random()*9)::int, (50+p*10)::numeric
 FROM (SELECT g, 1+floor(50*power(random(),1.7))::int p FROM generate_series(1,9000) g) s;
UPDATE orders o SET total=(SELECT sum(quantity*unit_price) FROM order_items i WHERE i.order_id=o.id);
GRANT SELECT ON ALL TABLES IN SCHEMA public TO oo_reader;
ANALYZE;
