CREATE TABLE public.side_note (id integer PRIMARY KEY, note text);
GRANT INSERT, SELECT ON public.side_note TO oo_pipeline_runtime;
