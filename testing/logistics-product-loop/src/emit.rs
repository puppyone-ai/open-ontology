//! Write the generated fixture. The recipe loads these files into the source.
//! The target stays empty until the product copy runs.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::fixture::{
    canonical_event, canonical_hub, canonical_shipment, copy_event, copy_hub, copy_shipment, EVENT_ROWS, HUB_ROWS, SHIPMENT_ROWS,
};
use crate::gold::{cells, questions};
use crate::json::{object, stringify, Value};


pub fn write_fixture(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    fs::write(dir.join("schema.sql"), schema_sql()).map_err(|error| error.to_string())?;
    let hub_hash = write_lines(&dir.join("hubs.copy"), &dir.join("hubs.canonical"), HUB_ROWS, copy_hub, canonical_hub)?;
    let shipment_hash = write_lines(&dir.join("shipments.copy"), &dir.join("shipments.canonical"), SHIPMENT_ROWS, copy_shipment, canonical_shipment)?;
    let event_hash = write_lines(
        &dir.join("tracking_events.copy"),
        &dir.join("tracking_events.canonical"),
        EVENT_ROWS,
        copy_event,
        canonical_event,
    )?;
    let manifest = object(vec![
        ("protocol", Value::String("oo.logistics-product-loop.fixture/v1".to_owned())),
        ("schema", Value::String("public".to_owned())),
        ("hubs", Value::Number(HUB_ROWS.to_string())),
        ("shipments", Value::Number(SHIPMENT_ROWS.to_string())),
        ("tracking_events", Value::Number(EVENT_ROWS.to_string())),
        ("total", Value::Number((HUB_ROWS + SHIPMENT_ROWS + EVENT_ROWS).to_string())),
        ("hubs_canonical_sha256", Value::String(hub_hash)),
        ("shipments_canonical_sha256", Value::String(shipment_hash)),
        ("tracking_events_canonical_sha256", Value::String(event_hash)),
        ("oracle_included", Value::Bool(false)),
    ]);
    fs::write(dir.join("manifest.json"), stringify(&manifest)).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn write_gold(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let mut public_questions = Vec::new();
    let mut oracle_rows = Vec::new();
    for question in questions() {
        public_questions.push(Value::Object(vec![
            ("id".to_owned(), Value::String(question.id.to_owned())),
            ("text".to_owned(), Value::String(question.text.to_owned())),
        ]));
        let cell_rows = cells(question.id).ok_or_else(|| format!("missing cells {}", question.id))?;
        oracle_rows.push(Value::Object(vec![
            ("id".to_owned(), Value::String(question.id.to_owned())),
            ("sql".to_owned(), Value::String(question.sql.to_owned())),
            ("cells".to_owned(), Value::Array(cell_rows.into_iter().map(|row| Value::Array(row.into_iter().map(Value::String).collect())).collect())),
        ]));
    }
    let public_doc = object(vec![
        ("protocol", Value::String("oo.logistics-product-loop.questions/v1".to_owned())),
        ("answers_included", Value::Bool(false)),
        ("sql_included", Value::Bool(false)),
        ("questions", Value::Array(public_questions)),
    ]);
    let oracle = object(vec![
        ("protocol", Value::String("oo.logistics-product-loop.oracle/v1".to_owned())),
        ("not_a_prompt", Value::Bool(true)),
        ("not_pack_input", Value::Bool(true)),
        ("questions", Value::Array(oracle_rows)),
    ]);
    fs::write(dir.join("questions.json"), stringify(&public_doc)).map_err(|error| error.to_string())?;
    fs::write(dir.join("oracle.json"), stringify(&oracle)).map_err(|error| error.to_string())?;
    fs::write(
        dir.join("ORACLE-NOT-A-PROMPT.txt"),
        "oracle.json holds SQL and expected cells. Do not mount it into the model prompt, the Knowledge Pack, Pi, or the gateway.\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn canonical_hash(table: &str) -> Result<String, String> {
    let mut hasher = crate::sha256::Hasher::new();
    match table {
        "hubs" => hash_rows(&mut hasher, HUB_ROWS, canonical_hub),
        "shipments" => hash_rows(&mut hasher, SHIPMENT_ROWS, canonical_shipment),
        "tracking_events" => hash_rows(&mut hasher, EVENT_ROWS, canonical_event),
        _ => return Err("unknown table".to_owned()),
    }
    Ok(hasher.finish_hex())
}

pub fn schema_sql() -> &'static str {
    r#"CREATE TABLE public.hubs (
    hub_id bigint PRIMARY KEY,
    hub_code text NOT NULL,
    hub_name text NOT NULL,
    region_code text,
    latitude numeric(38,4),
    longitude numeric(38,4),
    capacity_grams bigint NOT NULL,
    opened_at timestamp with time zone,
    badge bytea,
    note text
);
COMMENT ON TABLE public.hubs IS 'Public linehaul sort hub. capacity_grams is an exact gram count and may sit at the int8 bounds. Coordinates and the opening timestamp may be unknown.';
COMMENT ON COLUMN public.hubs.latitude IS 'WGS84 latitude as numeric(38,4). NULL when the survey is missing.';
COMMENT ON COLUMN public.hubs.capacity_grams IS 'Design sort capacity in grams, including the int8 minimum and maximum.';
COMMENT ON COLUMN public.hubs.badge IS 'Opaque dock badge. Empty, NUL, and 0xff are valid payloads.';

CREATE TABLE public.shipments (
    shipment_id bigint PRIMARY KEY,
    origin_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id),
    dest_hub_id bigint NOT NULL REFERENCES public.hubs (hub_id),
    status text NOT NULL,
    transport_mode text NOT NULL,
    weight_grams bigint,
    cost_amount numeric(38,4),
    created_at timestamp with time zone NOT NULL,
    promised_at timestamp with time zone,
    seal bytea,
    comment text,
    CONSTRAINT shipments_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed')),
    CONSTRAINT shipments_mode_ck CHECK (transport_mode IN ('road', 'rail', 'air', 'sea'))
);
COMMENT ON TABLE public.shipments IS 'One contracted movement from an origin hub to a different destination hub.';
COMMENT ON COLUMN public.shipments.cost_amount IS 'Customer charge as numeric(38,4). NULL when rating has not posted. Not a float.';
COMMENT ON COLUMN public.shipments.status IS 'delivered, in_transit, or delayed.';

