#!/usr/bin/env python3
"""Independent physical-mapping probe.

Runs after the unchanged oo-pipeline live test. One fresh database on the
same private PostgreSQL. Digests come from hashlib, not from the crate.
"""

import hashlib
import pathlib
import re
import subprocess
import sys

SNAPSHOT = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2])
PG = sys.argv[3]
OUT.mkdir(parents=True, exist_ok=True)

MIGRATION = SNAPSHOT / "crates" / "oo-pipeline" / "migrations"
FROZEN = {
    "0000_bootstrap_roles.sql": "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208",
    "0001_target_fence_expand.sql": "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9",
    "0002_target_segment_null_expand.sql": "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512",
    "0003_target_segment_preflight_expand.sql": "d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3",
    "0004_physical_mapping.sql": "844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95",
}
LIE = "e" * 64
CONFIG = "ef" * 32
SOURCE = "src_review"
GENERATION = 4
DB = "oo_pm_review"

fails = []
lines = []


def log(text):
    print(text, flush=True)
    lines.append(text)


def finish(code):
    (OUT / "transcript.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (OUT / "probe.exit").write_text(f"{code}\n", encoding="utf-8")
    sys.exit(code)


def check(name, cond, detail=""):
    status = "PASS" if cond else "FAIL"
    log(f"{status} {name} {detail}".rstrip())
    if not cond:
        fails.append(name)
    return cond


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def payload(source, config, generation, level, namespace, table, column):
    out = bytearray()

    def cstr(text):
        out.extend(text.encode("utf-8"))
        out.append(0)

    cstr("physical-mapping/v1")
    cstr(source)
    cstr(config)
    cstr(str(generation))
    out.append(ord(level))
    cstr(namespace)
    cstr(table)
    cstr(column)
    return bytes(out)


def physical(level, digest_hex):
    prefix = {"n": "pn_", "t": "pt_", "c": "pc_"}[level]
    name = prefix + digest_hex[:60]
    if len(name) != 63:
        raise SystemExit(f"physical length {len(name)}")
    return name


def psql(database, sql, user="postgres"):
    proc = subprocess.run(
        [
            "docker",
            "exec",
            "-i",
            PG,
            "psql",
            "-U",
            user,
            "-d",
            database,
            "-h",
            "127.0.0.1",
            "-v",
            "ON_ERROR_STOP=1",
            "-v",
            "VERBOSITY=verbose",
            "-At",
            "--no-psqlrc",
        ],
        input=sql.encode("utf-8"),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return proc.returncode, proc.stdout.decode("utf-8", "replace"), proc.stderr.decode("utf-8", "replace")


def notice_state(database, body, user="postgres", wrap=True):
    inner = f"""
DO $probe$
DECLARE
  v_state text;
BEGIN
  BEGIN
    {body}
    v_state := 'OK';
  EXCEPTION WHEN OTHERS THEN
    v_state := SQLSTATE;
  END;
  RAISE NOTICE 'sqlstate=%', v_state;
END
$probe$;
"""
    sql = inner if not wrap else inner
    code, out, err = psql(database, sql, user)
    match = re.search(r"sqlstate=([0-9A-Z]+)", err + out)
    state = match.group(1) if match else ""
    return code, state, out, err


def sql_quote(text):
    if not re.fullmatch(r"[A-Za-z0-9_]*", text) and not re.fullmatch(r"q_[0-9a-f]*", text):
        raise SystemExit(f"refusing to quote {text!r}")
    return "'" + text + "'"


file_bytes = {}
for name, expect in FROZEN.items():
    data = (MIGRATION / name).read_bytes()
    file_bytes[name] = data
    got = sha256(data)
    check(f"frozen-{name}", got == expect, got)

left = "q_" + ("ab" * 40)
right = left[:-2] + "cd"
check("long-prefix", left.encode()[:63] == right.encode()[:63] and left != right and len(left) == 82)

left_payload = payload(SOURCE, CONFIG, GENERATION, "n", left, "", "")
right_payload = payload(SOURCE, CONFIG, GENERATION, "n", right, "", "")
golden_payload = payload(
    "src_main",
    "ab" * 32,
    1,
    "n",
    "customer",
    "",
    "",
)
left_digest = sha256(left_payload)
right_digest = sha256(right_payload)
golden_digest = sha256(golden_payload)
left_name = physical("n", left_digest)
right_name = physical("n", right_digest)
oracle = "\n".join(
    [
        f"empty={sha256(b'')}",
        f"abc={sha256(b'abc')}",
        f"a200={sha256(b'a' * 200)}",
        f"golden_payload_len={len(golden_payload)}",
        f"golden={golden_digest}",
        f"left_logical={left}",
        f"left_logical_len={len(left)}",
        f"left_digest={left_digest}",
        f"left_physical={left_name}",
        f"right_logical={right}",
        f"right_digest={right_digest}",
        f"right_physical={right_name}",
        f"distinct_physical={str(left_name != right_name).lower()}",
    ]
)
(OUT / "python-oracles.txt").write_text(oracle + "\n", encoding="utf-8")
(OUT / "left-payload.bin").write_bytes(left_payload)
(OUT / "golden-payload.bin").write_bytes(golden_payload)
log(oracle)

code, out, err = psql(
    "postgres",
    "CREATE DATABASE oo_pm_review WITH TEMPLATE template0 ENCODING 'UTF8' "
    "LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';",
)
(OUT / "create-database.stderr").write_text(err, encoding="utf-8")
if not check("create-database", code == 0, err.strip()):
    finish(1)

code, out, err = psql(
    DB,
    "SELECT pg_catalog.pg_encoding_to_char(encoding) || '|' || datlocprovider::text || '|' || "
    "coalesce(datlocale, '') FROM pg_catalog.pg_database WHERE datname = current_database();",
)
check("locale", code == 0 and out.strip() == "UTF8|b|C.UTF-8", out.strip() or err.strip())

code, out, err = psql("postgres", "SHOW server_version;")
check("server-version", code == 0 and out.strip().startswith("17.10"), out.strip())
(OUT / "server-version.txt").write_text(out, encoding="utf-8")

bootstrap = (MIGRATION / "0000_bootstrap_roles.sql").read_text(encoding="utf-8")
remainder_at = bootstrap.index("ALTER ROLE oo_target_owner SET search_path = pg_catalog;")
remainder = bootstrap[remainder_at:]
if "CREATE ROLE" in remainder:
    check("bootstrap-remainder-has-no-create-role", False)
    finish(1)
check("bootstrap-remainder-has-no-create-role", True)

hash_0003 = sha256(file_bytes["0003_target_segment_preflight_expand.sql"])
hash_0004 = sha256(file_bytes["0004_physical_mapping.sql"])
install = "\n".join(
    [
        "BEGIN;",
        "SET LOCAL lock_timeout = '15s';",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202605);",
        remainder,
        "SET ROLE oo_target_owner;",
        file_bytes["0001_target_fence_expand.sql"].decode("utf-8"),
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;",
        file_bytes["0003_target_segment_preflight_expand.sql"].decode("utf-8"),
        file_bytes["0004_physical_mapping.sql"].decode("utf-8"),
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES "
        f"('target-fence-receipts/3', '{hash_0003}', 'expand', '0003_target_segment_preflight_expand.sql');",
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES "
        f"('physical-mapping/v1', '{hash_0004}', 'expand', '0004_physical_mapping.sql');",
        "RESET ROLE;",
        "COMMIT;",
    ]
)
(OUT / "install.sql").write_text(install, encoding="utf-8")
code, out, err = psql(DB, install)
(OUT / "install.stdout").write_text(out, encoding="utf-8")
(OUT / "install.stderr").write_text(err, encoding="utf-8")
if not check("install-0001-0003-0004", code == 0, err[-500:]):
    finish(1)

code, out, err = psql(
    DB,
    "SELECT version || '|' || checksum || '|' || phase || '|' || filename || '|' || applied_at::text "
    "FROM _oo.schema_migrations ORDER BY version;",
)
(OUT / "markers-after-install.txt").write_text(out, encoding="utf-8")
check("marker-0004", code == 0 and f"physical-mapping/v1|{hash_0004}|expand|0004_physical_mapping.sql|" in out, out.strip())
check("marker-0003", code == 0 and f"target-fence-receipts/3|{hash_0003}|expand|" in out)
applied_at = ""
for row in out.splitlines():
    if row.startswith("physical-mapping/v1|"):
        applied_at = row.split("|", 4)[4]
code, out, err = psql(
    DB,
    "SELECT c.oid::text FROM pg_catalog.pg_class AS c "
    "JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace "
    "WHERE n.nspname = '_oo' AND c.relname = 'physical_mappings';",
)
table_oid = out.strip()
check("table-oid", code == 0 and table_oid.isdigit(), table_oid)

def one(sql, user="postgres"):
    code, out, err = psql(DB, sql, user)
    return code, out.strip(), err


code, empty, err = one("SELECT encode(_oo.pm_sha256(decode('', 'hex')), 'hex');")
code2, abc, err2 = one("SELECT encode(_oo.pm_sha256(convert_to('abc', 'UTF8')), 'hex');")
code3, many, err3 = one("SELECT encode(_oo.pm_sha256(convert_to(repeat('a', 200), 'UTF8')), 'hex');")
check("sql-sha-empty", code == 0 and empty == sha256(b""), empty or err)
check("sql-sha-abc", code2 == 0 and abc == sha256(b"abc"), abc or err2)
check("sql-sha-a200", code3 == 0 and many == sha256(b"a" * 200), many or err3)

code, golden_sql, err = one(
    "SELECT encode(_oo.pm_sha256(_oo.pm_payload("
    "'src_main', 'abababababababababababababababababababababababababababababababab', "
    "1, 'n', 'customer', '', '')), 'hex');"
)
check("sql-golden-payload", code == 0 and golden_sql == golden_digest, golden_sql or err)
code, raw_golden, err = one(
    f"SELECT encode(_oo.pm_sha256(decode('{golden_payload.hex()}', 'hex')), 'hex');"
)
check("sql-golden-raw-bytes", code == 0 and raw_golden == golden_digest, raw_golden or err)
code, left_sql, err = one(
    "SELECT encode(_oo.pm_sha256(_oo.pm_payload("
    f"{sql_quote(SOURCE)}, {sql_quote(CONFIG)}, {GENERATION}, 'n', {sql_quote(left)}, '', '')), 'hex');"
)
check("sql-left-payload", code == 0 and left_sql == left_digest, left_sql or err)
code, left_raw, err = one(
    f"SELECT encode(_oo.pm_sha256(decode('{left_payload.hex()}', 'hex')), 'hex');"
)
check("sql-left-raw-bytes", code == 0 and left_raw == left_digest, left_raw or err)

roles = """
CREATE ROLE oo_a142_runtime LOGIN INHERIT;
CREATE ROLE oo_a142_agent LOGIN;
GRANT oo_pipeline_runtime TO oo_a142_runtime WITH INHERIT TRUE, SET FALSE;
GRANT CONNECT ON DATABASE oo_pm_review TO oo_a142_runtime;
GRANT CONNECT ON DATABASE oo_pm_review TO oo_a142_agent;
ALTER ROLE oo_a142_runtime SET search_path = pg_catalog;
ALTER ROLE oo_a142_agent SET search_path = pg_catalog;
"""
code, out, err = psql(DB, roles)
check("roles", code == 0, err.strip())

register_sql = (
    "SELECT _oo.register_physical_mapping("
    f"{sql_quote(SOURCE)}, {sql_quote(CONFIG)}, {GENERATION}, 'namespace', "
    f"{sql_quote(left)}, '', '', decode('{left_digest}', 'hex'));"
)
lookup_sql = (
    "SELECT _oo.lookup_physical_mapping("
    f"{sql_quote(SOURCE)}, {sql_quote(CONFIG)}, {GENERATION}, 'namespace', "
    f"{sql_quote(left)}, '', '');"
)
code, out, err = psql(
    DB,
    "BEGIN;\n"
    + register_sql
    + "\nSELECT coalesce(current_setting('_oo.physical_mapping_writer', true), '') "
    "|| '|' || coalesce(current_setting('_oo.writer', true), '');\nCOMMIT;\n",
    "oo_a142_runtime",
)
# psql -At still prints BEGIN/COMMIT command tags. The two data rows are the register result and the cleared GUC pair.
rows = [row for row in out.splitlines() if row and row not in {"BEGIN", "COMMIT", "ROLLBACK"}]
check("register-inserted", code == 0 and rows[:1] == ["inserted"], out.strip() or err)
check("guc-cleared", code == 0 and len(rows) >= 2 and rows[1] == "|", rows[1] if len(rows) > 1 else err)
code, first_stamp, err = one(
    "SELECT registered_at::text || '|' || physical_name || '|' || encode(identity_digest, 'hex') "
    "|| '|' || octet_length(physical_name)::text || '|' || octet_length(logical_namespace)::text "
    "|| '|' || logical_namespace "
    "FROM _oo.physical_mappings "
    f"WHERE source = {sql_quote(SOURCE)} AND config_digest = {sql_quote(CONFIG)} "
    f"AND run_generation = {GENERATION} AND level = 'n' AND logical_namespace = {sql_quote(left)} "
    "AND logical_table = '' AND logical_column = '';"
)
check(
    "stored-row",
    code == 0 and first_stamp.endswith(f"|{left_name}|{left_digest}|63|{len(left)}|{left}"),
    first_stamp or err,
)
code, replay, err = one(register_sql, "oo_a142_runtime")
check("replay", code == 0 and replay == "replay", replay or err)
code, second_stamp, err = one(
    "SELECT registered_at::text FROM _oo.physical_mappings "
    f"WHERE source = {sql_quote(SOURCE)} AND logical_namespace = {sql_quote(left)};"
)
check(
    "replay-stamp",
    code == 0 and first_stamp.startswith(second_stamp + "|"),
    second_stamp,
)
code, looked, err = one(lookup_sql, "oo_a142_runtime")
check("lookup-stored", code == 0 and looked == left_name, looked or err)

right_sql = (
    "SELECT _oo.register_physical_mapping("
    f"{sql_quote(SOURCE)}, {sql_quote(CONFIG)}, {GENERATION}, 'namespace', "
    f"{sql_quote(right)}, '', '', decode('{right_digest}', 'hex'));"
)
code, out, err = one(right_sql, "oo_a142_runtime")
check("right-inserted", code == 0 and out == "inserted", out or err)
code, pair, err = one(
    "SELECT logical_namespace || '|' || physical_name || '|' || octet_length(logical_namespace)::text "
    "|| '|' || octet_length(physical_name)::text FROM _oo.physical_mappings "
    f"WHERE source = {sql_quote(SOURCE)} ORDER BY logical_namespace;"
)
check("long-pair-stored", code == 0 and left_name in pair and right_name in pair and left_name != right_name, pair)
(OUT / "stored-rows.txt").write_text(pair + "\n", encoding="utf-8")

code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_agent'; EXECUTE 'SELECT count(*) FROM _oo.physical_mappings';",
)
check("agent-select", code == 0 and state == "42501", state or err)
agent_lookup = (
    "EXECUTE 'SET LOCAL ROLE oo_a142_agent';\n"
    "EXECUTE $agent$" + lookup_sql + "$agent$;"
)
code, state, out, err = notice_state(DB, agent_lookup)
check("agent-lookup", code == 0 and state == "42501", state or err)
agent_register = (
    "EXECUTE 'SET LOCAL ROLE oo_a142_agent';\n"
    "EXECUTE $agent$" + register_sql + "$agent$;"
)
code, state, out, err = notice_state(DB, agent_register)
check("agent-register", code == 0 and state == "42501", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_runtime'; EXECUTE 'SELECT count(*) FROM _oo.physical_mappings';",
)
check("runtime-select", code == 0 and state == "42501", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_runtime'; "
    "EXECUTE 'SELECT encode(_oo.pm_sha256(decode($$ $$, $$hex$$)), $$hex$$)';",
)
check("runtime-sha", code == 0 and state == "42501", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_runtime'; "
    "EXECUTE 'SELECT pg_catalog.set_config($$_oo.physical_mapping_writer$$, $$register$$, true)';",
)
check("runtime-set-config", code == 0 and state == "42501", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_runtime'; "
    "EXECUTE $set$SET LOCAL \"_oo.physical_mapping_writer\" = 'register'$set$; "
    "EXECUTE 'INSERT INTO _oo.physical_mappings DEFAULT VALUES';",
)
check("runtime-set-insert", code == 0 and state == "42501", state or err)

