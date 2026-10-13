#!/usr/bin/env python3
"""Read independent typed facts from the restricted source role after catch-up."""
import argparse
import json
from pathlib import Path
import subprocess


QUERIES = {
    "hub_count": "SELECT count(*)::bigint AS hub_count FROM public.hubs",
    "shipment_count": "SELECT count(*)::bigint AS shipment_count FROM public.shipments",
    "event_count": "SELECT count(*)::bigint AS event_count FROM public.tracking_events",
    "note_count": "SELECT count(*)::bigint AS note_count FROM customer_east.delivery_notes",
    "shipment_18": "SELECT shipment_id,shipment_token,cost_amount,created_at,promised_at,encode(seal,'hex') AS seal_hex FROM public.shipments WHERE shipment_id=18",
    "event_910000004": "SELECT event_id,event_token,shipment_id,hub_id,status,event_at,encode(payload,'hex') AS payload_hex FROM public.tracking_events WHERE event_id=910000004",
    "note_1": "SELECT note_id,amount,encode(receipt,'hex') AS receipt_hex,note_token,signed_at,memo FROM customer_east.delivery_notes WHERE note_id=1",
    "hub_2": "SELECT hub_id,hub_token,hub_code,hub_name,region_code,latitude,longitude,opened_at,encode(badge,'hex') AS badge_hex,note FROM public.hubs WHERE hub_id=2",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--end-lsn", required=True)
    args = parser.parse_args()
    facts = []
    for name, sql in QUERIES.items():
        wrapped = f"SELECT coalesce(jsonb_agg(r),'[]'::jsonb)::text FROM ({sql}) r"
        raw = subprocess.check_output([
            "docker", "exec", "oo-dl01w1-46-src", "psql", "-U", "oo_dl01w1_46_reader",
            "-d", "src", "-X", "-A", "-t", "-c", wrapped,
        ], text=True).strip()
        rows = json.loads(raw)
        if name.endswith(("_18", "_1", "_2", "_910000004")) and len(rows) != 1:
            raise ValueError(f"fixture row missing: {name}")
        facts.append({"name": name, "sql": sql, "rows": rows})
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({"protocol": "oo.postgres-preview.independent-gold/v1",
                                    "reader": "oo_dl01w1_46_reader", "source_end_lsn": args.end_lsn,
                                    "facts": facts}, indent=2) + "\n")
    print(args.out)


if __name__ == "__main__":
    main()
