SET statement_timeout = '120s';
CREATE TABLE public.batch_a (id int8 PRIMARY KEY, amount numeric(38,4), label text);
CREATE TABLE public.batch_b (id int8 PRIMARY KEY, at_time time, blob bytea);
CREATE TABLE public.batch_c (id int8 PRIMARY KEY, qty int8, amount numeric(38,4), note text);
INSERT INTO public.batch_a
SELECT g,
       CASE WHEN g = 1 THEN NULL WHEN g = 2 THEN 1234567890123456789012345678901234.1234 ELSE 0.0001 END,
       CASE WHEN g = 3 THEN '' ELSE 'n' END
FROM generate_series(1, 2000) AS g;
INSERT INTO public.batch_b
SELECT g,
       CASE WHEN g = 2 THEN NULL ELSE time '13:14:15.123456' END,
       CASE WHEN g = 3 THEN NULL ELSE '\x0001ff'::bytea END
FROM generate_series(1, 8000) AS g;
INSERT INTO public.batch_c
SELECT g,
       CASE WHEN g = 1 THEN 9223372036854775807 WHEN g = 2 THEN -9223372036854775808 ELSE g END,
       CASE WHEN g = 4 THEN NULL ELSE 1.5000 END,
       CASE WHEN g = 5 THEN NULL ELSE 'c' END
FROM generate_series(1, 90000) AS g;
GRANT CONNECT ON DATABASE oo_a407_src TO oo_a407_src_read;
GRANT USAGE ON SCHEMA public TO oo_a407_src_read;
GRANT SELECT ON TABLE public.batch_a, public.batch_b, public.batch_c TO oo_a407_src_read;
