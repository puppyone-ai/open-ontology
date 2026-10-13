use std::fs;
use std::io::{BufRead, BufReader};

use logistics_product_loop::{
    answers_absent_from_questions, badge, capacity_grams, cells, cost_coeff, cost_text, delayed_air_count, delivered_origin_cost,
    dest_hub, event_hub, event_shipment, judge, null_cost_count, opened_at, origin_hub, parse, prompt_leaks, questions,
    shipment_status, unix_micros, write_fixture, write_gold, EVENT_ROWS, EXACT_COST, HUB_ROWS, SHIPMENT_ROWS,
};

#[test]
fn counts_relationships_and_edges_are_deterministic() {
    assert_eq!(HUB_ROWS + SHIPMENT_ROWS + EVENT_ROWS, 100_000);
    assert_eq!(capacity_grams(1), i64::MIN);
    assert_eq!(capacity_grams(2), i64::MAX);
    assert_eq!(cost_text(1).as_deref(), Some(EXACT_COST));
    assert_eq!(cost_text(2).as_deref(), Some("-0.0001"));
    assert_eq!(cost_text(7001).as_deref(), Some("9007199254740993.0001"));
    assert_eq!(cost_text(17), None);
    assert_eq!(badge(1).as_deref(), Some(&[][..]));
    assert_eq!(badge(2).as_deref(), Some(&[0x00][..]));
    assert_eq!(badge(3).as_deref(), Some(&[0xff][..]));
    assert_eq!(badge(4).as_deref(), Some(&[0x00, 0x01, 0xff][..]));
    assert_eq!(badge(19), None);
    assert_eq!(unix_micros("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(unix_micros("2000-01-01T00:00:00Z"), Some(946_684_800_000_000));
    for stamp in [opened_at(1), opened_at(2), opened_at(4), opened_at(5), opened_at(6)] {
        let text = stamp.expect("edge timestamp");
        assert!(unix_micros(&text).is_some(), "{text}");
    }
    assert_eq!(opened_at(3), None);
    for id in 1..=SHIPMENT_ROWS {
        let origin = origin_hub(id);
        let dest = dest_hub(id);
        assert!((1..=HUB_ROWS).contains(&origin) && (1..=HUB_ROWS).contains(&dest) && origin != dest);
    }
    for id in 1..=64 {
        assert!((1..=SHIPMENT_ROWS).contains(&event_shipment(id)));
        assert!(event_hub(id).is_none_or(|hub| (1..=HUB_ROWS).contains(&hub)));
    }
    let mut modulus = 0_i64;
    for id in 1..=SHIPMENT_ROWS {
        if id % 12 == 2 {
            modulus += 1;
        }
    }
    assert_eq!(delayed_air_count(), modulus);
    assert_eq!(delayed_air_count(), 667);
    assert_eq!(null_cost_count(), 470);
    assert_eq!(delivered_origin_cost(), "6084.2588");
    let mut manual = 0_i128;
    for id in [42_i64, 6042] {
        assert_eq!(origin_hub(id), 42);
        assert_eq!(shipment_status(id), "delivered");
        manual += cost_coeff(id).expect("cost");
    }
    assert_eq!(logistics_product_loop::format_scale4(manual), "6084.2588");
}

#[test]
fn questions_hide_sql_and_answers() {
    assert!(answers_absent_from_questions());
    assert_eq!(questions().len(), 6);
    for question in questions() {
        assert!(!prompt_leaks(question.text), "{}", question.id);
        let expected = cells(question.id).expect("cells");
        for row in &expected {
            for cell in row {
                if cell.len() >= 3 {
                    assert!(!question.text.contains(cell), "{} leaks {cell}", question.id);
                }
            }
        }
    }
    assert_eq!(cells("event-count").unwrap()[0][0], "90000");
    assert_eq!(cells("hub-capacity-bounds").unwrap()[0], vec![i64::MIN.to_string(), i64::MAX.to_string()]);
}

#[test]
fn generated_files_keep_the_oracle_out_of_the_question_file() {
    let dir = std::env::temp_dir().join(format!("oo-a419-oracle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    write_fixture(&dir).expect("fixture");
    write_gold(&dir).expect("gold");
    let questions = fs::read_to_string(dir.join("questions.json")).expect("questions");
    let oracle = fs::read_to_string(dir.join("oracle.json")).expect("oracle");
    assert!(questions.contains("\"sql_included\":false"));
    assert!(!questions.contains("SELECT"));
    assert!(!questions.contains(EXACT_COST));
    assert!(oracle.contains("SELECT count(*)::text FROM public.tracking_events"));
    assert!(oracle.contains("not_a_prompt"));
    let schema = fs::read_to_string(dir.join("schema.sql")).expect("schema");
    assert!(schema.contains("numeric(38,4)"));
    assert!(schema.contains("REFERENCES public.hubs"));
    assert!(schema.contains("linehaul"));
    let hubs = BufReader::new(fs::File::open(dir.join("hubs.copy")).expect("hubs"));
    assert_eq!(hubs.lines().count(), 2000);
    let events = BufReader::new(fs::File::open(dir.join("tracking_events.copy")).expect("events"));
    assert_eq!(events.lines().count(), 90000);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_observation_stays_pending_and_does_not_approve_review() {
    let verdict = judge(None);
    assert_eq!(verdict.exit_code, 4);
    let text = logistics_product_loop::verdict_text(&verdict);
    assert!(text.contains("\"short_goal\":\"pending\""));
    assert!(text.contains("\"formal_s9\":\"notdone\""));
    assert!(text.contains("\"formal_s10\":\"notdone\""));
    assert!(text.contains("\"parent_plan_acceptance\":false"));
}

#[test]
fn synthetic_measurement_cannot_pass_and_a_wrong_cell_fails() {
    let live = complete_observation("live", false, false);
    let parsed = parse(&live).expect("parse");
    let verdict = judge(Some(&parsed));
    assert_eq!(verdict.exit_code, 0, "{}", logistics_product_loop::verdict_text(&verdict));
    let synthetic = parse(&complete_observation("synthetic", false, false)).expect("parse");
    let held = judge(Some(&synthetic));
    assert_eq!(held.exit_code, 4);
    assert!(logistics_product_loop::verdict_text(&held).contains("\"logic_pass\":true"));
    let wrong = parse(&complete_observation("live", true, false)).expect("parse");
    let failed = judge(Some(&wrong));
    assert_eq!(failed.exit_code, 1);
    let forged = parse(&complete_observation("live", false, true)).expect("parse");
    let fake = judge(Some(&forged));
    assert_eq!(fake.exit_code, 1);
    assert!(logistics_product_loop::verdict_text(&fake).contains("fake_approval"));
}

#[test]
fn an_explicit_source_block_failure_fails_and_a_missing_measurement_stays_pending() {
    let leaked = complete_observation("live", false, false).replace("\"source_unreachable_after_seal\":true", "\"source_unreachable_after_seal\":false");
    let leaked = parse(&leaked).expect("parse");
    assert_eq!(judge(Some(&leaked)).exit_code, 1);
    let unmeasured = complete_observation("live", false, false).replace("\"source_unreachable_after_seal\":true,", "");
    let unmeasured = parse(&unmeasured).expect("parse");
    assert_eq!(judge(Some(&unmeasured)).exit_code, 4);
    let partial_fault = complete_observation("live", false, false).replace("\"bad_connection_exit\":1,", "");
    let partial_fault = parse(&partial_fault).expect("parse");
    assert_eq!(judge(Some(&partial_fault)).exit_code, 4);
}

fn complete_observation(provenance: &str, wrong_cell: bool, fake_review: bool) -> String {
    let mut questions_json = String::from("[");
    for (index, question) in questions().iter().enumerate() {
        if index > 0 {
            questions_json.push(',');
        }
        let mut rows = cells(question.id).expect("cells");
        if wrong_cell && question.id == "event-count" {
            rows[0][0] = "1".to_owned();
        }
        questions_json.push_str(&format!(
            "{{\"id\":\"{}\",\"read_before_sql\":true,\"bash_wrote_sql\":true,\"sql_prewritten\":false,\"query_id\":\"q-{}\",\"cited\":true,\"cited_query_id\":\"q-{}\",\"audit_http\":200,\"audit_database\":\"oo_a419_dst\",\"audit_generation\":\"2\",\"gateway\":\"query-pg\",\"proxy_mtls\":true,\"direct_select\":false,\"cells\":{}}}",
            question.id,
            question.id,
            question.id,
            cells_json(&rows)
        ));
    }
    questions_json.push(']');
    let approved = if fake_review { "true" } else { "false" };
    format!(
        r#"{{
          "protocol":"oo.logistics-product-loop.observation/v1",
          "provenance":"{provenance}",
          "identities":{{"source_system_identifier":"src-1","target_system_identifier":"dst-2","source_database":"oo_a419_src","target_database":"oo_a419_dst"}},
          "run":{{"copy_performed":true,"target_was_preloaded":false,"source_tx":"repeatable_read_read_only","source_peak":1,"import_hash":"{hash}","snapshot_id":"snap-1","mapping_generation":"2","source_unreachable_after_seal":true,"source_access_after_seal":0,"source_secret_withheld":true}},
          "typed":{{"null_preserved":true,"int8_bounds":true,"numeric_exact":true,"timestamp_edges":true,"bytea_edges":true,"tables":[
            {{"schema":"public","name":"hubs","rows":2000,"distinct_keys":2000,"hash_equal":true}},
            {{"schema":"public","name":"shipments","rows":8000,"distinct_keys":8000,"hash_equal":true}},
            {{"schema":"public","name":"tracking_events","rows":90000,"distinct_keys":90000,"hash_equal":true}}
          ]}},
          "copy_fault":{{"source_writes":0,"partial_visible":false,"snapshot_splice":false,"duplicate_batch":false,"pseudo_complete":false}},
          "model":{{"fresh_requests":1,"cache_second_build_requests":0,"cache_hit":true,"changed_input_invalidates":true,"outcome":"validated","model_id":"grok-4.7","failure_exit":1,"failure_ready":false,"failure_published":false}},
          "pack":{{"unreviewed":true,"not_published":true,"native_parse":true}},
          "faults":{{"bad_connection_exit":1,"bad_connection_ready":false,"model_failure_exit":1,"model_failure_ready":false,"write_rejected":true,"source_unchanged_after_write":true,"cancel_exit":1,"released_ask_exit":1,"processes_absent":true,"proxy_absent":true,"data_persistent":true}},
          "repro":{{"secret_free_template":true,"command_documented":true,"clean_start":true,"second_run_cache_only":true}},
          "stages":{{"S0":{{"executed":true,"approved":false,"published":false}},"S1":{{"executed":true,"approved":false,"published":false}},"S5":{{"executed":true,"approved":false,"published":false}},"S9":{{"executed":false,"approved":{approved},"published":false}},"S10":{{"executed":false,"approved":false,"published":false}}}},
          "questions":{questions_json}
        }}"#,
        hash = "a".repeat(64),
    )
}

fn cells_json(rows: &[Vec<String>]) -> String {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('[');
        for (cell_index, cell) in row.iter().enumerate() {
            if cell_index > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(cell);
            out.push('"');
        }
        out.push(']');
    }
    out.push(']');
    out
}
