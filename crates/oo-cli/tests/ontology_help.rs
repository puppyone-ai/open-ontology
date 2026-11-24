//! `oo ontology --help` must show the Agent commands (search, entity, query, schema, sources) and the owner's
//! export commands, offline and without credentials. An Agent starts from this help.
#![cfg(feature = "docs")]

use std::process::Command;

fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(args)
        .env("HOME", "/nonexistent/oo-help-must-not-read-home")
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn ontology_help_lists_the_agent_commands_and_the_owner_exports() {
    for args in [vec!["ontology", "--help"], vec!["help", "ontology"]] {
        let text = help(&args);
        for want in [
            "oo ontology search <text>",
            "oo ontology entity <urn>",
            "oo ontology query",
            "oo ontology schema",
            "oo ontology sources",
            "OO_AGENT_TOKEN_FILE",
            "oo ontology export --format",
        ] {
            assert!(text.contains(want), "{args:?} lacks {want:?}");
        }
    }
}

#[test]
fn ontology_subcommand_help_stays_the_agent_help() {
    let text = help(&["ontology", "query", "--help"]);
    assert!(text.contains("Examples:") && text.contains("public.orders"));
}

#[test]
fn top_level_help_points_an_agent_at_the_ontology_commands() {
    let text = help(&["--help"]);
    assert!(text.contains("oo ontology search|entity|query|schema|sources"));
    assert!(text.contains("oo agent token create|ls|revoke"));
}
