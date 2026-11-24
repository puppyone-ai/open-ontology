-- docs-index/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-sources-jobs/1, inside the
-- installer transaction (advisory lock 6413202611). No BEGIN or COMMIT here;
-- the installer inserts the schema_migrations row.
-- Entities, facts, evidence, overlays, review items, ontology versions (10 §3.2).

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-sources-jobs/1') THEN
    RAISE EXCEPTION 'docs-sources-jobs/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

CREATE TABLE _oo_docs.entity (
  id uuid PRIMARY KEY,
  key text NOT NULL UNIQUE CHECK (key ~ '^[a-z_]+:[0-9a-f]{24}$'),   -- §3.6
  type text NOT NULL CHECK (type IN ('Person','Organization','Project','Decision','Document','Topic','Event','Term','Thing')),
  thing_label text CHECK ((type = 'Thing') = (thing_label IS NOT NULL)),
  subkind text CHECK (subkind IN ('preference')),                     -- 见 §15-Q5
  canonical_name text NOT NULL CHECK (char_length(canonical_name) BETWEEN 1 AND 256),
  norm_name text NOT NULL,
  identity jsonb NOT NULL DEFAULT '{}',   -- 确定性身份属性：email、工号、域名…（合并依据）
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active','merged','withdrawn')),
  merged_into uuid REFERENCES _oo_docs.entity(id),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  revision bigint NOT NULL DEFAULT 1,
  CHECK ((status = 'merged') = (merged_into IS NOT NULL)));
CREATE INDEX entity_norm ON _oo_docs.entity (type, norm_name) WHERE status = 'active';

CREATE TABLE _oo_docs.entity_redirect (
  from_key text PRIMARY KEY, to_key text NOT NULL,
  reason text NOT NULL CHECK (reason IN ('merge','identity','human')),
  review_id uuid, at timestamptz NOT NULL DEFAULT clock_timestamp());

CREATE TABLE _oo_docs.alias (
  id uuid PRIMARY KEY, entity_id uuid NOT NULL REFERENCES _oo_docs.entity(id),
  alias text NOT NULL CHECK (char_length(alias) BETWEEN 1 AND 256), norm_alias text NOT NULL,
  origin text NOT NULL CHECK (origin IN ('rule','llm','human','suggest')),
  confidence_bp integer NOT NULL CHECK (confidence_bp BETWEEN 0 AND 10000),
  status text NOT NULL CHECK (status IN ('draft','published','retracted','rejected')),
  published_in bigint, retracted_in bigint,
  UNIQUE (entity_id, norm_alias));
CREATE INDEX alias_lookup ON _oo_docs.alias (norm_alias) WHERE status IN ('draft','published');

CREATE TABLE _oo_docs.fact (
  id uuid PRIMARY KEY,
  fact_key text NOT NULL CHECK (fact_key ~ '^[0-9a-f]{64}$'),   -- §3.6，重抽取得同一 key
  subject_id uuid NOT NULL REFERENCES _oo_docs.entity(id),
  original_subject_id uuid NOT NULL,                             -- 合并前主语，用于拆分
  predicate text NOT NULL CHECK (predicate ~ '^(owns|member_of|decides|supersedes|rejects|mentions|about|tried_and_failed|attr\.[a-z_]{1,40})$'),
  object_id uuid REFERENCES _oo_docs.entity(id),
  original_object_id uuid,
  value jsonb,                                                   -- 属性值：{"text":…}|{"date":…}|{"enum":…}
  CHECK ((object_id IS NULL) <> (value IS NULL)),
  valid_from date, valid_to date, valid_precision text CHECK (valid_precision IN ('day','month','year','doc_date','file_mtime','unknown')),
  recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  retracted_at timestamptz,
  CHECK (valid_to IS NULL OR valid_from IS NULL OR valid_from <= valid_to),
  confidence_bp integer NOT NULL CHECK (confidence_bp BETWEEN 0 AND 10000),
  origin text NOT NULL CHECK (origin IN ('rule','llm','human','suggest')),
  rule_id text, prompt_version text, extraction_key text,
  status text NOT NULL CHECK (status IN ('draft','published','retracted','rejected')),
  published_in bigint, retracted_in bigint,
  superseded_by uuid REFERENCES _oo_docs.fact(id),
  CHECK (published_in IS NULL OR retracted_in IS NULL OR published_in < retracted_in));
CREATE INDEX fact_subject ON _oo_docs.fact (subject_id, predicate) WHERE status = 'published';
CREATE INDEX fact_object ON _oo_docs.fact (object_id) WHERE status = 'published';
CREATE INDEX fact_key_idx ON _oo_docs.fact (fact_key);
CREATE INDEX fact_version ON _oo_docs.fact (published_in, retracted_in);

