use std::fs;
use std::path::PathBuf;

fn require_dir(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("{name} is required")))
}

fn fresh(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a411/SP11/minimal-product-loop/linux/fresh-copy",
    );
    fs::read_to_string(path.join(relative)).expect("fixture")
}

#[test]
fn native_parser_accepts_the_real_unreviewed_pack() {
    let dir = require_dir("OO_PACK_OUT");
    let ontology_text = fs::read_to_string(dir.join("ontology/ontology.yaml")).expect("ontology");
    assert!(ontology_text.starts_with("spec_version: oos/v1\n"));
    assert!(!ontology_text.starts_with('{'));
    let ontology = oo_core::oos::parse_ontology_yaml(&ontology_text).expect("ontology");
    assert_eq!(ontology.name.as_str(), "a407bulk");
    assert_eq!(ontology.locale.as_str(), "en-US");
    assert_eq!(ontology.include_schemas.len(), 1);
    assert_eq!(ontology.include_schemas[0].as_str(), "public");
    assert!(ontology.publish.include_unreviewed.is_empty());
    assert!(ontology.publish.require_eval.is_empty());
    assert_eq!(ontology.classification.auto_accept.as_str(), "not_authorized");
    assert_eq!(ontology.compile.sample_values.default, oo_core::config::SampleValues::None);
    assert_eq!(ontology.compile.sample_values.max_per_column, 0);
    assert_eq!(ontology.default_tier_preference, vec![oo_core::oos::Tier::Raw]);
    for (name, rows, columns) in [
        ("batch_a", 2000u64, vec!["id", "amount", "label"]),
        ("batch_b", 8000u64, vec!["id", "at_time", "blob"]),
        ("batch_c", 90000u64, vec!["id", "qty", "amount", "note"]),
    ] {
        let text = fs::read_to_string(dir.join(format!("ontology/generated/datasets/public/{name}.yaml"))).expect(name);
        assert!(text.starts_with("schema_version: oos/v1\n"));
        let dataset = oo_core::oos::parse_dataset_yaml(&text).expect(name);
        assert_eq!(dataset.dataset.as_str(), format!("public.{name}"));
        assert_eq!(dataset.tier, oo_core::oos::Tier::Raw);
        assert_eq!(dataset.row_count_estimate, Some(rows));
        assert_eq!(dataset.recommended, Some(false));
        let names: Vec<&str> = dataset.columns.iter().map(|column| column.name.as_str()).collect();
        assert_eq!(names, columns);
        assert!(dataset.columns.iter().all(|column| {
            column.provenance == Some(oo_core::oos::ColumnProvenance::Generated)
                && column.semantic_type.is_none()
                && column.pii.is_none()
                && column.review.is_none()
                && column.label.is_some()
                && column.description.is_some()
        }));
    }
    let batch_a = oo_core::oos::parse_dataset_yaml(
        &fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_a.yaml")).expect("a"),
    )
    .expect("a");
    assert_eq!(batch_a.label.as_str(), "Batch A");
    assert_eq!(batch_a.columns[0].label.as_ref().expect("label").as_str(), "Identifier");
    assert_eq!(batch_a.columns[1].data_type.as_ref().expect("type").as_str(), "numeric");
}

#[test]
fn native_parser_accepts_the_unicode_writer_output() {
    let dir = require_dir("OO_PACK_UNICODE");
    let text = fs::read_to_string(dir.join("ontology/generated/datasets/public/batch_a.yaml")).expect("yaml");
    let dataset = oo_core::oos::parse_dataset_yaml(&text).expect("unicode");
    assert_eq!(dataset.label.as_str(), "金额 \"客户\"");
    assert_eq!(dataset.columns[2].label.as_ref().expect("label").as_str(), "金额 \"客户\"");
    let ontology = fs::read_to_string(dir.join("ontology/ontology.yaml")).expect("ontology");
    oo_core::oos::parse_ontology_yaml(&ontology).expect("ontology");
}

#[test]
fn native_parser_rejects_duplicate_keys_unknown_fields_and_string_columns() {
    let duplicate = fresh("knowledge/ontology/ontology.yaml");
    let duplicate_err = oo_core::oos::parse_ontology_yaml(&duplicate).expect_err("duplicate");
    let duplicate_text = format!("{duplicate_err:?}");
    assert!(duplicate_text.contains("duplicate"), "{duplicate_text}");
    let string_columns = fresh("knowledge/ontology/generated/datasets/public/batch_a.yaml");
    assert!(oo_core::oos::parse_dataset_yaml(&string_columns).is_err());
    let unknown = "\
spec_version: oos/v1
name: a407bulk
locale: en-US
include_schemas: [public]
default_tier_preference: [raw]
publish:
  include_unreviewed: []
  require_eval: []
classification:
  auto_accept: not_authorized
lifecycle:
  deprecation_min_releases: 1
compile:
  per_role: true
  budgets:
    readme_tokens: 2000
    table_file_tokens: 3000
  sample_values:
    default: none
    max_per_column: 0
review_state: unreviewed
";
    let unknown_err = oo_core::oos::parse_ontology_yaml(unknown).expect_err("review_state");
    let unknown_text = format!("{unknown_err:?}");
    assert!(unknown_text.contains("review_state") || unknown_text.contains("unknown"), "{unknown_text}");
}

#[test]
fn native_parser_accepts_quoted_unicode_and_rejects_a_non_oos_name() {
    let unicode = "\
schema_version: oos/v1
dataset: public.batch_a
label: \"金额 \\\"客户\\\"\"
tier: raw
columns:
  - name: id
    label: \"金额\"
    data_type: int8
    provenance: generated
";
    let dataset = oo_core::oos::parse_dataset_yaml(unicode).expect("unicode");
    assert_eq!(dataset.label.as_str(), "金额 \"客户\"");
    let cafe = "\
schema_version: oos/v1
dataset: public.café
label: \"Cafe\"
tier: raw
";
    assert!(oo_core::oos::parse_dataset_yaml(cafe).is_err());
}

#[test]
fn native_parser_json_document_result_is_recorded() {
    let json = "{\"spec_version\":\"oos/v1\",\"name\":\"a407bulk\",\"locale\":\"en-US\",\"include_schemas\":[\"public\"],\"default_tier_preference\":[\"raw\"],\"publish\":{\"include_unreviewed\":[],\"require_eval\":[]},\"classification\":{\"auto_accept\":\"not_authorized\"},\"lifecycle\":{\"deprecation_min_releases\":1},\"compile\":{\"per_role\":true,\"budgets\":{\"readme_tokens\":2000,\"table_file_tokens\":3000},\"sample_values\":{\"default\":\"none\",\"max_per_column\":0}}}";
    let parsed = oo_core::oos::parse_ontology_yaml(json);
    let record = match &parsed {
        Ok(file) => {
            assert_eq!(file.name.as_str(), "a407bulk");
            "accepted\n".to_owned()
        }
        Err(err) => format!("rejected: {err:?}\n"),
    };
    let path = require_dir_file("OO_PACK_JSON_YAML");
    fs::write(&path, &record).expect("json-as-yaml record");
    assert!(!record.is_empty());
}

fn require_dir_file(name: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("{name} is required")))
}
