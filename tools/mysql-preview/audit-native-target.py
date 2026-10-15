#!/usr/bin/env python3
"""Independently read every source/target fixture cell for a native generation."""
import argparse
import json
import shlex
import subprocess
import re
from decimal import Decimal


def ident(value):
    if not value.replace("_", "").isalnum():
        raise ValueError("identifier")
    return value


def lines(command):
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    for line in process.stdout:
        yield json.loads(line)
    error = process.stderr.read()
    if process.wait():
        raise RuntimeError(error[:300])


def pg(sql):
    command = ["docker", "exec", "oo-dl01w1-06-pg", "sh", "-lc",
               'psql -U "$POSTGRES_USER" -d mysql_native_20261012 -Atc ' + shlex.quote(sql)]
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    return result.stdout.splitlines()


def normalize(value, kind):
    if value is None:
        return None
    if kind == "decimal":
        return Decimal(value)
    if kind == "json":
        return json.loads(value)
    if kind == "time":
        match = re.fullmatch(r"(-?)(\d+):(\d{2}):(\d{2})(?:\.(\d{1,6}))?", value)
        if match is None:
            raise ValueError("time representation")
        sign, hours, minutes, seconds, fraction = match.groups()
        micros = ((int(hours) * 3600 + int(minutes) * 60 + int(seconds)) * 1000000
                  + int((fraction or "").ljust(6, "0")))
        return -micros if sign else micros
    if kind in ("date", "datetime"):
        return value.replace("T", " ").rstrip("0").rstrip(".") if "." in value else value.replace("T", " ")
    return value.lower() if kind == "varbinary" else value


def compare(generation, selected_table=None, source_id="mysql_native"):
    source_id = ident(source_id)
    catalog = pg("select logical_table,logical_column,physical_name from _oo.physical_mappings "
                 f"where source='{source_id}' and run_generation={generation} and level='c' "
                 "order by logical_table,logical_column")
    mapped = {}
    for row in catalog:
        table, column, physical = row.split("|")
        if selected_table is not None and table != selected_table:
            continue
        mapped.setdefault(table, {})[column] = physical
    types = subprocess.run(["docker", "exec", "oo-dl01w1-06-mysql", "mysql",
                            "--defaults-extra-file=/secrets/sol-dba.cnf", "-NBe",
                            "select table_name,column_name,data_type from information_schema.columns "
                            "where table_schema='native_mysql_20261012' order by table_name,ordinal_position"],
                           capture_output=True, text=True, check=True).stdout.splitlines()
    source = {}
    for row in types:
        table, column, kind = row.split("\t")
        if selected_table is not None and table != selected_table:
            continue
        source.setdefault(table, []).append((column, kind))
    if set(source) != set(mapped) or len(source) != (1 if selected_table else 5):
        raise RuntimeError("incomplete table mapping")
    result = {}
    for table, columns in source.items():
        table = ident(table)
        if set(mapped[table]) != {name for name, _ in columns}:
            raise RuntimeError("incomplete column mapping: " + table)
        physical = pg(f"select physical_name from _oo.physical_mappings where source='{source_id}' "
                      f"and run_generation={generation} and level='t' and logical_table='{table}'")
        if len(physical) != 1:
            raise RuntimeError("ambiguous physical table: " + table)
        left, right = [], []
        for name, kind in columns:
            name = ident(name)
            sql_name = "`" + name + "`"
            expression = f"CAST({sql_name} AS CHAR)"
            if kind == "varbinary":
                expression = f"HEX({sql_name})"
            if kind in ("date", "datetime"):
                expression = f"CASE WHEN CAST({sql_name} AS CHAR) REGEXP '^(0000-[0-9]{{2}}-[0-9]{{2}}|[0-9]{{4}}-00-[0-9]{{2}}|[0-9]{{4}}-[0-9]{{2}}-00)( |$)' THEN NULL ELSE {expression} END"
            left.append(expression)
            target = '"' + ident(mapped[table][name]) + '"'
            right.append(f"encode({target},'hex')" if kind == "varbinary" else target + "::text")
        mysql_sql = f"SELECT JSON_ARRAY({','.join(left)}) FROM `native_mysql_20261012`.`{table}` ORDER BY `{columns[0][0]}`"
        pg_sql = f'SELECT json_build_array({",".join(right)}) FROM _oo."{ident(physical[0])}" ORDER BY "{ident(mapped[table][columns[0][0]])}"'
        mysql_rows = lines(["docker", "exec", "oo-dl01w1-06-mysql", "mysql",
                            "--defaults-extra-file=/secrets/sol-dba.cnf", "--batch", "--raw", "--skip-column-names", "-e", mysql_sql])
        target_rows = lines(["docker", "exec", "oo-dl01w1-06-pg", "sh", "-lc",
                             'psql -U "$POSTGRES_USER" -d mysql_native_20261012 -Atc ' + shlex.quote(pg_sql)])
        count = 0
        sentinel = object()
        while True:
            a = next(mysql_rows, sentinel)
            b = next(target_rows, sentinel)
            if a is sentinel and b is sentinel:
                break
            if a is sentinel or b is sentinel:
                raise RuntimeError("row count differs: " + table)
            for index, (name, kind) in enumerate(columns):
                if normalize(a[index], kind) != normalize(b[index], kind):
                    raise RuntimeError(f"cell differs: {table}.{name} row {count}")
            count += 1
        result[table] = {"rows": count, "columns": len(columns), "different_cells": 0}
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--generation", type=int, required=True)
    parser.add_argument("--source-id", default="mysql_native")
    parser.add_argument("--table", choices=["hubs", "shipments", "tracking_events", "range_gap", "value_probe"])
    args = parser.parse_args()
    print(json.dumps({"generation": args.generation, "tables": compare(args.generation, args.table, args.source_id)}, sort_keys=True))
