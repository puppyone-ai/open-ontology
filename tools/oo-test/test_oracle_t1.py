import json
import re
import unittest

from oracle_t1 import compare_oracle, read_lob


class Source:
    def __init__(self):
        self.rows = [
            {"ID": "1", "C": "x" * 16000, "N": "\u4e2d" * 3001, "B": b"\0\xff" * 1200},
            {"ID": "2", "C": "", "N": "", "B": b""},
            {"ID": "3", "C": None, "N": None, "B": None},
        ]
        self.reads = 0

    def sql(self, sql):
        if "all_tab_columns" in sql:
            return 0, "ID|NUMBER\nC|CLOB\nN|NCLOB\nB|BLOB\n"
        if "GETLENGTH" not in sql and "SUBSTR" not in sql:
            return 0, "31|-|-|-\n32|-|-|-\n33|-|-|-\n"
        row = int(re.search(r"WHERE rn=(\d+)", sql)[1]) - 1
        column = re.search(r'DBMS_LOB.(?:GETLENGTH|SUBSTR)\("(\w+)"', sql)[1]
        value = self.rows[row][column]
        if "GETLENGTH" in sql:
            return 0, "-" if value is None else str(len(value))
        self.reads += 1
        offset = int(re.search(r'SUBSTR\("\w+",1000,(\d+)\)', sql)[1]) - 1
        part = value[offset:offset + 1000]
        encoded = part if isinstance(part, bytes) else part.encode()
        return 0, str(len(part)) + "|" + encoded.hex()


class Target:
    def __init__(self, source, corrupt=False):
        self.source, self.corrupt = source, corrupt

    def sql(self, _):
        rows = [dict(row, B=row["B"].hex() if row["B"] is not None else None)
                for row in self.source.rows]
        if self.corrupt:
            rows[1]["C"] = None
        return "\n".join(json.dumps(row) for row in rows)


class OracleLobComparisonTests(unittest.TestCase):
    def test_d17_zoned_instants_on_lob_tables(self):
        class ZonedSource(Source):
            def sql(self, sql):
                if "all_tab_columns" in sql:
                    return 0, ("ID|NUMBER\nC|CLOB\nN|NCLOB\nB|BLOB\n"
                               "Z|TIMESTAMP(6) WITH TIME ZONE\n"
                               "L|TIMESTAMP(6) WITH LOCAL TIME ZONE\n"
                               "PLAIN|TIMESTAMP(6)\nORIGINAL_ZONE|VARCHAR2\n")
                if "GETLENGTH" not in sql and "SUBSTR" not in sql:
                    values = ["+2024-01-02T08:00:00.123456000 +08:00"] * 2
                    values += ["+2024-01-02T08:00:00.123456000", "+08:00"]
                    suffix = "|".join(v.encode().hex() for v in values)
                    return 0, "\n".join(f"{n}|-|-|-|{suffix}" for n in ("31", "32", "33"))
                return super().sql(sql)

        class ZonedTarget(Target):
            def __init__(self, source, change=None):
                super().__init__(source)
                self.change = change or {}

            def sql(self, sql):
                rows = [dict(json.loads(line), Z="2024-01-02T00:00:00.123456+00:00",
                             L="2024-01-02T00:00:00.123456+00:00",
                             PLAIN="2024-01-02T08:00:00.123456", ORIGINAL_ZONE="+08:00")
                        for line in super().sql(sql).splitlines()]
                rows[0].update(self.change)
                return "\n".join(json.dumps(row) for row in rows)

        source = ZonedSource()
        self.assertTrue(compare_oracle(source, ZonedTarget(source), {"target": "fixture"},
                                       "TYPE_EDGES", None)["equal"])
        for change in [
            {"Z": "2024-01-02T00:00:00.123457+00:00"},
            {"L": "2024-01-02T00:00:01.123456+00:00"},
            {"PLAIN": "2024-01-02T00:00:00.123456"},
            {"ORIGINAL_ZONE": "+00:00"},
        ]:
            with self.subTest(change=change):
                self.assertFalse(compare_oracle(source, ZonedTarget(source, change),
                                                {"target": "fixture"}, "TYPE_EDGES", None)["equal"])
        with self.assertRaisesRegex(RuntimeError, "submicrosecond"):
            compare_oracle(source, ZonedTarget(source, {"Z": "2024-01-02T00:00:00.123456001+00:00"}),
                           {"target": "fixture"}, "TYPE_EDGES", None)

    def test_pg_bytea_prefix_and_decimal_json_are_lossless(self):
        class ExactSource(Source):
            def sql(self, sql):
                if "all_tab_columns" in sql:
                    return 0, "ID|NUMBER\nC|CLOB\nN|NCLOB\nB|BLOB\nEXACT|NUMBER\n"
                if "GETLENGTH" not in sql and "SUBSTR" not in sql:
                    number = "12345678901234567890.1234567890".encode().hex()
                    return 0, "\n".join(f"{n}|-|-|-|{number}" for n in ("31", "32", "33"))
                return super().sql(sql)
        class ExactTarget(Target):
            def sql(self, sql):
                rows = super().sql(sql).splitlines()
                output = []
                for line in rows:
                    row = json.loads(line)
                    if row["B"] is not None:
                        row["B"] = "\\x" + row["B"]
                    output.append(json.dumps(row)[:-1] + ',"EXACT":12345678901234567890.1234567890}')
                return "\n".join(output)
        source = ExactSource()
        self.assertTrue(compare_oracle(source, ExactTarget(source), {"target": "fixture"},
                                       "TYPE_EDGES", None)["equal"])

    def test_scalar_wide_query_wraps_without_changing_projection(self):
        class ScalarSource:
            def sql(self, sql):
                if "all_tab_columns" in sql:
                    return 0, "ID|NUMBER\n"
                self.query = sql
                self.assertion = max(map(len, sql.splitlines())) < 4999
                return 0, "31"
        query = "SELECT " + "||'|'||".join(
            f"""NVL(RAWTOHEX(UTL_I18N.STRING_TO_RAW(TO_CHAR("C{i:03}"),'AL32UTF8')),'-')"""
            for i in range(400)) + " FROM FIXTURE.WIDE_ROWS;"
        for table in ("EVENTS", "WIDE_ROWS"):
            source = ScalarSource()
            def scalar(run, target, receipt, name):
                self.assertEqual(name, table)
                return run.sql(query)
            self.assertEqual(compare_oracle(source, None, {}, table, scalar), (0, "31"))
            self.assertTrue(source.assertion)
            self.assertEqual("SET LINESIZE 1024" in source.query, table == "EVENTS")
            projected = source.query[source.query.index("SELECT "):].replace("\n", "")
            self.assertEqual(projected, query)

    def test_full_lobs_nulls_and_empty_locators(self):
        source = Source()
        result = compare_oracle(source, Target(source), {"target": "fixture"}, "TYPE_EDGES", None)
        self.assertTrue(result["equal"])
        self.assertGreater(source.reads, 20)
        result = compare_oracle(source, Target(source, True), {"target": "fixture"}, "TYPE_EDGES", None)
        self.assertFalse(result["equal"], "empty LOB is not NULL")

    def test_incomplete_part_is_not_silent(self):
        class Broken:
            def sql(self, sql):
                return 0, "10" if "GETLENGTH" in sql else "0|"
        with self.assertRaisesRegex(RuntimeError, "incomplete LOB"):
            read_lob(Broken(), "SELECT * FROM T", 1, "C", False)


if __name__ == "__main__":
    unittest.main()
