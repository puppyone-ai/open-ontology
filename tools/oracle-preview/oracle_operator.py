#!/usr/bin/env python3
"""Local Oracle source registration. Not worker/admission/publication authority."""
from pathlib import Path
import json
import stat

import bounded_source as b
import buffer_cursor
import producer as p

INPUT_KEYS = {"type", "host", "port", "service", "database", "user", "secret_ref"}

def validate_input(value):
    if not isinstance(value, dict) or set(value) != INPUT_KEYS or value["type"] != "oracle":
        raise ValueError("oracle_input_keys")
    if value["host"] != "127.0.0.1" or type(value["port"]) is not int or not 1 <= value["port"] <= 65535:
        raise ValueError("oracle_local_endpoint")
    if any(not isinstance(value[k], str) or not value[k] or "\0" in value[k]
           for k in ("service", "database", "user", "secret_ref")):
        raise ValueError("oracle_input_value")
    if not value["secret_ref"].startswith("secret://"):
        raise ValueError("oracle_secret_ref")

def private(path):
    path = Path(path)
    mode = path.lstat().st_mode
    if not path.is_absolute() or ".." in path.parts or not stat.S_ISREG(mode) or mode & 0o077:
        raise ValueError("oracle_private_file")
    return path

def captured_columns_match(captured, current):
    # Older sealed mappings predate the four explicit dictionary flags. These
    # flags may be added only with the ordinary visible, stored-column values.
    if len(captured) != len(current):
        return False
    for i, (before, after) in enumerate(zip(captured, current), 1):
        extras = {"position": i, "hidden": "NO", "virtual": "NO", "user_generated": "YES"}
        if any(k not in extras or after[k] != extras[k] for k in set(after)-set(before)):
            return False
        if any(k not in after or after[k] != v for k, v in before.items()):
            return False
    return True

def register(input_value, installed, destination):
    """Measure identity/catalog with the fixed reader before registering source scope."""
    validate_input(input_value)
    if installed.get("protocol") != "oo.oracle.source-operator/v1":
        raise ValueError("oracle_operator_protocol")
    if any(installed.get(k) != input_value[k] for k in INPUT_KEYS):
        raise ValueError("oracle_operator_connection")
    reader = p.pinned(installed["reader"])
    registry = json.loads(private(installed["secret_registry"]).read_text())
    password_file = private(installed["password_file"])
    if registry.get(input_value["secret_ref"]) != str(password_file):
        raise ValueError("oracle_secret_binding")
    if not password_file.resolve().is_relative_to(Path(installed["host_secret_root"]).resolve(strict=True)):
        raise ValueError("oracle_secret_root")
    config = {k: installed[k] for k in ("host", "port", "service", "user", "owner", "source_id",
                                       "identity", "password_file")}
    identity = buffer_cursor.identity(config["identity"])
    if identity != installed["source_identity"] or identity["db_unique_name"] != input_value["database"]:
        raise ValueError("oracle_database_identity")
    if config["identity"].split("|")[-1] != config["user"]:
        raise ValueError("oracle_principal_identity")
    inventory = b.Driver(reader, config).call("inventory")
    actual = sorted(f'{t["owner"]}.{t["table"]}' for t in inventory["tables"])
    exclusions = installed.get("inventory_exclusions", {})
    if (not isinstance(exclusions, dict) or any(not isinstance(v, str) or not v for v in exclusions.values())
            or not set(exclusions) <= set(actual) or set(exclusions) & set(installed["authorized_tables"])
            or sorted(set(actual)-set(exclusions)) != sorted(installed["authorized_tables"])):
        raise ValueError("oracle_authorized_table_coverage")
    buffer_binding = None
    if installed.get("buffer_index") is not None:
        index_path = p.pinned(installed["buffer_index"])
        buffer_cursor.verify(index_path, installed["buffer_index"]["sha256"])
        index = json.loads(index_path.read_text())
        context = json.loads(p.pinned(index["context"]).read_text())
        manifest = p.validate_bundle(p.pinned(context["bundle"]).parent, context["bundle"]["sha256"])
        if (context["source_id"] != config["source_id"] or manifest["source_id"] != config["source_id"]
                or context["source_identity"] != identity
                or sorted(manifest["typed_tables"]) != sorted(installed["authorized_tables"])):
            raise ValueError("oracle_operator_buffer_identity")
        for table in inventory["tables"]:
            qualified = f'{table["owner"]}.{table["table"]}'
            if qualified not in manifest["typed_tables"]:
                continue
            typed = manifest["typed_tables"][qualified]
            columns = [m["source_column"] for m in manifest["source_mappings"][qualified]]
            keys = [typed["columns"][i]["name"] for i in typed["keys"]]
            if not captured_columns_match(columns, table["columns"]) or keys != table["keys"]:
                raise ValueError("oracle_operator_buffer_schema_drift")
        buffer_binding = {"context": index["context"], "bundle": context["bundle"],
                          "input_mode": "retained_buffer", "source_id": config["source_id"],
                          "source_resume": "real vendor offset/history remain independent",
                          "fresh_source_rows_checked": False}
    root = Path(destination)
    root.mkdir(mode=0o700, exist_ok=False)
    for name, value in (("input.json", input_value), ("operator.json", installed),
                        ("source-config.json", config), ("inventory.json", inventory)):
        b.atomic(root/name, b.canonical(value)+b"\n")
    b.atomic(root/"operators.json", b.canonical({"protocol": "oo.oracle.source-operators/v1",
                                               "operators": [str(root/"operator.json")]})+b"\n")
    receipt = {"protocol": "oo.oracle.source-registration/v1", "source_id": config["source_id"],
               "source_identity": identity, "reader_sha256": installed["reader"]["sha256"],
               "input_sha256": b.sha(root/"input.json"), "operator_sha256": b.sha(root/"operator.json"),
               "inventory_sha256": b.sha(root/"inventory.json"), "service": config["service"],
               "database_semantics": "expected DB_UNIQUE_NAME; not service, PDB name or schema",
               "source_registered": True, "target_registered": False, "admitted": False,
               "published": False, "business_rows_read": 0, "identity_catalog_checked": True,
               "inventory_exclusions": exclusions, "buffer_binding": buffer_binding,
               "catalog_projection": "sealed columns plus explicit safe dictionary flags/v1"}
    b.atomic(root/"registration.json", b.canonical(receipt)+b"\n")
    return receipt