code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_target_owner'; "
    "EXECUTE 'INSERT INTO _oo.physical_mappings DEFAULT VALUES';",
)
check("owner-insert-without-guc", code == 0 and state == "OO014", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_target_owner'; "
    "EXECUTE 'SELECT pg_catalog.set_config($$_oo.physical_mapping_writer$$, $$register$$, true)'; "
    "EXECUTE 'UPDATE _oo.physical_mappings SET algorithm = algorithm';",
)
check("owner-update-with-guc", code == 0 and state == "OO014", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_target_owner'; "
    "EXECUTE 'SELECT pg_catalog.set_config($$_oo.physical_mapping_writer$$, $$register$$, true)'; "
    "EXECUTE 'DELETE FROM _oo.physical_mappings';",
)
check("owner-delete-with-guc", code == 0 and state == "OO014", state or err)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_target_owner'; "
    "EXECUTE 'SELECT pg_catalog.set_config($$_oo.physical_mapping_writer$$, $$register$$, true)'; "
    "EXECUTE 'TRUNCATE _oo.physical_mappings';",
)
check("owner-truncate-with-guc", code == 0 and state == "OO014", state or err)

lie_digest = "11" * 32
lie_physical = "pn_" + lie_digest[:60]
lie_sql = f"""
EXECUTE 'SET LOCAL ROLE oo_target_owner';
EXECUTE 'SELECT pg_catalog.set_config($$_oo.physical_mapping_writer$$, $$register$$, true)';
EXECUTE $lie$INSERT INTO _oo.physical_mappings (
  source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column,
  identity_digest, physical_name, scope_namespace_physical, scope_table_physical, algorithm
) VALUES (
  'src_review', '{CONFIG}', 9, 'n', 'review_lie', '', '',
  decode('{lie_digest}', 'hex'), '{lie_physical}', '{lie_physical}', NULL, 'physical-mapping/v1'
)$lie$;
"""
code, out, err = psql(
    DB,
    "BEGIN;\n"
    + """
DO $probe$
DECLARE
  v_state text;
BEGIN
  BEGIN
"""
    + lie_sql
    + """
    v_state := 'OK';
  EXCEPTION WHEN OTHERS THEN
    v_state := SQLSTATE;
  END;
  RAISE NOTICE 'sqlstate=%', v_state;
END
$probe$;
ROLLBACK;
""",
)
state = ""
match = re.search(r"sqlstate=([0-9A-Z]+)", err + out)
if match:
    state = match.group(1)
