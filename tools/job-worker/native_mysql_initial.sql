-- Add exact initial-resume stages without broadening the existing audit set.
DO $mysql_initial_audit$
DECLARE definition text; predicate text; valid boolean;
BEGIN
  SELECT pg_get_constraintdef(oid) INTO STRICT definition FROM pg_constraint
    WHERE conrelid='jw_native.process_attempts'::regclass
      AND conname='process_attempts_stage_check' AND contype='c' AND convalidated;
  IF left(definition,6)<>'CHECK ' THEN RAISE EXCEPTION 'closed audit stage guard required'; END IF;
  predicate:=substring(definition from 7);
  IF position('loader_c1_initial_resume' in definition)=0
    OR position('loader_c1_initial_end' in definition)=0
    OR position('normalization_rejects_' in definition)=0 THEN
    ALTER TABLE jw_native.process_attempts DROP CONSTRAINT process_attempts_stage_check;
    EXECUTE 'ALTER TABLE jw_native.process_attempts ADD CONSTRAINT process_attempts_stage_check CHECK ('||
      predicate||' OR stage IN (''loader_c1_initial_resume'',''loader_c1_initial_end'')'||
      ' OR stage ~ ''^normalization_rejects_(0|[1-9][0-9]{0,2})$'')';
  END IF;
  SELECT substring(pg_get_constraintdef(oid) from 7) INTO STRICT predicate FROM pg_constraint
    WHERE conrelid='jw_native.process_attempts'::regclass AND conname='process_attempts_stage_check';
  EXECUTE 'SELECT bool_and(allowed IS NOT DISTINCT FROM ('||predicate||')) FROM (VALUES '||
    '(''loader_c1_initial_resume'',true),(''loader_c1_initial_end'',true),'||
    '(''loader_c1_initial_arbitrary'',false),(''loader_c1_initial_resume_1'',false),'||
    '(''loader_c1_export_meta'',true),(''loader_c1_export_page_000000'',true),'||
    '(''normalization_rejects_999'',true),(''normalization_rejects_1000'',false),'||
    '(''loader_c1_stage_snapshot'',true),(''ack'',true),(''unapproved_stage'',false)) t(stage,allowed)'
    INTO valid;
  IF valid IS DISTINCT FROM true THEN RAISE EXCEPTION 'MySQL initial audit guard mismatch'; END IF;
END $mysql_initial_audit$;
