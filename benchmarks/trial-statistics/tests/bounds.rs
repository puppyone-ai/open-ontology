use std::io::Cursor;

use oo_trial_statistics::{DOCUMENT_MAX_BYTES, evaluate, run};

fn tuple(model: &str, project: &str, as_of: &str) -> String {
    format!(
        concat!(
            r#"{{"as_of":"{as_of}","budget":"tokens-8000","dataset":"public-slice","#,
            r#""harness":"harness-0","model":"{model}","project":"{project}","#,
            r#""role":"analyst","scorer_version":"scorer-1","#,
            r#""snapshot":"snap-2026-10-01","suite":"paired-k3"}}"#
        ),
        as_of = as_of,
        model = model,
        project = project,
    )
}

fn question(id: &str, statuses: &[&str]) -> String {
    let mut trials = String::new();
    for (index, status) in statuses.iter().enumerate() {
        if index > 0 {
            trials.push(',');
        }
        trials.push_str(&format!(r#"{{"index":{index},"status":"{status}"}}"#));
    }
    format!(r#"{{"id":"{id}","trials":[{trials}]}}"#)
}

fn questions(count: usize, id_len: usize, trials: usize) -> String {
    let mut body = String::new();
    for index in 0..count {
        if index > 0 {
            body.push(',');
        }
        let prefix = format!("q{index}");
        let id = format!("{prefix}{}", "a".repeat(id_len - prefix.len()));
        let statuses = vec!["pass"; trials];
        body.push_str(&question(&id, &statuses));
    }
    body
}

fn variant(id: &str, model: &str, project: &str, as_of: &str, questions: &str) -> String {
    format!(
        r#"{{"id":"{id}","questions":[{questions}],"tuple":{tuple}}}"#,
        tuple = tuple(model, project, as_of),
    )
}

struct Side<'a> {
    id: &'a str,
    model: &'a str,
    project: &'a str,
    as_of: &'a str,
    questions: &'a str,
}

fn document(
    resamples: &str,
    seed: &str,
    left_id: &str,
    right_id: &str,
    left_questions: &str,
    right_questions: &str,
) -> String {
    document_tuple(
        resamples,
        seed,
        Side {
            id: left_id,
            model: "model-fixed",
            project: "logistics",
            as_of: "2026-10-01",
            questions: left_questions,
        },
        Side {
            id: right_id,
            model: "model-fixed",
            project: "logistics",
            as_of: "2026-10-01",
            questions: right_questions,
        },
    )
}

fn document_tuple(resamples: &str, seed: &str, left: Side<'_>, right: Side<'_>) -> String {
    let left_body = variant(
        left.id,
        left.model,
        left.project,
        left.as_of,
        left.questions,
    );
    let right_body = variant(
        right.id,
        right.model,
        right.project,
        right.as_of,
        right.questions,
    );
    format!(
        concat!(
            r#"{{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","resamples":{resamples},"#,
            r#""seed":"{seed}","variants":[{left},{right}],"#,
            r#""version":"oo.benchmark.trial-statistics/v1"}}"#
        ),
        resamples = resamples,
        seed = seed,
        left = left_body,
        right = right_body,
    )
}

fn execute(text: &str) -> (i32, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        std::iter::empty::<String>(),
        Cursor::new(text.as_bytes()),
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8(stdout).expect("stdout"),
        String::from_utf8(stderr).expect("stderr"),
    )
}

fn execute_bytes(bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        std::iter::empty::<String>(),
        Cursor::new(bytes),
        &mut stdout,
        &mut stderr,
    );
    (code, stdout, stderr)
}

fn assert_config(text: &str, stderr: &str) {
    let (code, stdout, err) = execute(text);
    assert_eq!(code, 2, "{err}");
    assert!(stdout.is_empty(), "{stdout}");
    assert_eq!(err, stderr);
}

fn one() -> String {
    question("q0", &["pass"])
}

#[test]
fn help_and_usage_do_not_parse_stdin() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        ["--help".to_owned()],
        Cursor::new(b"not json"),
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    assert!(stderr.is_empty());
    assert!(stdout.starts_with(b"trial-statistics reads"));
    let (code, stdout, stderr) = {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(
            ["--nope".to_owned()],
            Cursor::new(b"{}"),
            &mut stdout,
            &mut stderr,
        );
        (code, stdout, stderr)
    };
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: usage\n");
}

