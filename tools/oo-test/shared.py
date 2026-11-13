"""One source container per engine x version, shared by suites (plan section 2.2).

Suites take turns on one owned snapshot clone. Each turn is a lease: the suite
gets its own slot/publication prefix and a recorded start position, and when
it ends every slot, publication and data change it made is removed by a reset
to a pristine copy taken right after seeding. The gold answers are re-checked
after every reset, so a leaked change fails the next lease instead of
silently feeding it. Destructive cases (restart, log purge, resetlogs) never
lease the shared source; they get their own clone.
"""
from contextlib import contextmanager
import re
import time

import runner

SHARED_SUITES = ("T0", "T1", "T2", "T4", "T7", "T8")
# T3 restarts the instance, purges logs and invalidates slots (Oracle resetlogs).
DESTRUCTIVE = frozenset({"T3"})
PRISTINE = "fixture_pristine"


def needs_own_clone(suite):
    return suite in DESTRUCTIVE


class SharedSource:
    def __init__(self, run):
        if run.engine not in ("postgres", "mysql"):
            # Oracle has no cheap in-instance pristine copy with constraints and
            # LOBs intact; every Oracle suite restarts from the COW snapshot.
            raise ValueError("shared_source_unsupported_engine")
        self.run = run
        self.engine = run.engine
        self.gold = None
        self.leases = []

    # -- engine primitives -------------------------------------------------
    def pg(self, text, database="postgres", required=True):
        """Admin SQL on a database other than the one being copied."""
        return self.run.command(
            ["docker", "exec", "-i", "-u", "postgres", self.run.name, "psql", "-X",
             "-v", "ON_ERROR_STOP=1", "-A", "-t", "-F", "|", "-U", "postgres", "-d", database],
            data=text, required=required)

    def mysql_tables(self, schema):
        _, raw = self.run.sql(
            "SELECT table_name FROM information_schema.tables WHERE table_schema='"
            + schema + "' AND table_type='BASE TABLE' ORDER BY table_name;", admin=True)
        names = raw.split()
        for name in names:
            if not re.fullmatch(r"[A-Za-z0-9_]+", name):
                raise ValueError("unexpected_fixture_table_name")
        return names

    def mysql_views(self):
        _, raw = self.run.sql(
            "SELECT table_name FROM information_schema.views WHERE table_schema='fixture' "
            "ORDER BY table_name;", admin=True)
        views = []
        for name in raw.split():
            if not re.fullmatch(r"[A-Za-z0-9_]+", name):
                raise ValueError("unexpected_fixture_view_name")
            _, row = self.run.sql(f"SHOW CREATE VIEW {name};", admin=True)
            views.append(row.split("\t")[1].replace("CREATE ", "CREATE OR REPLACE ", 1))
        return views

    def mysql_copy(self, source, destination):
        """Copy base tables; the destination database (grants, views) is kept."""
        tables = self.mysql_tables(source)
        statements = ["SET FOREIGN_KEY_CHECKS=0;",
                      f"CREATE DATABASE IF NOT EXISTS {destination};"]
        statements += [f"DROP TABLE {destination}.{name};"
                       for name in self.mysql_tables(destination)]
        for name in tables:
            statements.append(f"CREATE TABLE {destination}.{name} LIKE {source}.{name};")
            statements.append(f"INSERT INTO {destination}.{name} SELECT * FROM {source}.{name};")
        if destination == "fixture":
            statements += [view + ";" for view in self.views]
        self.run.sql("\n".join(statements), admin=True)
        return len(tables)

    def terminate_pg(self, database):
        self.pg("SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity "
                f"WHERE datname='{database}' AND pid<>pg_backend_pid();")

    def positions(self):
        if self.engine == "postgres":
            _, slots = self.pg("SELECT slot_name FROM pg_replication_slots ORDER BY 1;")
            _, pubs = self.pg("SELECT pubname FROM pg_publication ORDER BY 1;", database="fixture")
            _, lsn = self.pg("SELECT pg_current_wal_lsn();")
            _, conf = self.pg("SELECT name||'='||setting FROM pg_file_settings "
                              "WHERE sourcefile LIKE '%postgresql.auto.conf' AND setting IS NOT NULL ORDER BY 1;")
            return {"slots": slots.split(), "publications": pubs.split(), "lsn": lsn.strip(),
                    "auto_conf": [line for line in conf.split("\n") if "=" in line]}
        code, status = self.run.sql("SHOW MASTER STATUS;", admin=True, required=False)
        if code:
            _, status = self.run.sql("SHOW BINARY LOG STATUS;", admin=True)
        return {"binlog": status.strip()}

    # -- lifecycle ---------------------------------------------------------
    @runner.timed("seed")
    def prepare(self):
        """Take the pristine copy once, after seed and gold validation."""
        self.gold = self.run.gold()
        self.baseline = self.positions()
        if self.engine == "postgres":
            self.terminate_pg("fixture")
            self.pg(f"DROP DATABASE IF EXISTS {PRISTINE};\n"
                    f"CREATE DATABASE {PRISTINE} TEMPLATE fixture;\n"
                    f"REVOKE CONNECT, TEMPORARY ON DATABASE {PRISTINE} FROM PUBLIC;\n")
        else:
            self.views = self.mysql_views()
            self.mysql_copy("fixture", PRISTINE)
        return self.baseline

    @runner.timed("cleanup")
    def reset(self):
        """Drop what the lease created and restore the pristine data."""
        removed = {}
        if self.engine == "postgres":
            now = self.positions()
            slots = [s for s in now["slots"] if s not in self.baseline["slots"]]
            for slot in slots:
                if not re.fullmatch(r"[a-z0-9_]+", slot):
                    raise ValueError("unexpected_slot_name")
                self.pg("SELECT pg_terminate_backend(active_pid) FROM pg_replication_slots "
                        f"WHERE slot_name='{slot}' AND active_pid IS NOT NULL;")
                self.pg(f"SELECT pg_drop_replication_slot('{slot}');")
            removed["slots"] = slots
            removed["settings"] = self.restore_settings(now["auto_conf"])
            self.terminate_pg("fixture")
            self.pg("DROP DATABASE fixture;\n"
                    f"CREATE DATABASE fixture TEMPLATE {PRISTINE};\n"
                    # Database ACLs are not part of the template copy.
                    "REVOKE TEMPORARY ON DATABASE fixture FROM PUBLIC;\n"
                    "GRANT CONNECT ON DATABASE fixture TO fixture_reader;\n")
        else:
            removed["tables"] = self.mysql_copy(PRISTINE, "fixture")
        answers = self.run.gold()
        if answers != self.gold:
            raise RuntimeError("shared_source_reset_changed_gold")
        after = self.positions()
        if self.engine == "postgres" and (after["slots"] != self.baseline["slots"]
                                          or after["publications"] != self.baseline["publications"]):
            raise RuntimeError("shared_source_reset_left_replication_state")
        return removed

    def restore_settings(self, current):
        """Undo ALTER SYSTEM; a pending restart means the suite needed its own clone."""
        base = dict(item.split("=", 1) for item in self.baseline["auto_conf"])
        now = dict(item.split("=", 1) for item in current)
        changed = sorted(n for n in set(base) | set(now) if base.get(n) != now.get(n))
        for name in changed:
            if not re.fullmatch(r"[a-z_.]+", name):
                raise ValueError("unexpected_setting_name")
            if name in base:
                value = base[name].replace("'", "''")
                self.pg(f"ALTER SYSTEM SET {name} = '{value}';")
            else:
                self.pg(f"ALTER SYSTEM RESET {name};")
        if changed:
            self.pg("SELECT pg_reload_conf();")
            time.sleep(0.5)
            _, pending = self.pg("SELECT name FROM pg_settings WHERE pending_restart;")
            if pending.strip():
                raise RuntimeError("lease_changed_restart_setting_requires_own_clone")
        return changed

    @contextmanager
    def lease(self, suite):
        if needs_own_clone(suite):
            raise ValueError("destructive_suite_requires_own_clone")
        if suite not in SHARED_SUITES:
            raise ValueError("unknown_shared_suite")
        if self.gold is None:
            raise RuntimeError("shared_source_not_prepared")
        start = self.positions()
        record = {"suite": suite, "slot_prefix": f"oo_{suite.lower()}_",
                  "start": start, "started": time.time()}
        self.leases.append(record)
        began = time.monotonic()
        try:
            yield record
        finally:
            record["suite_seconds"] = round(time.monotonic() - began, 3)
            reset_started = time.monotonic()
            record["removed"] = self.reset()
            record["reset_seconds"] = round(time.monotonic() - reset_started, 3)
            runner.write_json(self.run.path / "shared-leases.json", self.leases)