(OUT / "owner-guc-lie.stderr").write_text(err, encoding="utf-8")
log(f"NOTE owner_guc_digest_lie state={state or 'OK'} exit={code}")
(OUT / "owner-guc-lie.txt").write_text(f"state={state or 'OK'}\nexit={code}\n", encoding="utf-8")
code, lie_count, err = one(
    "SELECT count(*)::text FROM _oo.physical_mappings WHERE logical_namespace = 'review_lie';"
)
check("owner-lie-not-committed", code == 0 and lie_count == "0", lie_count or err)

code, looked, err = one(lookup_sql, "oo_a142_runtime")
check("lookup-after-denials", code == 0 and looked == left_name, looked or err)

code, constraints, err = one(
    "SELECT c.conname || ' ' || pg_catalog.pg_get_constraintdef(c.oid) "
    "FROM pg_catalog.pg_constraint AS c "
    "WHERE c.conrelid = '_oo.physical_mappings'::regclass ORDER BY 1;"
)
(OUT / "constraints.txt").write_text(constraints + "\n", encoding="utf-8")
check("constraint-pkey", "physical_mappings_pkey" in constraints)
check("constraint-digest-unique", "physical_mappings_identity_digest_unique" in constraints)
check("constraint-physical-unique", "physical_mappings_physical_name_unique" in constraints)
check("constraint-octet-63", "octet_length(physical_name) = 63" in constraints)

