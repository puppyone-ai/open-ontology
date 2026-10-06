-- enum-store-v1. The adapter hashes this file before execution.
-- Exact replay does not execute it again. A different hash or a partial
-- catalog is drift. This script does not create objects conditionally and
-- it does not repair an existing schema. It does not call a clock.

CREATE SCHEMA oo_enum_control;

CREATE TABLE oo_enum_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_enum_control.deployment (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    finance_certified boolean NOT NULL,
    semantic_certified boolean NOT NULL,
    owner_authenticated boolean NOT NULL,
    metric_certified boolean NOT NULL,
    clock_used boolean NOT NULL,
    CONSTRAINT deployment_pk PRIMARY KEY (singleton),
    CONSTRAINT deployment_one CHECK (singleton),
    CONSTRAINT deployment_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT deployment_flags CHECK (
        finance_certified = FALSE
        AND semantic_certified = FALSE
        AND owner_authenticated = FALSE
        AND metric_certified = FALSE
        AND clock_used = FALSE
    )
);

CREATE TABLE oo_enum_control.runtime_allow (
    role_name text NOT NULL,
    CONSTRAINT runtime_allow_pk PRIMARY KEY (role_name),
    CONSTRAINT runtime_allow_name CHECK (role_name ~ '^[a-z_][a-z0-9_]{0,62}$')
);

