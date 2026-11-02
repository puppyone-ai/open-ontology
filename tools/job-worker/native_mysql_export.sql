-- Add only bounded MySQL export audit stages; retain the current closed set.
DO $mysql_export_audit$
DECLARE definition text; predicate text; valid boolean;
BEGIN
  SELECT pg_get_constraintdef(oid) INTO STRICT definition FROM pg_constraint
    WHERE conrelid='jw_native.process_attempts'::regclass
      AND conname='process_attempts_stage_check' AND contype='c' AND convalidated;
  IF left(definition,6)<>'CHECK ' THEN RAISE EXCEPTION 'closed audit stage guard required'; END IF;
  predicate:=substring(definition from 7);
  IF position('loader_c1_export_meta' in definition)=0
    OR position('c1_export_resume' in definition)=0
    OR position('loader_c1_export_page_' in definition)=0 THEN
    ALTER TABLE jw_native.process_attempts DROP CONSTRAINT process_attempts_stage_check;
    EXECUTE 'ALTER TABLE jw_native.process_attempts ADD CONSTRAINT process_attempts_stage_check CHECK ('||
      predicate||' OR stage IN (''loader_c1_export_meta'',''c1_export_resume'')'||
      ' OR stage ~ ''^loader_c1_export_page_[0-9]{6}$'')';
  END IF;
  SELECT substring(pg_get_constraintdef(oid) from 7) INTO STRICT predicate FROM pg_constraint
    WHERE conrelid='jw_native.process_attempts'::regclass AND conname='process_attempts_stage_check';
  -- Evaluate the real CHECK without fabricating process_attempts receipts.
  EXECUTE 'SELECT bool_and(allowed IS NOT DISTINCT FROM ('||predicate||')) FROM (VALUES '||
    '(''loader_c1_export_meta'',true),(''c1_export_resume'',true),'||
    '(''loader_c1_export_page_000000'',true),(''loader_c1_export_page_262175'',true),'||
    '(''loader_c1_export_page_1000000'',false),(''loader_c1_export_page_-1'',false),'||
    '(''loader_c1_export_page_1'',false),(''unapproved_stage'',false),'||
    '(''c1_baseline_stream'',true),(''loader_c1_stage_snapshot'',true),(''ack'',true)) t(stage,allowed)'
    INTO valid;
  IF valid IS DISTINCT FROM true THEN RAISE EXCEPTION 'MySQL export audit guard mismatch'; END IF;
END $mysql_export_audit$;
