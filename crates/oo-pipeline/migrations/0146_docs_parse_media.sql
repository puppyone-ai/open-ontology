-- docs-parse-media/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-share-consistency/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- L6 multimodal (10 17.8, D77): audio and video parse jobs have their own kind and pool,
-- blobs may be `audio` or `video`, and the speaker identity mapping is an overlay kind.
-- `parse_media` behaves like `parse` everywhere the job functions treat `parse` specially
-- (retry delay cap, quarantine at the attempt cap, 10 4.1.6).

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-share-consistency/1') THEN
    RAISE EXCEPTION 'docs-share-consistency/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

ALTER TABLE _oo_docs.job DROP CONSTRAINT job_kind_check;
ALTER TABLE _oo_docs.job ADD CONSTRAINT job_kind_check CHECK (kind IN ('sync', 'hash', 'parse', 'parse_media',
  'normalize', 'extract_rules', 'extract_llm', 'resolve', 'publish', 'compile', 'gc', 'expire', 'drift', 'reprocess'));

ALTER TABLE _oo_docs.blob DROP CONSTRAINT blob_format_check;
ALTER TABLE _oo_docs.blob ADD CONSTRAINT blob_format_check CHECK (format IN ('md', 'txt', 'csv', 'docx', 'xlsx',
  'pptx', 'pdf', 'pdf_scanned', 'image', 'audio', 'video', 'legacy_office', 'zip', 'unknown'));

ALTER TABLE _oo_docs.overlay DROP CONSTRAINT overlay_kind_check;
ALTER TABLE _oo_docs.overlay ADD CONSTRAINT overlay_kind_check CHECK (kind IN ('fact_assert','fact_reject','fact_edit',
  'entity_merge','entity_split','alias_add','alias_reject','type_promote','accept_source','suggestion_accept',
  'speaker_map'));

-- The job functions of 0138 are rewritten from their own definitions so no body is copied by hand:
-- every `v_row.kind = 'parse'` becomes `IN ('parse', 'parse_media')`, and the per kind retry cap gets the same
-- 30 minute entry for `parse_media`. The block fails when a pattern is not found exactly where it is expected.
DO $rewrite$
DECLARE
  f record;
  def text;
  new_def text;
  touched integer := 0;
  cap_done boolean := false;
BEGIN
  FOR f IN
    SELECT p.oid, p.proname
    FROM pg_catalog.pg_proc p
    JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = '_oo_docs'
      AND (p.prosrc LIKE '%v_row.kind = ''parse''%' OR p.proname = '_retry_delay_cap_ms')
    ORDER BY p.proname
  LOOP
    def := pg_catalog.pg_get_functiondef(f.oid);
    IF f.proname = '_retry_delay_cap_ms' THEN
      new_def := pg_catalog.replace(def, 'WHEN ''parse'' THEN 1800000',
                                    'WHEN ''parse'' THEN 1800000 WHEN ''parse_media'' THEN 1800000');
      cap_done := new_def <> def;
    ELSE
      new_def := pg_catalog.replace(def, 'v_row.kind = ''parse''', 'v_row.kind IN (''parse'', ''parse_media'')');
    END IF;
    IF new_def = def THEN
      RAISE EXCEPTION 'docs-parse-media/1: no change in %', f.proname USING ERRCODE = 'OO002';
    END IF;
    EXECUTE new_def;
    touched := touched + 1;
  END LOOP;
  IF touched < 3 OR NOT cap_done THEN
    RAISE EXCEPTION 'docs-parse-media/1: expected claim, retry and the retry cap, rewrote %', touched
      USING ERRCODE = 'OO002';
  END IF;
END
$rewrite$;
