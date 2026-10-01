const SCHEMA_TEXT: &str = include_str!("../../../contracts/evidence/evidence-v1.json");

fn main() {
    assert!(SCHEMA_TEXT.contains("snapshot-include-v1"));
}
