"""Install the unchanged product migrations into a new owned fixture only."""
import hashlib
from pathlib import Path

PRODUCT = Path(__file__).resolve().parents[3]


def setup_sql(database):
    assert database.startswith("oo_business_guard_") and database.replace("_", "").isalnum()
    pipeline = PRODUCT / "crates/oo-pipeline/migrations"
    jobs = PRODUCT / "crates/oo-jobs/migrations"
    sql = [(pipeline / "0000_bootstrap_roles.sql").read_text(),
           (jobs / "0000_bootstrap_roles.sql").read_text(),
           "SET ROLE oo_jobs_owner;"]
    for path in sorted(jobs.glob("*.sql")):
        if 1 <= int(path.name[:4]) <= 3:
            sql += ["BEGIN;", path.read_text(), "COMMIT;"]
            version = {1: "jobs-control/1", 2: "jobs-event-project-binding/1",
                       3: "jobs-lease-operations/1"}[int(path.name[:4])]
            sql.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES("
                       f"'{version}','{hashlib.sha256(path.read_bytes()).hexdigest()}','expand','{path.name}');")
    sql += ["RESET ROLE;", "GRANT USAGE ON SCHEMA jobs TO oo_target_owner,oo_pipeline_runtime;",
            "GRANT SELECT ON jobs.tasks TO oo_target_owner;",
            "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE;",
            "SET ROLE oo_target_owner;"]
    for path in sorted(pipeline.glob("*.sql")):
        number = int(path.name[:4])
        if 1 <= number <= 82:
            if number == 44:
                sql.append("RESET ROLE;")
            sql += ["BEGIN;", path.read_text(), "COMMIT;"]
            if number == 44:
                sql.append("SET ROLE oo_target_owner;")
            sql.append("INSERT INTO _oo.schema_migrations(version,checksum,phase,filename) VALUES("
                       f"'owned-native-test/{number}','{hashlib.sha256(path.read_bytes()).hexdigest()}',"
                       f"'expand','{path.name}');")
    sql += ["RESET ROLE;",
            f"GRANT CONNECT ON DATABASE {database} TO oo_release_admin,biz_executor,biz_oracle,biz_evaluator,biz_reviewer;",
            "GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(bigint) TO oo_release_admin;",
            "GRANT USAGE ON SCHEMA _oo TO oo_release_admin,oo_r_clerk,biz_oracle;",
            "GRANT SELECT ON _oo.schema_migrations,_oo.native_publication_receipt TO oo_release_admin;",
            "GRANT SELECT,UPDATE ON _oo.full_import_run,_oo.full_import_table,_oo.table_freshness,"
            "_oo.source_watermark,_oo.physical_mappings TO oo_release_admin;",
            "GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text,text,bigint,text,text,text,text) TO oo_release_admin;",
            (Path(__file__).parent / "fixtures/business_native_load.sql").read_text()]
    return "\n".join(sql)
