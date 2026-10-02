CREATE TABLE public.fixture_future (id integer PRIMARY KEY, note text);
GRANT INSERT, SELECT ON public.fixture_future TO oo_pipeline_runtime;