CREATE TABLE _oo_docs.evidence (
  id uuid PRIMARY KEY,
  target_kind text NOT NULL CHECK (target_kind IN ('fact','alias','entity')),
  target_id uuid NOT NULL,
  version_id uuid NOT NULL REFERENCES _oo_docs.file_version(id),
  b3 text NOT NULL,
  block_id text NOT NULL CHECK (block_id ~ '^blk_[0-9a-f]{20}$'),
  anchor jsonb NOT NULL,                 -- oo.doc-ir/v1 Anchor
  quote_b3 text NOT NULL,                -- 原文片段哈希；片段本身在 IR 里，不复制进库
  origin text NOT NULL CHECK (origin IN ('rule','llm','human','suggest')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  withdrawn_at timestamptz,
  withdrawn_reason text CHECK (withdrawn_reason IN ('file_deleted','file_superseded','reprocessed','human')));
CREATE INDEX evidence_target ON _oo_docs.evidence (target_kind, target_id) WHERE withdrawn_at IS NULL;
CREATE INDEX evidence_version ON _oo_docs.evidence (version_id) WHERE withdrawn_at IS NULL;

CREATE TABLE _oo_docs.overlay (
  id uuid PRIMARY KEY,
  kind text NOT NULL CHECK (kind IN ('fact_assert','fact_reject','fact_edit','entity_merge','entity_split',
        'alias_add','alias_reject','type_promote','accept_source','suggestion_accept')),
  binds text NOT NULL,                   -- fact_key / entity key / alias norm
  payload jsonb NOT NULL, author text NOT NULL, review_id uuid,
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active','orphaned','withdrawn')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp());
-- overlay 只追加：状态变化写新行（kind 相同，payload.supersedes_overlay），旧行用视图过滤
CREATE TRIGGER overlay_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.overlay
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();

CREATE TABLE _oo_docs.review_item (
  id uuid PRIMARY KEY,
  kind text NOT NULL CHECK (kind IN ('low_confidence_fact','entity_merge','alias_conflict','orphaned_overlay',
        'type_promotion','suggestion')),
  status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','accepted','rejected','superseded')),
  dedupe_key text NOT NULL UNIQUE,
  drive_id uuid, source_file_id uuid, source_version_id uuid,
  subject jsonb NOT NULL,                -- {fact_id}|{entity_ids:[…]}|{alias,entity_ids}|{overlay_id}|{label}
  confidence_bp integer, reason_code text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  decided_at timestamptz, decided_by text, decision_note text CHECK (char_length(decision_note) <= 500),
  revision bigint NOT NULL DEFAULT 1);
CREATE INDEX review_pending ON _oo_docs.review_item (kind, source_file_id, created_at) WHERE status = 'pending';

CREATE TABLE _oo_docs.ontology_version (
  n bigint PRIMARY KEY CHECK (n >= 1),
  parent bigint, as_of date NOT NULL,    -- as_of = 发布当日（UTC），BRIEF 的有效性判断只用它，不用“现在”
  facts_added integer NOT NULL, facts_retracted integer NOT NULL,
  digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  gate_report_sha256 text NOT NULL,
  trigger text NOT NULL CHECK (trigger IN ('auto','review','overlay','reprocess','manual')),
  published_at timestamptz NOT NULL DEFAULT clock_timestamp());
CREATE TABLE _oo_docs.gate_report (
  n bigint NOT NULL REFERENCES _oo_docs.ontology_version(n),
  gate_id text NOT NULL, disposition text NOT NULL CHECK (disposition IN ('pass','warn','blocked')),
  detail jsonb NOT NULL, evidence_sha256 text NOT NULL, PRIMARY KEY (n, gate_id));

-- Runtime grants. overlay, ontology_version, gate_report, entity_redirect are
-- append-only; the rest are mutable by the engine.
CREATE TRIGGER ontology_version_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.ontology_version
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER gate_report_insert_only BEFORE UPDATE OR DELETE ON _oo_docs.gate_report
  FOR EACH ROW EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER overlay_no_truncate BEFORE TRUNCATE ON _oo_docs.overlay
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER ontology_version_no_truncate BEFORE TRUNCATE ON _oo_docs.ontology_version
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();
CREATE TRIGGER gate_report_no_truncate BEFORE TRUNCATE ON _oo_docs.gate_report
  FOR EACH STATEMENT EXECUTE FUNCTION _oo_docs.reject_mutation();
GRANT SELECT, INSERT, UPDATE ON _oo_docs.entity, _oo_docs.alias, _oo_docs.fact,
  _oo_docs.evidence, _oo_docs.review_item TO oo_docs_runtime;
GRANT SELECT, INSERT ON _oo_docs.entity_redirect, _oo_docs.overlay,
  _oo_docs.ontology_version, _oo_docs.gate_report TO oo_docs_runtime;
