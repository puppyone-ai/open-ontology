CREATE TABLE public.customers(id int PRIMARY KEY, name text NOT NULL, region text NOT NULL, tier text NOT NULL);
CREATE TABLE public.products(id int PRIMARY KEY, name text NOT NULL, category text NOT NULL, price numeric(10,2) NOT NULL);
CREATE TABLE public.orders(id int PRIMARY KEY, customer_id int NOT NULL REFERENCES public.customers(id), status text NOT NULL, total numeric(12,2) NOT NULL, ordered_on date NOT NULL);
CREATE TABLE public.order_items(id int PRIMARY KEY, order_id int NOT NULL REFERENCES public.orders(id), product_id int NOT NULL REFERENCES public.products(id), quantity int NOT NULL, unit_price numeric(10,2) NOT NULL);
INSERT INTO customers SELECT g, CASE g WHEN 1 THEN '澄海物流' WHEN 2 THEN '远帆咨询' WHEN 3 THEN '北辰云' ELSE 'Customer '||lpad(g::text,3,'0') END,
  (ARRAY['east','north','south','west'])[1+(g%4)], (ARRAY['gold','silver','bronze'])[1+(g%3)] FROM generate_series(1,200) g;
INSERT INTO products SELECT g, CASE g WHEN 1 THEN '星图看板企业版' WHEN 2 THEN '灯塔迁移服务包' WHEN 3 THEN '知了客服标准版' WHEN 4 THEN '官网改版设计服务' ELSE 'Widget '||lpad(g::text,2,'0') END,
  (ARRAY['software','service','hardware','support','training'])[1+(g%5)], (50+g*10)::numeric FROM generate_series(1,50) g;
INSERT INTO orders SELECT g, 1+((g*7)%200), (ARRAY['paid','paid','paid','shipped','cancelled'])[1+(g%5)], 0, DATE '2026-01-01'+(g%240) FROM generate_series(1,3000) g;
INSERT INTO order_items SELECT g, 1+((g-1)/3), 1+((g*13)%50), 1+(g%5), (50+(1+((g*13)%50))*10)::numeric FROM generate_series(1,9000) g;
UPDATE orders o SET total=(SELECT sum(quantity*unit_price) FROM order_items i WHERE i.order_id=o.id);
