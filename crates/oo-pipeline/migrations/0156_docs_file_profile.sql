-- docs-file-profile/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-drive-scale/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- D89: every file has a profile (what kind of document it is) that decides how it is read. The column is nullable: a
-- row that was never extracted shows the profile the API derives from its path and format. Nothing is rewritten.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-drive-scale/1') THEN
    RAISE EXCEPTION 'docs-drive-scale/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

ALTER TABLE _oo_docs.file ADD COLUMN profile text
  CHECK (profile IN ('business', 'meeting', 'table', 'instruction', 'code', 'reference', 'media', 'other'));
-- Why the file has that profile: a rule (frontmatter, name, extension, directory, format), its content, the model, or the user.
ALTER TABLE _oo_docs.file ADD COLUMN profile_reason text
  CHECK (profile_reason IN ('frontmatter', 'name', 'dir', 'ext', 'format', 'content', 'default', 'model', 'user'));

-- A person's choice of the class of one file or of a directory and everything below it (D94). A file's own choice wins over a
-- directory's, the nearest directory over a farther one.
CREATE TABLE _oo_docs.profile_override (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  scope text NOT NULL CHECK (scope IN ('file', 'dir')),
  path text NOT NULL CHECK (char_length(path) <= 1024),
  profile text NOT NULL
    CHECK (profile IN ('business', 'meeting', 'table', 'instruction', 'code', 'reference', 'media', 'other')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, scope, path)
);
GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.profile_override TO oo_docs_runtime;

-- D94: two relations of a reference (`cites`, `authored`) join the core ones.
ALTER TABLE _oo_docs.fact DROP CONSTRAINT fact_predicate_check;
ALTER TABLE _oo_docs.fact ADD CONSTRAINT fact_predicate_check CHECK (
  predicate ~ '^(owns|member_of|decides|supersedes|rejects|mentions|about|tried_and_failed|cites|authored|attr\.[a-z_]{1,40})$'
);