CREATE TABLE public.tracking_events (
    event_id bigint PRIMARY KEY,
    shipment_id bigint NOT NULL REFERENCES public.shipments (shipment_id),
    hub_id bigint REFERENCES public.hubs (hub_id),
    status text NOT NULL,
    event_at timestamp with time zone,
    recorded_weight_grams bigint,
    payload bytea,
    comment text,
    CONSTRAINT tracking_status_ck CHECK (status IN ('delivered', 'in_transit', 'delayed'))
);
COMMENT ON TABLE public.tracking_events IS 'Scan history for a shipment. The scan hub and the event time may be unknown.';
COMMENT ON COLUMN public.tracking_events.payload IS 'Reader payload. NULL, empty, and raw bytes are distinct.';
"#
}

fn write_lines(
    copy_path: &Path,
    canonical_path: &Path,
    rows: i64,
    copy_row: fn(i64) -> String,
    canonical_row: fn(i64) -> String,
) -> Result<String, String> {
    let copy_file = fs::File::create(copy_path).map_err(|error| error.to_string())?;
    let canon_file = fs::File::create(canonical_path).map_err(|error| error.to_string())?;
    let mut copy_writer = BufWriter::new(copy_file);
    let mut canon_writer = BufWriter::new(canon_file);
    let mut hasher = crate::sha256::Hasher::new();
    for id in 1..=rows {
        let copy_line = copy_row(id);
        let canon_line = canonical_row(id);
        writeln!(copy_writer, "{copy_line}").map_err(|error| error.to_string())?;
        writeln!(canon_writer, "{canon_line}").map_err(|error| error.to_string())?;
        hasher.update(canon_line.as_bytes());
        hasher.update(b"\n");
    }
    copy_writer.flush().map_err(|error| error.to_string())?;
    canon_writer.flush().map_err(|error| error.to_string())?;
    Ok(hasher.finish_hex())
}

fn hash_rows(hasher: &mut crate::sha256::Hasher, rows: i64, row: fn(i64) -> String) {
    for id in 1..=rows {
        let line = row(id);
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
}


