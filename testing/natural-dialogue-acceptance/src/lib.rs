//! Public checks for one natural question.
//!
//! The product crate owns the judge. This package calls that public API and
//! reads the frozen prompt card. It does not accept the parent goal.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use minimal_product_loop::{ask, bulk_questions, expected_cells, prompt_for, question_text_for};

    const ASK_RS: &str = include_str!("../../../tools/minimal-product-loop/src/ask.rs");
    const MAIN_RS: &str = include_str!("../../../tools/minimal-product-loop/src/main.rs");
    const CARD: &str = include_str!("../../minimal-loop-acceptance/BULK-QUESTIONS.md");

    #[test]
    fn frozen_prompts_match_the_public_card_and_hide_answers() {
        let ids = [
            "count-batch-c",
            "id-overlap",
            "amount-when-qty-above",
            "null-amount",
            "batch-a-amount",
            "batch-c-qty-bounds",
        ];
        assert_eq!(bulk_questions().len(), ids.len());
        for (question, id) in bulk_questions().iter().zip(ids) {
            assert_eq!(question.id, id);
            assert!(CARD.contains(question.text), "{id}");
            assert_eq!(question_text_for("bulk", id), Some(question.text));
            assert!(expected_cells(id).is_some(), "{id}");
            let prompt = prompt_for(
                question.text,
                Path::new("/usr/local/bin/oo"),
                Path::new("/knowledge"),
                Path::new("/secrets/token"),
                "http://127.0.0.1:9",
            );
            assert!(!prompt.contains("SELECT"), "{id}");
            assert!(!prompt.contains("90000"), "{id}");
            assert!(!prompt.contains("1234567890"), "{id}");
            assert!(prompt.contains("query_id"), "{id}");
            assert!(prompt.contains("read tool"), "{id}");
            assert!(prompt.contains("bash tool"), "{id}");
            assert!(prompt.contains("public proxy marker"), "{id}");
            assert!(prompt.contains(question.text), "{id}");
        }
        assert_eq!(expected_cells("count-batch-c").expect("count")[0][0].as_str(), Some("90000"));
        assert_eq!(expected_cells("id-overlap").expect("overlap")[0][0].as_str(), Some("2000"));
        assert_eq!(expected_cells("amount-when-qty-above").expect("sum")[0][0].as_str(), Some("134997.0000"));
        assert_eq!(expected_cells("null-amount").expect("nulls")[0][0].as_str(), Some("1"));
        assert!(expected_cells("binary-row").is_none());
        assert!(question_text_for("bulk", "binary-row").expect("legacy").contains("blob"));
        assert_eq!(question_text_for("bulk", "decimal-row"), question_text_for("bulk", "batch-a-amount"));
        assert!(!bulk_questions().iter().any(|item| item.id == "binary-row" || item.id == "qty-above"));
        assert!(question_text_for("shop", "count-entry").is_some());
        assert!(question_text_for("bulk", "count-entry").is_none());
        assert!(question_text_for("configured", "count-batch-c").is_none());
    }

    #[test]
    fn product_source_cites_the_audited_query_and_does_not_read_gold_cells() {
        assert!(!ASK_RS.contains("cells_match"));
        assert!(ASK_RS.contains("citation.ambiguous"));
        assert!(ASK_RS.contains("citation.absent"));
        assert!(ASK_RS.contains("tools.absent"));
        assert!(ASK_RS.contains("tools.incomplete"));
        assert!(ASK_RS.contains("turn.absent"));
        assert!(ASK_RS.contains("{\"ask_status\": status}"));
        assert!(ASK_RS.contains("Some(\"draft_ready\") | None"));
        assert!(MAIN_RS.contains("outcome.cited"));
        assert!(MAIN_RS.contains("outcome.matched"));
        assert!(MAIN_RS.contains("!outcome.query_id.is_empty()"));
    }

    #[test]
    fn released_closed_and_failure_refuse_without_a_map_or_a_reopen() {
        for status in ["released", "closed", "failure"] {
            let dir = scratch(status);
            let config = write_config(&dir);
            let work = dir.join("work");
            fs::create_dir_all(&work).expect("work");
            fs::write(
                work.join("state.json"),
                format!(r#"{{"status":"{status}","selfbuild":"generated_unreviewed","question_set":"bulk"}}"#),
            )
            .expect("state");
            let error = ask(&config, "count-batch-c").expect_err(status);
            assert_eq!(error.code, "ask.refused", "{status}");
            assert!(!work.join("proxy-session-map.json").exists(), "{status}");
            let state = fs::read_to_string(work.join("state.json")).expect("state");
            assert!(state.contains(&format!("\"status\":\"{status}\"")), "{status}");
            assert!(!state.contains("draft_ready"), "{status}");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn arbitrary_text_aliases_and_traversal_stop_before_pi() {
        let dir = scratch("open");
        let config = write_config(&dir);
        let work = dir.join("work");
        fs::create_dir_all(&work).expect("work");
        fs::write(
            work.join("state.json"),
            r#"{"status":"draft_ready","selfbuild":"generated_unreviewed","question_set":"bulk"}"#,
        )
        .expect("state");
        let natural = "How many rows are visible in the sealed public batch c table?";
        let error = ask(&config, natural).expect_err("natural");
        assert_eq!(error.code, "pi.absent");
        let again = ask(&config, "今天密封表里有多少行？").expect_err("utf8");
        assert_eq!(again.code, "pi.absent");
        let frozen = question_text_for("bulk", "count-batch-c").expect("frozen");
        let alias = ask(&config, frozen).expect_err("alias");
        assert_eq!(alias.code, "pi.absent");
        let by_id = ask(&config, "count-batch-c").expect_err("id");
        assert_eq!(by_id.code, "pi.absent");
        let legacy = ask(&config, "decimal-row").expect_err("legacy");
        assert_eq!(legacy.code, "pi.absent");
        let unknown = ask(&config, "not-a-question").expect_err("unknown");
        assert_eq!(unknown.code, "ask.question");
        let shop_only = ask(&config, "count-entry").expect_err("shop");
        assert_eq!(shop_only.code, "ask.question");
        let traversal = ask(&config, "../etc/passwd").expect_err("path");
        assert_eq!(traversal.code, "pi.absent");
        let dotted = ask(&config, "nl-../x").expect_err("dot");
        assert_eq!(dotted.code, "pi.absent");
        assert_eq!(ask(&config, " ").expect_err("blank").code, "ask.question");
        assert_eq!(ask(&config, &"a".repeat(2_001)).expect_err("long").code, "ask.question");
        assert_eq!(ask(&config, "a\0b").expect_err("nul").code, "ask.question");
        assert!(!work.join("proxy-session-map.json").exists());
        assert!(!work.join("asks").exists());
        let state = fs::read_to_string(work.join("state.json")).expect("state");
        assert!(state.contains("\"status\":\"draft_ready\""));
        assert!(!dir.join("etc").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_status_still_reports_pi_absent_for_the_shop_fixture() {
        let dir = scratch("none");
        let config = write_config(&dir);
        let work = dir.join("work");
        fs::create_dir_all(&work).expect("work");
        fs::write(work.join("state.json"), r#"{"selfbuild":"generated_unreviewed"}"#).expect("state");
        let error = ask(&config, "count-entry").expect_err("pi");
        assert_eq!(error.code, "pi.absent");
        assert!(!work.join("proxy-session-map.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oo-a416-accept-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn write_config(dir: &Path) -> PathBuf {
        fs::write(dir.join("password"), b"pppppppp").expect("password");
        fs::write(dir.join("ca.pem"), b"ca-bytes\n").expect("ca");
        fs::write(dir.join("bearer"), b"bbbbbbbb").expect("bearer");
        let config = dir.join("loop.json");
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let body = format!(
            "{{\n\
               \"protocol\": \"oo.minimal-product-loop.config/v1\",\n\
               \"mode\": \"operator-development\",\n\
               \"development_preview\": true,\n\
               \"publication\": \"not_published\",\n\
               \"llm_review\": \"unreviewed\",\n\
               \"runtime_acknowledgement\": \"local-process-not-a-sandbox-certificate\",\n\
               \"database\": {{\"type\": \"postgresql\", \"host\": \"127.0.0.1\", \"port\": 9, \"name\": \"mirrordb\", \"username\": \"reader\", \"tls\": \"require\", \"secret_ref\": \"secret://pg/a416/reader\", \"password_file\": \"{password}\", \"ca_file\": \"{ca}\"}},\n\
               \"tables\": [{{\"schema\": \"public\", \"table\": \"batch_c\"}}],\n\
               \"llm\": {{\"model_id\": \"grok-4.7\", \"origin\": \"http://127.0.0.1:8317\", \"path\": \"/v1/chat/completions\", \"bearer_file\": \"{bearer}\", \"deadline_ms\": 30000}},\n\
               \"gateway\": {{\"oo\": \"{oo}\", \"query_pg\": \"{query}\", \"policy_file\": \"{policy}\", \"token_file\": \"{token}\"}},\n\
               \"pi\": {{\"executable\": \"{pi}\", \"executable_sha256\": \"{sha}\", \"runtime\": \"{node}\", \"runtime_sha256\": \"{sha}\", \"provider\": \"grok\", \"model\": \"grok-4.7\"}},\n\
               \"paths\": {{\"catalog_bin\": \"{catalog}\", \"knowledge\": \"{knowledge}\", \"work\": \"{work}\"}},\n\
               \"operator\": {{\"label\": \"a416\", \"at\": \"2026-10-09T00:00:00Z\"}}\n\
             }}\n",
            password = dir.join("password").display(),
            ca = dir.join("ca.pem").display(),
            bearer = dir.join("bearer").display(),
            oo = dir.join("missing-oo").display(),
            query = dir.join("missing-query-pg").display(),
            policy = dir.join("policy.json").display(),
            token = dir.join("token").display(),
            pi = dir.join("missing-pi").display(),
            node = dir.join("missing-node").display(),
            catalog = dir.join("missing-catalog").display(),
            knowledge = dir.join("knowledge").display(),
            work = dir.join("work").display(),
        );
        fs::write(&config, body).expect("config");
        config
    }
}
