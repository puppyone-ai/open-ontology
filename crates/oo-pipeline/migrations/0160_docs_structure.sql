-- docs-structure/1 phase expand
-- Run by oo_docs::store::install as oo_docs_owner after docs-file-profile/1, inside the installer
-- transaction. No BEGIN or COMMIT here; the installer inserts the schema_migrations row.
-- D93: the structure layer of the ontology. What the files of a record class share (their properties, types and
-- relations) is induced like the columns of a table, stored as versioned schema objects and confirmed by a person.
-- Additive: nothing existing is rewritten.

SET LOCAL search_path = pg_catalog;

DO $guard$
BEGIN
  IF current_user IS DISTINCT FROM 'oo_docs_owner' THEN
    RAISE EXCEPTION 'docs migration must run as the owner' USING ERRCODE = 'OO013';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM _oo_docs.schema_migrations WHERE version = 'docs-file-profile/1') THEN
    RAISE EXCEPTION 'docs-file-profile/1 is not installed' USING ERRCODE = 'OO002';
  END IF;
END
$guard$;

-- The fields one content carries (frontmatter keys, table headers, section headings, key: value lines), once per blob.
CREATE TABLE _oo_docs.structure_observation (
  b3 text PRIMARY KEY CHECK (b3 ~ '^[0-9a-f]{64}$'),
  obs_version text NOT NULL CHECK (char_length(obs_version) BETWEEN 1 AND 40),
  shape text NOT NULL CHECK (shape IN ('document', 'table')),
  obs jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(obs) = 'object'),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

-- One class of record files with its properties (the document is `oo.docs.structure/v1` `ClassStructure`).
CREATE TABLE _oo_docs.structure_class (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  class_key text NOT NULL CHECK (class_key ~ '^[a-z][a-z0-9_]{0,63}$'),
  label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 128),
  kind text NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 40),
  shape text NOT NULL CHECK (shape IN ('document', 'table')),
  status text NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed', 'rejected')),
  version integer NOT NULL CHECK (version >= 1),
  file_count integer NOT NULL CHECK (file_count >= 0),
  doc jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(doc) = 'object'),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, class_key)
);

-- Which class a record file belongs to (the file, not a version).
CREATE TABLE _oo_docs.structure_member (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  file_id uuid NOT NULL REFERENCES _oo_docs.file (id),
  class_key text NOT NULL,
  PRIMARY KEY (drive_id, file_id),
  FOREIGN KEY (drive_id, class_key) REFERENCES _oo_docs.structure_class (drive_id, class_key) ON DELETE CASCADE
);
CREATE INDEX structure_member_class ON _oo_docs.structure_member (drive_id, class_key);

-- Every change of the structure layer of a Drive is a version: the whole schema as it was, and who changed it.
CREATE TABLE _oo_docs.structure_version (
  drive_id uuid NOT NULL REFERENCES _oo_docs.drive (id),
  n integer NOT NULL CHECK (n >= 1),
  reason text NOT NULL CHECK (reason IN ('induce', 'accept', 'reject', 'rename', 'merge', 'propose')),
  actor text NOT NULL CHECK (actor IN ('engine', 'person')),
  doc jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(doc) = 'object'),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (drive_id, n)
);

-- How a field label that the dictionary does not know is read (a concept or a shared name), asked of the model once.
CREATE TABLE _oo_docs.structure_alignment (
  key text PRIMARY KEY CHECK (char_length(key) BETWEEN 1 AND 80),
  target text NOT NULL CHECK (target ~ '^[a-z_]{2,40}$'),
  source text NOT NULL CHECK (source IN ('model')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

GRANT SELECT, INSERT, UPDATE, DELETE ON _oo_docs.structure_alignment, _oo_docs.structure_observation, _oo_docs.structure_class,
  _oo_docs.structure_member, _oo_docs.structure_version TO oo_docs_runtime;