code, functions, err = one(
    "SELECT p.proname || '|' || r.rolname || '|' || p.prosecdef::text || '|' || "
    "coalesce(p.proconfig::text, '') || '|' || pg_catalog.strpos(p.prosrc, 'pm_sha256')::text "
    "FROM pg_catalog.pg_proc AS p "
    "JOIN pg_catalog.pg_roles AS r ON r.oid = p.proowner "
    "JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace "
    "WHERE n.nspname = '_oo' AND p.proname IN "
    "('lookup_physical_mapping', 'pm_sha256', 'register_physical_mapping') ORDER BY 1;"
)
(OUT / "functions.txt").write_text(functions + "\n", encoding="utf-8")
parsed = {}
for row in functions.splitlines():
    parts = row.split("|")
    if parts and parts[0]:
        parsed[parts[0]] = parts
lookup_parts = parsed.get("lookup_physical_mapping", [])
register_parts = parsed.get("register_physical_mapping", [])
sha_parts = parsed.get("pm_sha256", [])
check(
    "lookup-definer-no-hash",
    len(lookup_parts) >= 5
    and lookup_parts[1] == "oo_target_owner"
    and lookup_parts[2] == "true"
    and lookup_parts[-1] == "0",
    "|".join(lookup_parts),
)
check(
    "register-definer-hashes",
    len(register_parts) >= 5
    and register_parts[1] == "oo_target_owner"
    and register_parts[2] == "true"
    and register_parts[-1] not in ("", "0"),
    "|".join(register_parts),
)
check(
    "sha-invoker",
    len(sha_parts) >= 3 and sha_parts[1] == "oo_target_owner" and sha_parts[2] == "false",
    "|".join(sha_parts),
)

