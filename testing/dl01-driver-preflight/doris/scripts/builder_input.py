#!/usr/bin/env python3
"""Write the Doris builder handoff from the applied capture indexes.

The file lists real Arrow bytes, schema digests, and the sealed-catalog gap.
It does not write seal.json, cards.json, or a sealed catalog.
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
PRODUCT = ROOT / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product"
OUT = PRODUCT / "review-65abb6c7/builder-input.json"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_generation(name: str) -> dict:
    index_path = PRODUCT / "native-2e0bd/receipts" / name
    index = json.loads(index_path.read_text())
    tables = []
    for row in index["tables"]:
        request_name = row["receipt"].replace(".receipt.json", ".request.json")
        request = json.loads((index_path.parent / request_name).read_text())
        arrow = Path(request["arrow_path"])
        arrow_sha = sha256_file(arrow)
        if arrow_sha != row["arrow_sha256"]:
            raise SystemExit(f"arrow hash mismatch for {row['table']}")
        if request["applied_pos"] != "0":
            raise SystemExit(f"unexpected applied_pos on {row['table']}")
        if request["consistency_grade"] != "S0":
            raise SystemExit(f"unexpected grade on {row['table']}")
        tables.append(
            {
                "table": row["table"],
                "route": row["route"],
                "quality": row["quality"],
                "rows": row["rows"],
                "tombstones": row["tombstones"],
                "arrow_path": str(arrow.relative_to(ROOT)),
                "arrow_sha256": arrow_sha,
                "arrow_schema_sha256": row["arrow_schema_sha256"],
                "receipt_sha256": row["receipt_sha256"],
                "request_applied_pos": request["applied_pos"],
                "request_consistency_grade": request["consistency_grade"],
            }
        )
    return {
        "generation": index["generation"],
        "label": index["label"],
        "run_id": index["run_id"],
        "schema_digest": index["schema_digest"],
        "type_digest": index["type_digest"],
        "config_digest": index["config_digest"],
        "audit_digest": index["audit_digest"],
        "driver_digest": index["driver_digest"],
        "tables": tables,
    }


def main() -> None:
    oracle = json.loads((PRODUCT / "native-2e0bd/raw/cell-oracle.json").read_text())
    if oracle["cell_mismatch_labels"] != []:
        raise SystemExit("cell oracle mismatches are not empty")
    document = {
        "protocol": "oo.doris.builder-input-handoff/v1",
        "task_id": "DL01-W1-43",
        "consumer": "DL01-W1-71",
        "source_id": "dorispreview",
        "source_sha": "66c7d5bd24bdcf894976da9508cc963356079e06",
        "engine": "doris",
        "product_version": "doris-3.1.4-rc02-7f5ba43de6c",
        "logical_schema": "oo_dl01w1_43",
        "owner": "5106fed0-5bcd-4aaf-8df7-838c95a7d7e2",
        "sealed_catalog": None,
        "seal_json": None,
        "cards_json": None,
        "catalog_import": {
            "command": "oo-pipeline catalog-import-native",
            "request": "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/native-2e0bd/raw/catalog-import.request.json",
            "exit": 1,
            "stderr": "oo-pipeline: nonadmitted",
        },
        "loader_review": {
            "review": "65abb6c7f9ee8af20ecd50de5a6a2e66f15750b0",
            "judgment": "REQUEST_CHANGES",
            "reviewed_native_update_blob": "32a0e7d117f95c41c52a1389e63c220635f6a9b9",
            "head_native_update_blob": "3faf071fb747dd48e4789221fb90687e29d3a74b",
            "native_binding_blob": "10240cf44a11a6f64c79eee826b59d7242dbab6a",
            "head_loader_commit": "5e1c0ce86d7563e61ebfe0dc306912cb69985844",
            "repair_status": "absent",
        },
        "cell_oracle": {
            "path": "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/native-2e0bd/raw/cell-oracle.json",
            "sha256": sha256_file(PRODUCT / "native-2e0bd/raw/cell-oracle.json"),
            "cell_mismatch_labels": [],
        },
        "control_create": {
            "method": "POST",
            "path": "/control/v1/builds",
            "listener": "127.0.0.1:18449",
            "curl_exit": 7,
            "body_sent": False,
            "field_names": [
                "allowance_id",
                "config_commit",
                "identity_revision",
                "idempotency_key",
                "llm_secret_ref",
                "project_id",
                "source_ref",
            ],
            "recipe": "tools/selfbuild-control-api/recipes/CALLERS.md",
        },
        "generations": [load_generation("g1.index.json"), load_generation("g2.index.json")],
    }
    encoded = json.dumps(document, indent=2, sort_keys=True) + "\n"
    if "secret://" in encoded:
        raise SystemExit("handoff would embed a secret uri")
    def refuse_secret_key(value):
        if isinstance(value, dict):
            if "secret_ref" in value:
                raise SystemExit("handoff would embed a secret_ref key")
            for item in value.values():
                refuse_secret_key(item)
        elif isinstance(value, list):
            for item in value:
                refuse_secret_key(item)
    refuse_secret_key(document)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(encoded)
    print(sha256_file(OUT))


if __name__ == "__main__":
    main()
