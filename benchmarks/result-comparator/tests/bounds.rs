use oo_result_comparator::compare_document;

fn side(kind_body: &str) -> String {
    format!(
        r#"{{"version":"oo.benchmark.result-comparison/v1","row_comparator":"ordered","expected":{kind_body},"actual":{kind_body}}}"#
    )
}

fn rows_body(rows: &str) -> String {
    format!(
        r#"{{"kind":"rows","completion":"complete","warnings":[],"columns":[{{"name":"k","type":"int64"}}],"rows":[{rows}]}}"#
    )
}

fn token(input: &str) -> (String, Option<String>) {
    let error = compare_document(input).expect_err("config");
    (error.token().to_owned(), error.pointer().map(str::to_owned))
}

#[test]
fn duplicate_key_is_json() {
    let input = r#"{"version":"oo.benchmark.result-comparison/v1","version":"oo.benchmark.result-comparison/v1"}"#;
    assert_eq!(token(input), ("json".to_owned(), None));
}

#[test]
fn set_comparator_is_rejected() {
    let input = r#"{"version":"oo.benchmark.result-comparison/v1","row_comparator":"set","expected":{"kind":"not_found","completion":"complete","warnings":[]},"actual":{"kind":"not_found","completion":"complete","warnings":[]}}"#;
    assert_eq!(
        token(input),
        ("schema".to_owned(), Some("/row_comparator".to_owned()))
    );
}

#[test]
fn json_number_coefficient_is_schema() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"amount","type":"decimal"}],"rows":[[{"presence":"value","negative":false,"coefficient":120,"scale":2}]]}"#;
    assert_eq!(
        token(&side(body)),
        (
            "schema".to_owned(),
            Some("/expected/rows/0/0/coefficient".to_owned())
        )
    );
}

#[test]
fn fractional_scale_is_decimal() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"amount","type":"decimal"}],"rows":[[{"presence":"value","negative":false,"coefficient":"120","scale":2.0}]]}"#;
    assert_eq!(
        token(&side(body)),
        (
            "decimal".to_owned(),
            Some("/expected/rows/0/0/scale".to_owned())
        )
    );
}

#[test]
fn int64_extremes_outside_the_range_are_config() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"k","type":"int64"}],"rows":[[{"presence":"value","value":"9223372036854775808"}]]}"#;
    assert_eq!(
        token(&side(body)),
        (
            "int64".to_owned(),
            Some("/expected/rows/0/0/value".to_owned())
        )
    );
    let negative = body.replace("9223372036854775808", "-9223372036854775809");
    assert_eq!(token(&side(&negative)).0, "int64");
}

#[test]
fn int64_json_number_and_noncanonical_spellings_are_refused() {
    let number = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"k","type":"int64"}],"rows":[[{"presence":"value","value":1}]]}"#;
    assert_eq!(token(&side(number)).0, "schema");
    for spelling in ["01", "+1", "-0", "1.0", "1e2"] {
        let body = format!(
            r#"{{"kind":"rows","completion":"complete","warnings":[],"columns":[{{"name":"k","type":"int64"}}],"rows":[[{{"presence":"value","value":"{spelling}"}}]]}}"#
        );
        assert_eq!(token(&side(&body)).0, "int64", "{spelling}");
    }
}

#[test]
fn leading_zero_coefficient_is_decimal() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"amount","type":"decimal"}],"rows":[[{"presence":"value","negative":false,"coefficient":"00","scale":0}]]}"#;
    assert_eq!(token(&side(body)).0, "decimal");
}

#[test]
fn negative_zero_decimal_is_refused() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"amount","type":"decimal"}],"rows":[[{"presence":"value","negative":true,"coefficient":"0","scale":2}]]}"#;
    assert_eq!(
        token(&side(body)),
        (
            "decimal".to_owned(),
            Some("/expected/rows/0/0/negative".to_owned())
        )
    );
}

#[test]
fn not_found_rejects_a_row_payload() {
    let body = r#"{"kind":"not_found","completion":"complete","warnings":[],"rows":[]}"#;
    assert_eq!(
        token(&side(body)),
        ("schema".to_owned(), Some("/expected/rows".to_owned()))
    );
}

#[test]
fn short_row_is_schema() {
    let body = r#"{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"k","type":"int64"},{"name":"n","type":"text"}],"rows":[[{"presence":"null"}]]}"#;
    assert_eq!(
        token(&side(body)),
        ("schema".to_owned(), Some("/expected/rows/0".to_owned()))
    );
}

#[test]
fn too_many_rows_is_budget() {
    let row = r#"[{"presence":"value","value":"1"}]"#;
    let rows = vec![row; 2001].join(",");
    assert_eq!(
        token(&side(&rows_body(&rows))),
        ("budget".to_owned(), Some("/expected/rows".to_owned()))
    );
}

#[test]
fn payload_byte_cap_is_budget() {
    let cell = format!(r#"[{{"presence":"value","value":"{}"}}]"#, "a".repeat(500));
    let rows = vec![cell; 524].join(",");
    let body = format!(
        r#"{{"kind":"rows","completion":"complete","warnings":[],"columns":[{{"name":"t","type":"text"}}],"rows":[{rows}]}}"#
    );
    assert_eq!(
        token(&side(&body)),
        ("budget".to_owned(), Some("/expected".to_owned()))
    );
}

#[test]
fn document_byte_cap_is_budget() {
    let input = format!("{{{} }}", " ".repeat(1_048_576));
    assert!(input.len() > 1_048_576);
    assert_eq!(token(&input), ("budget".to_owned(), Some("/".to_owned())));
}

#[test]
fn complete_actual_does_not_match_partial_expected() {
    let input = r#"{"version":"oo.benchmark.result-comparison/v1","row_comparator":"ordered","expected":{"kind":"not_found","completion":"partial","warnings":[]},"actual":{"kind":"not_found","completion":"complete","warnings":[]}}"#;
    let comparison = compare_document(input).expect("report");
    assert!(!comparison.matched);
    assert!(comparison.report.contains("\"reason\":\"completion\""));
    assert!(comparison.report.contains("\"expected\":\"partial\""));
    assert!(comparison.report.contains("\"actual\":\"complete\""));
}

#[test]
fn unavailable_does_not_match_null() {
    let input = r#"{"version":"oo.benchmark.result-comparison/v1","row_comparator":"ordered","expected":{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"k","type":"text"}],"rows":[[{"presence":"null"}]]},"actual":{"kind":"rows","completion":"complete","warnings":[],"columns":[{"name":"k","type":"text"}],"rows":[[{"presence":"unavailable"}]]}}"#;
    let comparison = compare_document(input).expect("report");
    assert!(!comparison.matched);
    assert!(comparison.report.contains("\"reason\":\"presence\""));
}

#[test]
fn failed_does_not_match_complete() {
    let input = r#"{"version":"oo.benchmark.result-comparison/v1","row_comparator":"ordered","expected":{"kind":"not_found","completion":"complete","warnings":[]},"actual":{"kind":"not_found","completion":"failed","warnings":[]}}"#;
    let comparison = compare_document(input).expect("report");
    assert!(!comparison.matched);
    assert!(comparison.report.contains("\"actual\":\"failed\""));
}