code, relation, err = one(
    "SELECT c.relpersistence::text || '|' || pg_catalog.pg_get_userbyid(c.relowner) "
    "FROM pg_catalog.pg_class AS c "
    "JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace "
    "WHERE n.nspname = '_oo' AND c.relname = 'physical_mappings';"
)
check("relation-logged-owner", code == 0 and relation == "p|oo_target_owner", relation or err)

code, privs, err = one(
    "SELECT has_table_privilege('oo_a142_agent', '_oo.physical_mappings', 'SELECT')::text || ' ' || "
    "has_table_privilege('oo_a142_runtime', '_oo.physical_mappings', 'SELECT')::text || ' ' || "
    "has_function_privilege('oo_a142_agent', '_oo.register_physical_mapping(text,text,bigint,text,text,text,text,bytea)', 'EXECUTE')::text || ' ' || "
    "has_function_privilege('oo_a142_runtime', '_oo.register_physical_mapping(text,text,bigint,text,text,text,text,bytea)', 'EXECUTE')::text || ' ' || "
    "has_function_privilege('oo_a142_runtime', '_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)', 'EXECUTE')::text || ' ' || "
    "has_function_privilege('oo_a142_runtime', '_oo.pm_sha256(bytea)', 'EXECUTE')::text || ' ' || "
    "has_function_privilege('public', '_oo.lookup_physical_mapping(text,text,bigint,text,text,text,text)', 'EXECUTE')::text;"
)
(OUT / "privileges.txt").write_text(privs + "\n", encoding="utf-8")
check("privileges", code == 0 and privs == "false false false true true false false", privs or err)