CREATE TABLE oo_enum_control.snapshot (
    snapshot_id text NOT NULL,
    project_id text NOT NULL,
    enum_version text NOT NULL,
    config_sha256 text NOT NULL,
    as_of date NOT NULL,
    observations_sha256 text NOT NULL,
    threshold_spelling text NOT NULL,
    report_sha256 text NOT NULL,
    enum_yaml text NOT NULL,
    observations_json text NOT NULL,
    report_json text NOT NULL,
    enum_id text NOT NULL,
    observed_defined boolean NOT NULL,
    coverage_numerator text,
    coverage_denominator text,
    coverage_ratio text,
    threshold_decimal text NOT NULL,
    threshold_numerator text NOT NULL,
    threshold_denominator text NOT NULL,
    threshold_defaulted boolean NOT NULL,
    threshold_met boolean,
    comparison text,
    review_needed boolean NOT NULL,
    warning_count integer NOT NULL,
    published_count integer NOT NULL,
    declared_coverage text,
    declared_owner text,
    CONSTRAINT snapshot_pk PRIMARY KEY (snapshot_id),
    CONSTRAINT snapshot_id_hex CHECK (snapshot_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT snapshot_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT snapshot_version CHECK (enum_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT snapshot_hashes CHECK (
        config_sha256 ~ '^[0-9a-f]{64}$'
        AND observations_sha256 ~ '^[0-9a-f]{64}$'
        AND report_sha256 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT snapshot_as_of CHECK (to_char(as_of, 'YYYY-MM-DD') ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'),
    CONSTRAINT snapshot_yaml_bound CHECK (octet_length(enum_yaml) BETWEEN 1 AND 2097152),
    CONSTRAINT snapshot_json_bound CHECK (octet_length(report_json) BETWEEN 2 AND 2097152),
    CONSTRAINT snapshot_obs_bound CHECK (octet_length(observations_json) BETWEEN 2 AND 2097152),
    CONSTRAINT snapshot_enum_id CHECK (octet_length(enum_id) BETWEEN 1 AND 1024),
    CONSTRAINT snapshot_threshold CHECK (
        octet_length(threshold_spelling) BETWEEN 1 AND 64
        AND octet_length(threshold_decimal) BETWEEN 1 AND 128
        AND threshold_numerator ~ '^[0-9]+$'
        AND threshold_denominator ~ '^[0-9]+$'
        AND octet_length(threshold_numerator) BETWEEN 1 AND 128
        AND octet_length(threshold_denominator) BETWEEN 1 AND 128
        AND warning_count >= 0
        AND published_count >= 0
    ),
    CONSTRAINT snapshot_coverage CHECK (
        (
            observed_defined = FALSE
            AND coverage_numerator IS NULL
            AND coverage_denominator IS NULL
            AND coverage_ratio IS NULL
            AND threshold_met IS NULL
            AND comparison IS NULL
        )
        OR (
            observed_defined = TRUE
            AND coverage_numerator IS NOT NULL
            AND coverage_denominator IS NOT NULL
            AND coverage_ratio IS NOT NULL
            AND threshold_met IS NOT NULL
            AND comparison IS NOT NULL
            AND coverage_numerator ~ '^[0-9]+$'
            AND coverage_denominator ~ '^[0-9]+$'
            AND octet_length(coverage_ratio) BETWEEN 1 AND 128
            AND comparison IN ('below', 'equal', 'above')
        )
    ),
    CONSTRAINT snapshot_declared CHECK (
        (declared_coverage IS NULL OR octet_length(declared_coverage) BETWEEN 1 AND 128)
        AND (declared_owner IS NULL OR octet_length(declared_owner) BETWEEN 1 AND 256)
    ),
    CONSTRAINT snapshot_identity_uq UNIQUE (
        project_id, enum_version, config_sha256, as_of, observations_sha256, threshold_spelling
    )
);

CREATE TABLE oo_enum_control.notice (
    snapshot_id text NOT NULL,
    notice_no integer NOT NULL,
    code text NOT NULL,
    kind text NOT NULL,
    reason text NOT NULL,
    count_text text NOT NULL,
    source_index integer,
    CONSTRAINT notice_pk PRIMARY KEY (snapshot_id, notice_no),
    CONSTRAINT notice_snapshot_fk FOREIGN KEY (snapshot_id)
        REFERENCES oo_enum_control.snapshot (snapshot_id),
    CONSTRAINT notice_no CHECK (notice_no > 0 AND (source_index IS NULL OR source_index >= 0)),
    CONSTRAINT notice_code CHECK (octet_length(code) BETWEEN 1 AND 2048),
    CONSTRAINT notice_count CHECK (count_text ~ '^[0-9]+$' AND octet_length(count_text) BETWEEN 1 AND 20),
    CONSTRAINT notice_kind CHECK (kind IN (
        'undetermined', 'unexplained', 'expired_no_active', 'historical_unapproved', 'class_not_business'
    )),
    CONSTRAINT notice_reason CHECK (
        (kind = 'undetermined' AND reason = 'undetermined')
        OR (kind = 'unexplained' AND reason IN ('unexplained', 'unknown_code'))
        OR (kind = 'expired_no_active' AND reason = 'no_active')
        OR (kind = 'historical_unapproved' AND reason = 'historical_unapproved')
        OR (kind = 'class_not_business' AND reason = 'class_not_business')
    )
);

CREATE TABLE oo_enum_control.published (
    snapshot_id text NOT NULL,
    line_no integer NOT NULL,
    row_kind text NOT NULL,
    enum_id text NOT NULL,
    code text NOT NULL,
    description text,
    class text,
    valid_from date,
    valid_to date,
    version text NOT NULL,
    as_of date NOT NULL,
    project_id text NOT NULL,
    status text NOT NULL,
    CONSTRAINT published_pk PRIMARY KEY (snapshot_id, line_no),
    CONSTRAINT published_snapshot_fk FOREIGN KEY (snapshot_id)
        REFERENCES oo_enum_control.snapshot (snapshot_id),
    CONSTRAINT published_line CHECK (line_no > 0),
    CONSTRAINT published_kind CHECK (row_kind IN ('definition', 'unknown_code')),
    CONSTRAINT published_enum_id CHECK (octet_length(enum_id) BETWEEN 1 AND 1024),
    CONSTRAINT published_code CHECK (octet_length(code) BETWEEN 1 AND 2048),
    CONSTRAINT published_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT published_version CHECK (version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT published_status CHECK (status IN (
        'approved', 'undetermined', 'unexplained', 'superseded', 'deprecated'
    )),
    CONSTRAINT published_unknown_shape CHECK (
        (
            row_kind = 'definition'
            AND (description IS NULL OR octet_length(description) BETWEEN 1 AND 2048)
            AND (class IS NULL OR octet_length(class) BETWEEN 1 AND 1024)
        )
        OR (
            row_kind = 'unknown_code'
            AND status = 'unexplained'
            AND class IS NULL
            AND description IS NULL
            AND valid_from IS NULL
            AND valid_to IS NULL
        )
    )
);

CREATE TABLE oo_enum_control.served (
    line_no integer NOT NULL,
    snapshot_id text NOT NULL,
    enum_id text NOT NULL,
    code text NOT NULL,
    description text,
    class text,
    valid_from date,
    valid_to date,
    version text NOT NULL,
    as_of date NOT NULL,
    project_id text NOT NULL,
    status text NOT NULL,
    CONSTRAINT served_pk PRIMARY KEY (line_no),
    CONSTRAINT served_published_fk FOREIGN KEY (snapshot_id, line_no)
        REFERENCES oo_enum_control.published (snapshot_id, line_no),
    CONSTRAINT served_snapshot CHECK (snapshot_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT served_enum_id CHECK (octet_length(enum_id) BETWEEN 1 AND 1024),
    CONSTRAINT served_code CHECK (octet_length(code) BETWEEN 1 AND 2048 AND line_no > 0),
    CONSTRAINT served_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT served_version CHECK (version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT served_status CHECK (status IN (
        'approved', 'undetermined', 'unexplained', 'superseded', 'deprecated'
    ))
);

CREATE TABLE oo_enum_control.active_pointer (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    snapshot_id text,
    enum_version text,
    as_of date,
    CONSTRAINT active_pointer_pk PRIMARY KEY (singleton),
    CONSTRAINT active_pointer_one CHECK (singleton),
    CONSTRAINT active_pointer_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT active_pointer_pair CHECK (
        (snapshot_id IS NULL AND enum_version IS NULL AND as_of IS NULL)
        OR (snapshot_id IS NOT NULL AND enum_version IS NOT NULL AND as_of IS NOT NULL)
    ),
    CONSTRAINT active_pointer_snapshot CHECK (snapshot_id IS NULL OR snapshot_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT active_pointer_version CHECK (
        enum_version IS NULL OR enum_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'
    )
);

REVOKE ALL ON SCHEMA oo_enum_control FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.install_manifest FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.deployment FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.runtime_allow FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.snapshot FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.notice FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.published FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.served FROM PUBLIC;
REVOKE ALL ON TABLE oo_enum_control.active_pointer FROM PUBLIC;

GRANT USAGE ON SCHEMA oo_enum_control TO oo_enum_view;
GRANT SELECT ON TABLE oo_enum_control.served TO oo_enum_view;

CREATE SCHEMA oo_dict;
REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_enum_view;

SET LOCAL ROLE oo_enum_view;
CREATE VIEW oo_dict.enums WITH (security_invoker = FALSE, security_barrier = TRUE) AS
SELECT enum_id,
       code,
       description,
       class,
       valid_from,
       valid_to,
       version,
       as_of,
       project_id,
       status
FROM oo_enum_control.served;
REVOKE ALL ON TABLE oo_dict.enums FROM PUBLIC;
RESET ROLE;

REVOKE CREATE ON SCHEMA oo_dict FROM oo_enum_view;