#[test]
fn passed_gate_and_whitespace_are_stable() {
    let q = one();
    let text = document("1", "0", "left", "right", &q, &q);
    let (code, stdout, stderr) = execute(&text);
    assert_eq!(code, 0);
    assert!(stderr.is_empty());
    assert!(stdout.ends_with('\n'));
    let padded = format!(" \n{text}\n");
    let (again, second, stderr) = execute(&padded);
    assert_eq!(again, 0);
    assert!(stderr.is_empty());
    assert_eq!(second, stdout);
}

#[test]
fn question_and_trial_and_resample_bounds() {
    assert_eq!(DOCUMENT_MAX_BYTES, 1_048_576);
    let q = one();
    let at_questions = document(
        "1",
        "0",
        "left",
        "right",
        &questions(256, 4, 1),
        &questions(256, 4, 1),
    );
    let evaluation = evaluate(&at_questions).expect("256 questions");
    assert!(evaluation.gate_passed);
    assert!(evaluation.report.contains("\"question_count\":256"));
    assert!(evaluation.report.contains("\"low_rank\":0"));
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &questions(257, 4, 1),
            &questions(257, 4, 1),
        ),
        "trial-statistics: budget /variants/0/questions\n",
    );

    let k8 = question("q0", &["pass"; 8]);
    let evaluation = evaluate(&document("1", "0", "left", "right", &k8, &k8)).expect("k8");
    assert!(evaluation.report.contains("\"trial_count\":8"));
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &question("q0", &["pass"; 9]),
            &question("q0", &["pass"; 9]),
        ),
        "trial-statistics: budget /variants/0/questions/0/trials\n",
    );
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &questions(1, 4, 0),
            &questions(1, 4, 0),
        ),
        "trial-statistics: budget /variants/0/questions/0/trials\n",
    );

    let evaluation = evaluate(&document("10000", "0", "left", "right", &q, &q)).expect("10000");
    assert!(evaluation.report.contains("\"resamples\":10000"));
    assert!(evaluation.report.contains("\"low_rank\":250"));
    assert!(evaluation.report.contains("\"high_rank\":9750"));
    assert_config(
        &document("10001", "0", "left", "right", &q, &q),
        "trial-statistics: budget /resamples\n",
    );
    assert_config(
        &document("0", "0", "left", "right", &q, &q),
        "trial-statistics: budget /resamples\n",
    );
    assert_config(
        &document("-1", "0", "left", "right", &q, &q),
        "trial-statistics: budget /resamples\n",
    );
    assert_config(
        &document("1.5", "0", "left", "right", &q, &q),
        "trial-statistics: schema /resamples\n",
    );
    assert_config(
        &document("1e3", "0", "left", "right", &q, &q),
        "trial-statistics: schema /resamples\n",
    );
}