fence_sql = (
    "SELECT _oo.commit_apply_receipt("
    "'shard_a', '11111111-1111-4111-8111-111111111111'::uuid, 1, 'src_main', 'table_one', 'batch-1', "
    "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', "
    "'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', "
    "'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', "
    "'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', "
    "'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', "
    "'snapshot-zero/v1', 'lineage_a', 0, 0, false, true, true, NULL, NULL, decode('', 'hex'));"
)
code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_a142_runtime';\nEXECUTE $fence$" + fence_sql + "$fence$;",
)
check("fence-unfenced", code == 0 and state == "OO011", state or err)
code, receipts, err = one("SELECT count(*)::text FROM _oo.apply_receipts;")
check("receipts-zero", code == 0 and receipts == "0", receipts or err)
code, public_tables, err = one(
    "SELECT count(*)::text FROM pg_catalog.pg_class AS class "
    "JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = class.relnamespace "
    "WHERE namespace.nspname = 'public' AND class.relkind = 'r';"
)
check("public-tables-zero", code == 0 and public_tables == "0", public_tables or err)

skip = "\n".join(
    [
        "BEGIN;",
        "SET LOCAL lock_timeout = '15s';",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202605);",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;",
        "SELECT checksum FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';",
        "COMMIT;",
    ]
)
code, out, err = psql(DB, skip)
check("second-apply-lock", code == 0 and hash_0004 in out, out.strip() or err)
code, after, err = one(
    "SELECT checksum || '|' || applied_at::text FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';"
)
check("second-apply-unchanged", code == 0 and after == f"{hash_0004}|{applied_at}", after or err)
code, oid_after, err = one(
    "SELECT c.oid::text FROM pg_catalog.pg_class AS c "
    "JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace "
    "WHERE n.nspname = '_oo' AND c.relname = 'physical_mappings';"
)
check("second-apply-same-oid", code == 0 and oid_after == table_oid, oid_after)

