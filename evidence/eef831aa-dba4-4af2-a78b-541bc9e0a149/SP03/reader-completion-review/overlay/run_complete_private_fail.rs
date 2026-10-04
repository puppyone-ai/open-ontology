//! Expected compile failure. Callers cannot build `RunComplete` or name the pull plan.
use oo_connectors::{PgTypePolicy, RunComplete, SourceCatalog};

#[test]
fn caller_cannot_construct_run_complete() {
    let catalog = SourceCatalog {
        protocol: String::new(),
        core_contract: String::new(),
        source_id: String::new(),
        database: String::new(),
        captured_at: String::new(),
        schema_hash: String::new(),
        hash_algorithm: String::new(),
        ident_algorithm: String::new(),
        tables: Vec::new(),
        type_conversion: String::new(),
    };
    let _token = RunComplete {
        source_id: String::new(),
        generation: 1,
        config_identity: [0_u8; 32],
        type_policy_identity: [0_u8; 32],
        policy: PgTypePolicy::PRESERVE_ROW,
        input_digest: [0_u8; 32],
        schema_hash: String::new(),
        catalog,
        selected: Vec::new(),
        seal: (),
    };
    let _also = RunComplete::new();
}

#[test]
fn caller_cannot_name_stream_plan() {
    let _plan = oo_connectors::stream::StreamPlan;
}
