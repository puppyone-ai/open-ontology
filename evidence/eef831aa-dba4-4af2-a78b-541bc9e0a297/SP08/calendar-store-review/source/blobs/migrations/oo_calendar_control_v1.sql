-- calendar-store-v1. The adapter hashes this file before execution.
-- Exact replay does not execute it again. A different hash or a partial
-- catalog is drift. This script does not create objects conditionally and
-- it does not repair an existing schema. It does not call a clock.

CREATE SCHEMA oo_calendar_control;

CREATE TABLE oo_calendar_control.install_manifest (
    singleton boolean NOT NULL,
    sql_sha256 text NOT NULL,
    CONSTRAINT install_manifest_pk PRIMARY KEY (singleton),
    CONSTRAINT install_manifest_one CHECK (singleton),
    CONSTRAINT install_manifest_sha CHECK (sql_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE TABLE oo_calendar_control.deployment (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    finance_approval boolean NOT NULL,
    authenticated boolean NOT NULL,
    ontology_published boolean NOT NULL,
    clock_used boolean NOT NULL,
    CONSTRAINT deployment_pk PRIMARY KEY (singleton),
    CONSTRAINT deployment_one CHECK (singleton),
    CONSTRAINT deployment_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT deployment_flags CHECK (
        finance_approval = FALSE
        AND authenticated = FALSE
        AND ontology_published = FALSE
        AND clock_used = FALSE
    )
);

CREATE TABLE oo_calendar_control.runtime_allow (
    role_name text NOT NULL,
    CONSTRAINT runtime_allow_pk PRIMARY KEY (role_name),
    CONSTRAINT runtime_allow_name CHECK (role_name ~ '^[a-z_][a-z0-9_]{0,62}$')
);

CREATE TABLE oo_calendar_control.snapshot (
    snapshot_id text NOT NULL,
    project_id text NOT NULL,
    calendar_version text NOT NULL,
    config_sha256 text NOT NULL,
    as_of text NOT NULL,
    closure_fingerprint text NOT NULL,
    resolved_sha256 text NOT NULL,
    calendar_yaml text NOT NULL,
    closure_date text,
    closure_label text,
    closure_record_sha256 text,
    report_json text NOT NULL,
    business_tz text NOT NULL,
    CONSTRAINT snapshot_pk PRIMARY KEY (snapshot_id),
    CONSTRAINT snapshot_id_hex CHECK (snapshot_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT snapshot_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT snapshot_version CHECK (calendar_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT snapshot_hashes CHECK (
        config_sha256 ~ '^[0-9a-f]{64}$'
        AND resolved_sha256 ~ '^[0-9a-f]{64}$'
        AND (closure_fingerprint = 'none' OR closure_fingerprint ~ '^[0-9a-f]{64}$')
    ),
    CONSTRAINT snapshot_as_of CHECK (octet_length(as_of) BETWEEN 1 AND 40),
    CONSTRAINT snapshot_yaml_bound CHECK (octet_length(calendar_yaml) BETWEEN 1 AND 196608),
    CONSTRAINT snapshot_json_bound CHECK (octet_length(report_json) BETWEEN 1 AND 524288),
    CONSTRAINT snapshot_tz CHECK (business_tz ~ '^[A-Za-z0-9_+-][A-Za-z0-9_+-/]{0,127}$'),
    CONSTRAINT snapshot_closure CHECK (
        (
            closure_date IS NULL
            AND closure_label IS NULL
            AND closure_record_sha256 IS NULL
            AND closure_fingerprint = 'none'
        )
        OR (
            closure_date IS NOT NULL
            AND closure_label IS NOT NULL
            AND closure_record_sha256 IS NOT NULL
            AND closure_fingerprint ~ '^[0-9a-f]{64}$'
        )
    ),
    CONSTRAINT snapshot_identity_uq UNIQUE (
        project_id, calendar_version, config_sha256, as_of, closure_fingerprint
    )
);

CREATE TABLE oo_calendar_control.published (
    snapshot_id text NOT NULL,
    project_id text NOT NULL,
    business_tz text NOT NULL,
    as_of timestamp with time zone NOT NULL,
    calendar_version text NOT NULL,
    today_date date,
    today_midnight_status text NOT NULL,
    today_midnight timestamp with time zone,
    month_start_date date,
    month_start_midnight_status text NOT NULL,
    month_start_midnight timestamp with time zone,
    prev_month_start_date date,
    prev_month_start_midnight_status text NOT NULL,
    prev_month_start_midnight timestamp with time zone,
    prev_month_end_date date,
    prev_month_end_midnight_status text NOT NULL,
    prev_month_end_midnight timestamp with time zone,
    quarter_start_date date,
    quarter_start_midnight_status text NOT NULL,
    quarter_start_midnight timestamp with time zone,
    prev_quarter_start_date date,
    prev_quarter_start_midnight_status text NOT NULL,
    prev_quarter_start_midnight timestamp with time zone,
    prev_quarter_end_date date,
    prev_quarter_end_midnight_status text NOT NULL,
    prev_quarter_end_midnight timestamp with time zone,
    fiscal_year_start_date date,
    fiscal_year_start_midnight_status text NOT NULL,
    fiscal_year_start_midnight timestamp with time zone,
    prev_fiscal_year_end_date date,
    prev_fiscal_year_end_midnight_status text NOT NULL,
    prev_fiscal_year_end_midnight timestamp with time zone,
    last_closed_period_end_date date,
    last_closed_period_end_midnight_status text NOT NULL,
    last_closed_period_end_midnight timestamp with time zone,
    CONSTRAINT published_pk PRIMARY KEY (snapshot_id),
    CONSTRAINT published_snapshot_fk FOREIGN KEY (snapshot_id)
        REFERENCES oo_calendar_control.snapshot (snapshot_id),
    CONSTRAINT published_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT published_version CHECK (calendar_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT published_tz CHECK (business_tz ~ '^[A-Za-z0-9_+-][A-Za-z0-9_+-/]{0,127}$'),
    CONSTRAINT published_today_status CHECK (today_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_today_shape CHECK (
        (today_midnight_status = 'absent' AND today_date IS NULL AND today_midnight IS NULL)
        OR (today_midnight_status = 'unique' AND today_date IS NOT NULL AND today_midnight IS NOT NULL)
        OR (today_midnight_status IN ('nonexistent', 'ambiguous') AND today_date IS NOT NULL AND today_midnight IS NULL)
    ),
    CONSTRAINT published_month_start_status CHECK (month_start_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_month_start_shape CHECK (
        (month_start_midnight_status = 'absent' AND month_start_date IS NULL AND month_start_midnight IS NULL)
        OR (month_start_midnight_status = 'unique' AND month_start_date IS NOT NULL AND month_start_midnight IS NOT NULL)
        OR (month_start_midnight_status IN ('nonexistent', 'ambiguous') AND month_start_date IS NOT NULL AND month_start_midnight IS NULL)
    ),
    CONSTRAINT published_prev_month_start_status CHECK (prev_month_start_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_prev_month_start_shape CHECK (
        (prev_month_start_midnight_status = 'absent' AND prev_month_start_date IS NULL AND prev_month_start_midnight IS NULL)
        OR (prev_month_start_midnight_status = 'unique' AND prev_month_start_date IS NOT NULL AND prev_month_start_midnight IS NOT NULL)
        OR (prev_month_start_midnight_status IN ('nonexistent', 'ambiguous') AND prev_month_start_date IS NOT NULL AND prev_month_start_midnight IS NULL)
    ),
    CONSTRAINT published_prev_month_end_status CHECK (prev_month_end_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_prev_month_end_shape CHECK (
        (prev_month_end_midnight_status = 'absent' AND prev_month_end_date IS NULL AND prev_month_end_midnight IS NULL)
        OR (prev_month_end_midnight_status = 'unique' AND prev_month_end_date IS NOT NULL AND prev_month_end_midnight IS NOT NULL)
        OR (prev_month_end_midnight_status IN ('nonexistent', 'ambiguous') AND prev_month_end_date IS NOT NULL AND prev_month_end_midnight IS NULL)
    ),
    CONSTRAINT published_quarter_start_status CHECK (quarter_start_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_quarter_start_shape CHECK (
        (quarter_start_midnight_status = 'absent' AND quarter_start_date IS NULL AND quarter_start_midnight IS NULL)
        OR (quarter_start_midnight_status = 'unique' AND quarter_start_date IS NOT NULL AND quarter_start_midnight IS NOT NULL)
        OR (quarter_start_midnight_status IN ('nonexistent', 'ambiguous') AND quarter_start_date IS NOT NULL AND quarter_start_midnight IS NULL)
    ),
    CONSTRAINT published_prev_quarter_start_status CHECK (prev_quarter_start_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_prev_quarter_start_shape CHECK (
        (prev_quarter_start_midnight_status = 'absent' AND prev_quarter_start_date IS NULL AND prev_quarter_start_midnight IS NULL)
        OR (prev_quarter_start_midnight_status = 'unique' AND prev_quarter_start_date IS NOT NULL AND prev_quarter_start_midnight IS NOT NULL)
        OR (prev_quarter_start_midnight_status IN ('nonexistent', 'ambiguous') AND prev_quarter_start_date IS NOT NULL AND prev_quarter_start_midnight IS NULL)
    ),
    CONSTRAINT published_prev_quarter_end_status CHECK (prev_quarter_end_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_prev_quarter_end_shape CHECK (
        (prev_quarter_end_midnight_status = 'absent' AND prev_quarter_end_date IS NULL AND prev_quarter_end_midnight IS NULL)
        OR (prev_quarter_end_midnight_status = 'unique' AND prev_quarter_end_date IS NOT NULL AND prev_quarter_end_midnight IS NOT NULL)
        OR (prev_quarter_end_midnight_status IN ('nonexistent', 'ambiguous') AND prev_quarter_end_date IS NOT NULL AND prev_quarter_end_midnight IS NULL)
    ),
    CONSTRAINT published_fiscal_year_start_status CHECK (fiscal_year_start_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_fiscal_year_start_shape CHECK (
        (fiscal_year_start_midnight_status = 'absent' AND fiscal_year_start_date IS NULL AND fiscal_year_start_midnight IS NULL)
        OR (fiscal_year_start_midnight_status = 'unique' AND fiscal_year_start_date IS NOT NULL AND fiscal_year_start_midnight IS NOT NULL)
        OR (fiscal_year_start_midnight_status IN ('nonexistent', 'ambiguous') AND fiscal_year_start_date IS NOT NULL AND fiscal_year_start_midnight IS NULL)
    ),
    CONSTRAINT published_prev_fiscal_year_end_status CHECK (prev_fiscal_year_end_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_prev_fiscal_year_end_shape CHECK (
        (prev_fiscal_year_end_midnight_status = 'absent' AND prev_fiscal_year_end_date IS NULL AND prev_fiscal_year_end_midnight IS NULL)
        OR (prev_fiscal_year_end_midnight_status = 'unique' AND prev_fiscal_year_end_date IS NOT NULL AND prev_fiscal_year_end_midnight IS NOT NULL)
        OR (prev_fiscal_year_end_midnight_status IN ('nonexistent', 'ambiguous') AND prev_fiscal_year_end_date IS NOT NULL AND prev_fiscal_year_end_midnight IS NULL)
    ),
    CONSTRAINT published_last_closed_period_end_status CHECK (last_closed_period_end_midnight_status IN ('absent', 'unique', 'nonexistent', 'ambiguous')),
    CONSTRAINT published_last_closed_period_end_shape CHECK (
        (last_closed_period_end_midnight_status = 'absent' AND last_closed_period_end_date IS NULL AND last_closed_period_end_midnight IS NULL)
        OR (last_closed_period_end_midnight_status = 'unique' AND last_closed_period_end_date IS NOT NULL AND last_closed_period_end_midnight IS NOT NULL)
        OR (last_closed_period_end_midnight_status IN ('nonexistent', 'ambiguous') AND last_closed_period_end_date IS NOT NULL AND last_closed_period_end_midnight IS NULL)
    )
);

CREATE TABLE oo_calendar_control.active_pointer (
    singleton boolean NOT NULL,
    project_id text NOT NULL,
    snapshot_id text,
    calendar_version text,
    as_of timestamp with time zone,
    CONSTRAINT active_pointer_pk PRIMARY KEY (singleton),
    CONSTRAINT active_pointer_one CHECK (singleton),
    CONSTRAINT active_pointer_project CHECK (project_id ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'),
    CONSTRAINT active_pointer_pair CHECK (
        (snapshot_id IS NULL AND calendar_version IS NULL AND as_of IS NULL)
        OR (snapshot_id IS NOT NULL AND calendar_version IS NOT NULL AND as_of IS NOT NULL)
    ),
    CONSTRAINT active_pointer_snapshot CHECK (snapshot_id IS NULL OR snapshot_id ~ '^[0-9a-f]{64}$'),
    CONSTRAINT active_pointer_version CHECK (
        calendar_version IS NULL OR calendar_version ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'
    )
);

REVOKE ALL ON SCHEMA oo_calendar_control FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.install_manifest FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.deployment FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.runtime_allow FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.snapshot FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.published FROM PUBLIC;
REVOKE ALL ON TABLE oo_calendar_control.active_pointer FROM PUBLIC;

GRANT USAGE ON SCHEMA oo_calendar_control TO oo_calendar_view;
GRANT SELECT ON TABLE oo_calendar_control.published TO oo_calendar_view;
GRANT SELECT (singleton, project_id, snapshot_id, calendar_version, as_of)
    ON TABLE oo_calendar_control.active_pointer TO oo_calendar_view;

CREATE SCHEMA oo_dict;
REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA oo_dict TO oo_calendar_view;

SET LOCAL ROLE oo_calendar_view;
CREATE VIEW oo_dict.calendar WITH (security_invoker = FALSE, security_barrier = TRUE) AS
SELECT p.project_id,
       p.business_tz,
       p.as_of,
       p.calendar_version,
       p.today_date,
       p.today_midnight_status,
       p.today_midnight,
       p.month_start_date,
       p.month_start_midnight_status,
       p.month_start_midnight,
       p.prev_month_start_date,
       p.prev_month_start_midnight_status,
       p.prev_month_start_midnight,
       p.prev_month_end_date,
       p.prev_month_end_midnight_status,
       p.prev_month_end_midnight,
       p.quarter_start_date,
       p.quarter_start_midnight_status,
       p.quarter_start_midnight,
       p.prev_quarter_start_date,
       p.prev_quarter_start_midnight_status,
       p.prev_quarter_start_midnight,
       p.prev_quarter_end_date,
       p.prev_quarter_end_midnight_status,
       p.prev_quarter_end_midnight,
       p.fiscal_year_start_date,
       p.fiscal_year_start_midnight_status,
       p.fiscal_year_start_midnight,
       p.prev_fiscal_year_end_date,
       p.prev_fiscal_year_end_midnight_status,
       p.prev_fiscal_year_end_midnight,
       p.last_closed_period_end_date,
       p.last_closed_period_end_midnight_status,
       p.last_closed_period_end_midnight
FROM oo_calendar_control.published p
JOIN oo_calendar_control.active_pointer a
  ON a.singleton
 AND a.snapshot_id = p.snapshot_id
 AND a.project_id = p.project_id;
RESET ROLE;

REVOKE CREATE ON SCHEMA oo_dict FROM oo_calendar_view;
REVOKE ALL ON TABLE oo_dict.calendar FROM PUBLIC;