mismatch = "\n".join(
    [
        "BEGIN;",
        "SET LOCAL lock_timeout = '15s';",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202605);",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE;",
        "DO $pm$",
        "DECLARE",
        "  stored text;",
        "BEGIN",
        "  SELECT checksum INTO stored FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';",
        f"  IF stored IS DISTINCT FROM '{LIE}' THEN",
        "    RAISE EXCEPTION 'migration checksum mismatch' USING ERRCODE = 'OO001';",
        "  END IF;",
        "END",
        "$pm$;",
        "COMMIT;",
    ]
)
code, out, err = psql(DB, mismatch)
(OUT / "mismatch.stderr").write_text(err, encoding="utf-8")
check("checksum-mismatch-oo001", code != 0 and "OO001" in (err + out), err.strip()[:400])
code, after, err = one(
    "SELECT checksum || '|' || applied_at::text FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';"
)
check("checksum-mismatch-row-unchanged", code == 0 and after == f"{hash_0004}|{applied_at}", after or err)

code, state, out, err = notice_state(
    DB,
    "EXECUTE 'SET LOCAL ROLE oo_target_owner'; "
    f"EXECUTE 'UPDATE _oo.schema_migrations SET checksum = ''{LIE}'' WHERE version = ''physical-mapping/v1''';",
)
check("marker-update-rejected", code == 0 and state == "OO014", state or err)
code, after, err = one(
    "SELECT checksum FROM _oo.schema_migrations WHERE version = 'physical-mapping/v1';"
)
check("marker-update-unchanged", code == 0 and after == hash_0004, after or err)

quoted = '"' + left_name + '"'
code, quoted_ok, err = one(f"SELECT 1 AS {quoted};")
check("quoted-identifier", code == 0 and quoted_ok == "1", quoted_ok or err)

restart_lookup = (
    "SELECT _oo.lookup_physical_mapping("
    f"{sql_quote(SOURCE)}, {sql_quote(CONFIG)}, {GENERATION}, 'namespace', {sql_quote(left)}, '', '');\n"
)
restart_select = (
    "SELECT physical_name || '|' || encode(identity_digest, 'hex') || '|' || octet_length(physical_name)::text "
    "FROM _oo.physical_mappings "
    f"WHERE source = {sql_quote(SOURCE)} AND config_digest = {sql_quote(CONFIG)} "
    f"AND run_generation = {GENERATION} AND level = 'n' AND logical_namespace = {sql_quote(left)} "
    "AND logical_table = '' AND logical_column = '';\n"
)
(OUT / "restart-lookup.sql").write_text(restart_lookup, encoding="utf-8")
(OUT / "restart-select.sql").write_text(restart_select, encoding="utf-8")
(OUT / "expected.txt").write_text(
    f"physical={left_name}\ndigest={left_digest}\nlogical_len={len(left)}\n",
    encoding="utf-8",
)

log(f"fails={len(fails)}")
if fails:
    log("fail_names=" + ",".join(fails))
    finish(1)
log("probe=ok")
finish(0)