#[test]
fn schema_duplicates_unknown_and_bad_spellings() {
    let q = one();
    assert_config(
        r#"{"version":"oo.benchmark.trial-statistics/v1","version":"x"}"#,
        "trial-statistics: json\n",
    );
    let mut unknown = document("1", "0", "left", "right", &q, &q);
    unknown.insert_str(1, r#""note":"x","#);
    assert_config(&unknown, "trial-statistics: schema /note\n");
    assert_config(
        &document("1", "01", "left", "right", &q, &q),
        "trial-statistics: schema /seed\n",
    );
    assert_config(
        &document("1", "18446744073709551616", "left", "right", &q, &q),
        "trial-statistics: schema /seed\n",
    );
    assert_config(
        &document("1", "-1", "left", "right", &q, &q),
        "trial-statistics: schema /seed\n",
    );
    let max_seed = document("1", "18446744073709551615", "left", "right", &q, &q);
    let evaluation = evaluate(&max_seed).expect("max seed");
    assert!(
        evaluation
            .report
            .contains("\"seed\":\"18446744073709551615\"")
    );

    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &question("q0", &["error"]),
            &question("q0", &["error"]),
        ),
        "trial-statistics: schema /variants/0/questions/0/trials/0/status\n",
    );
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &question("q0", &["missing"]),
            &question("q0", &["missing"]),
        ),
        "trial-statistics: schema /variants/0/questions/0/trials/0/status\n",
    );
    assert_config("", "trial-statistics: json\n");
    assert_config("null", "trial-statistics: schema /\n");
    let (code, stdout, stderr) = execute_bytes(&[0xff, 0xfe]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: json\n");
    let (code, stdout, stderr) = execute_bytes(&vec![b' '; DOCUMENT_MAX_BYTES + 1]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: budget /\n");
}

#[test]
fn mixed_tuple_is_not_compared() {
    let q = one();
    assert_config(
        &document_tuple(
            "1",
            "0",
            Side {
                id: "left",
                model: "model-fixed",
                project: "logistics",
                as_of: "2026-10-01",
                questions: &q,
            },
            Side {
                id: "right",
                model: "model-other",
                project: "logistics",
                as_of: "2026-10-01",
                questions: &q,
            },
        ),
        "trial-statistics: tuple /variants/1/tuple/model\n",
    );
    assert_config(
        &document_tuple(
            "1",
            "0",
            Side {
                id: "left",
                model: "model-fixed",
                project: "logistics",
                as_of: "2026-10-01",
                questions: &q,
            },
            Side {
                id: "right",
                model: "model-fixed",
                project: "logistics",
                as_of: "2026-10-01T00:00:00Z",
                questions: &q,
            },
        ),
        "trial-statistics: tuple /variants/1/tuple/as_of\n",
    );
    assert_config(
        &document_tuple(
            "1",
            "0",
            Side {
                id: "left",
                model: "model-other",
                project: "other",
                as_of: "2026-10-01",
                questions: &q,
            },
            Side {
                id: "right",
                model: "model-fixed",
                project: "logistics",
                as_of: "2026-10-01",
                questions: &q,
            },
        ),
        "trial-statistics: tuple /variants/1/tuple/project\n",
    );
}

#[test]
fn trial_vectors_and_ids_must_line_up() {
    let left = format!(
        "{},{}",
        question("q0", &["pass"; 3]),
        question("q1", &["pass"; 3])
    );
    let short = format!(
        "{},{}",
        question("q0", &["pass"; 3]),
        question("q1", &["fail"; 2])
    );
    assert_config(
        &document("1", "0", "left", "right", &left, &short),
        "trial-statistics: trials /variants/1/questions/1/trials\n",
    );
    let swapped = format!(
        "{},{}",
        question("q1", &["pass"; 3]),
        question("q0", &["pass"; 3])
    );
    assert_config(
        &document("1", "0", "left", "right", &left, &swapped),
        "trial-statistics: schema /variants/1/questions/0/id\n",
    );
    let duplicated = format!(
        "{},{}",
        question("q0", &["pass"]),
        question("q0", &["fail"])
    );
    assert_config(
        &document("1", "0", "left", "right", &duplicated, &duplicated),
        "trial-statistics: schema /variants/0/questions/1/id\n",
    );
    let q = one();
    assert_config(
        &document("1", "0", "left", "left", &q, &q),
        "trial-statistics: schema /variants/1/id\n",
    );
    assert_config(
        &document("1", "0", "left", "right", "", ""),
        "trial-statistics: budget /variants/0/questions\n",
    );
    let bad_index = r#"{"id":"q0","trials":[{"index":1,"status":"pass"}]}"#;
    assert_config(
        &document("1", "0", "left", "right", bad_index, bad_index),
        "trial-statistics: trials /variants/0/questions/0/trials/0/index\n",
    );
    let fraction = r#"{"id":"q0","trials":[{"index":0.0,"status":"pass"}]}"#;
    assert_config(
        &document("1", "0", "left", "right", fraction, fraction),
        "trial-statistics: schema /variants/0/questions/0/trials/0/index\n",
    );
}

#[test]
fn length_and_payload_budgets() {
    let q = one();
    let long_id = format!("l{}", "a".repeat(64));
    assert_config(
        &document("1", "0", &long_id, "right", &q, &q),
        "trial-statistics: budget /variants/0/id\n",
    );
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &question("?", &["pass"]),
            &question("?", &["pass"]),
        ),
        "trial-statistics: schema /variants/0/questions/0/id\n",
    );
    let mut project = "p".to_owned();
    project.push_str(&"a".repeat(128));
    assert_config(
        &document_tuple(
            "1",
            "0",
            Side {
                id: "left",
                model: "model-fixed",
                project: &project,
                as_of: "2026-10-01",
                questions: &q,
            },
            Side {
                id: "right",
                model: "model-fixed",
                project: &project,
                as_of: "2026-10-01",
                questions: &q,
            },
        ),
        "trial-statistics: budget /variants/0/tuple/project\n",
    );
    assert_config(
        &document_tuple(
            "1",
            "0",
            Side {
                id: "left",
                model: "model-fixed",
                project: "log\\nistics",
                as_of: "2026-10-01",
                questions: &q,
            },
            Side {
                id: "right",
                model: "model-fixed",
                project: "log\\nistics",
                as_of: "2026-10-01",
                questions: &q,
            },
        ),
        "trial-statistics: schema /variants/0/tuple/project\n",
    );
    assert_config(
        &document(
            "1",
            "0",
            "left",
            "right",
            &questions(256, 128, 1),
            &questions(256, 128, 1),
        ),
        "trial-statistics: budget /\n",
    );
}
