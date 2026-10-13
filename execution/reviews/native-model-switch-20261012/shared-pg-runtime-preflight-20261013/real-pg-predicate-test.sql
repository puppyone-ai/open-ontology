BEGIN;
INSERT INTO _oo.source_watermark VALUES ('root_pg_probe','per_table','T',100,NULL,now(),'lagging',false,1,1,'pg-commit-lsn/v1','root-probe');
INSERT INTO _oo.table_freshness VALUES ('root_probe','already','root_pg_probe',1,1,'pt_probe',100,NULL,now(),'incremental','awaiting_publish');
INSERT INTO _oo.table_freshness VALUES ('root_probe','behind','root_pg_probe',1,1,'pt_probe',90,NULL,now(),'incremental','awaiting_publish');
SET LOCAL ROLE oo_pipeline_runtime;
DO $$ BEGIN
IF _oo.native_pg_position_ahead('root_pg_probe','root_probe','already',100) THEN RAISE EXCEPTION 'same table replay incorrectly accepted'; END IF;
IF NOT _oo.native_pg_position_ahead('root_pg_probe','root_probe','behind',100) THEN RAISE EXCEPTION 'same commit other table refused'; END IF;
IF NOT _oo.native_pg_position_ahead('root_pg_probe','root_probe','new_table',100) THEN RAISE EXCEPTION 'same commit new table refused'; END IF;
IF _oo.native_pg_position_ahead('root_pg_probe','root_probe','behind',99) THEN RAISE EXCEPTION 'backward global LSN accepted'; END IF;
IF NOT _oo.native_pg_position_ahead('root_pg_probe','root_probe','already',101) THEN RAISE EXCEPTION 'forward LSN refused'; END IF;
BEGIN PERFORM 1 FROM _oo.source_watermark; RAISE EXCEPTION 'runtime unexpectedly has SELECT'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
