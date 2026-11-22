use super::*;
use crate::extract::model::{EntityType, Predicate};
use crate::parse::markdown::parse_markdown;
use crate::parse::text::parse_text;

fn md(text: &str) -> DocIr {
    parse_markdown(text.as_bytes()).expect("md").ir
}

fn txt(text: &str) -> DocIr {
    parse_text(text.as_bytes()).expect("txt").ir
}

fn names(x: &crate::extract::model::Extraction, label: &str) -> Vec<String> {
    let mut v: Vec<String> = x
        .entities
        .iter()
        .filter(|e| e.entity.thing_label.as_deref() == Some(label))
        .map(|e| e.entity.name.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn profiles_by_path_and_format() {
    let p = |path: &str, f: Option<&str>| classify_path(path, f).profile;
    assert_eq!(p("AGENTS.md", Some("md")), Profile::Instruction);
    assert_eq!(p("sub/CLAUDE.md", Some("md")), Profile::Instruction);
    assert_eq!(p(".cursorrules", Some("txt")), Profile::Instruction);
    assert_eq!(p("README.md", Some("md")), Profile::Instruction);
    assert_eq!(
        p("docs/adr/0003-use-postgres.md", Some("md")),
        Profile::Instruction
    );
    assert_eq!(p("docs/STYLE_GUIDE.md", Some("md")), Profile::Instruction);
    assert_eq!(
        p("docs/decisions/0007-queue-limits.md", Some("md")),
        Profile::Instruction
    );
    // The Decisions folder of a business Drive is not an ADR tree.
    assert_eq!(
        p("Decisions/D-2026-01-weekly.md", Some("md")),
        Profile::Business
    );
    assert_eq!(p("CONTRIBUTING.md", Some("md")), Profile::Instruction);
    assert_eq!(p("src/main.rs", Some("txt")), Profile::Code);
    assert_eq!(p("web/app.tsx", Some("txt")), Profile::Code);
    assert_eq!(p("package.json", Some("txt")), Profile::Code);
    assert_eq!(p("Cargo.toml", Some("txt")), Profile::Code);
    assert_eq!(p("data/q1.csv", Some("csv")), Profile::Table);
    assert_eq!(p("a/b.xlsx", Some("xlsx")), Profile::Table);
    assert_eq!(p("a/talk.m4a", Some("audio")), Profile::Media);
    assert_eq!(p("a/shot.png", Some("image")), Profile::Media);
    assert_eq!(
        p("meetings/2027-01-12-sync.md", Some("md")),
        Profile::Meeting
    );
    assert_eq!(p("reports/q1.docx", Some("docx")), Profile::Business);
    assert_eq!(p("CHANGELOG.md", Some("md")), Profile::Other);
    assert_eq!(p("LICENSE", Some("txt")), Profile::Other);
    assert_eq!(p("app.log", Some("txt")), Profile::Other);
    assert_eq!(p("Cargo.lock", Some("txt")), Profile::Other);
    assert_eq!(p("blob.bin", Some("unknown")), Profile::Other);
    assert_eq!(p("x/y.ts", Some("unknown")), Profile::Other);
}

#[test]
fn a_paper_is_known_by_its_content() {
    let ir = md(
        "# Sparse Mixing for Retrieval\n\nAda Lovelace, Alan Turing\n\n## Abstract\n\nWe propose SparseMix.\n\n## References\n\n[1] A. Author. A Title Here. NeurIPS, 2017. doi:10.1234/abcd.5678\n",
    );
    assert_eq!(classify("papers/x.md", &ir).profile, Profile::Paper);
    let plain = md("# Plan\n\nWe will ship in March.\n");
    assert_eq!(classify("plans/x.md", &plain).profile, Profile::Business);
    let meeting = md("# Sync\n\nAttendees: Ada, Alan\n\nWe agreed.\n");
    assert_eq!(
        classify("notes/sync.md", &meeting).profile,
        Profile::Meeting
    );
}

const AGENTS: &str = "# AGENTS.md\n\nOwner: Platform Team\n\n## Commands\n\n- Install: `pnpm install`\n- Run the unit tests with `cargo test -p oo-docs` before every commit.\n\n```bash\npnpm test\n```\n\n## Rules\n\n- Never edit files under contracts/ by hand.\n- Keep functions short.\n- Use rustfmt for formatting.\n\nWe use Postgres for storage. The build lives in `crates/oo-docs/src`.\n";

#[test]
fn an_instruction_document_yields_rules_commands_tools_and_paths() {
    let ir = md(AGENTS);
    let x = extraction("AGENTS.md", &ir);
    let rules = names(&x, RULE);
    assert!(
        rules.contains(&"Never edit files under contracts/ by hand.".to_owned()),
        "{rules:?}"
    );
    assert!(rules.contains(&"Keep functions short.".to_owned()));
    assert!(rules.contains(&"Use rustfmt for formatting.".to_owned()));
    assert!(
        rules.iter().any(|r| r.starts_with("Run the unit tests")),
        "{rules:?}"
    );
    // The sentence about Postgres is no rule.
    assert!(!rules.iter().any(|r| r.contains("Postgres")), "{rules:?}");
    let commands = names(&x, COMMAND);
    assert!(
        commands.contains(&"cargo test -p oo-docs".to_owned()),
        "{commands:?}"
    );
    assert!(commands.contains(&"pnpm test".to_owned()));
    assert!(commands.contains(&"pnpm install".to_owned()));
    assert_eq!(names(&x, TOOL), vec!["cargo", "pnpm", "rustfmt"]);
    let paths = names(&x, PATH);
    assert!(paths.contains(&"contracts".to_owned()), "{paths:?}");
    assert!(paths.contains(&"crates/oo-docs/src".to_owned()));
    // The rule carries what its own text names.
    let linked = x.facts.iter().any(|f| {
        f.subject.name.starts_with("Never edit files")
            && f.predicate == Predicate::Attr("applies_to_path".into())
            && f.value.as_ref().and_then(|v| v["text"].as_str()) == Some("contracts")
    });
    assert!(linked);
    // The rule is its own item with its own block as the anchor.
    let rule = x
        .entities
        .iter()
        .find(|e| e.entity.name == "Keep functions short.")
        .unwrap();
    assert_eq!(rule.evidence.len(), 1);
}

#[test]
fn a_rules_section_keeps_every_item_and_other_sections_need_a_cue() {
    let ir = md(
        "# Style\n\n## Code style\n\n- Four spaces.\n- Name tests after behaviour.\n\n## Background\n\n- We started in 2024.\n- Tests must pass on CI.\n",
    );
    let x = extraction("STYLEGUIDE.md", &ir);
    let rules = names(&x, RULE);
    assert_eq!(
        rules,
        vec![
            "Four spaces.",
            "Name tests after behaviour.",
            "Tests must pass on CI."
        ]
    );
}

const RUST_SRC: &str = "use std::collections::HashMap;\nuse serde_json::Value;\nuse tokio::sync::Mutex;\n\npub struct Store {\n    rows: HashMap<String, Value>,\n}\n\nimpl Store {\n    pub fn new() -> Self {\n        Self { rows: HashMap::new() }\n    }\n\n    pub fn get(&self, key: &str) -> Option<&Value> {\n        self.rows.get(key)\n    }\n}\n\npub enum Mode { Fast, Slow }\n\npub fn open(path: &str) -> Store {\n    let _ = path;\n    Store::new()\n}\n\n#[cfg(test)]\nmod tests {\n    fn hidden() {}\n}\n";

#[test]
fn rust_symbols_and_imports_come_from_the_syntax_tree() {
    let ir = txt(RUST_SRC);
    let x = extraction("src/store.rs", &ir);
    assert_eq!(
        names(&x, FUNCTION),
        vec![
            "src/store.rs::Store::get",
            "src/store.rs::Store::new",
            "src/store.rs::open"
        ]
    );
    assert_eq!(
        names(&x, TYPE),
        vec!["src/store.rs::Mode", "src/store.rs::Store"]
    );
    assert_eq!(names(&x, DEPENDENCY), vec!["serde_json", "tokio"]);
    assert_eq!(names(&x, PATH), vec!["src"]);
    // A symbol keeps the block it is written in as the anchor.
    assert!(x.entities.iter().all(|e| !e.evidence.is_empty()));
}

#[test]
fn python_typescript_and_go_are_read_too() {
    let py = "import os\nimport requests\nfrom flask import Flask\nfrom . import local\n\n\nclass Service:\n    def run(self):\n        return 1\n\n    def stop(self):\n        return 0\n\n\n@decorator\ndef main():\n    pass\n";
    let x = extraction("app/service.py", &txt(py));
    assert_eq!(names(&x, CLASS), vec!["app/service.py::Service"]);
    assert_eq!(
        names(&x, FUNCTION),
        vec![
            "app/service.py::Service.run",
            "app/service.py::Service.stop",
            "app/service.py::main"
        ]
    );
    assert_eq!(names(&x, DEPENDENCY), vec!["flask", "requests"]);

    let ts = "import React from 'react';\nimport { z } from \"zod\";\nimport { helper } from './helper';\nimport fs from 'node:fs';\n\nexport interface Props { id: string }\n\nexport type Id = string;\n\nexport function render(p: Props) {\n  return p.id;\n}\n\nexport const useThing = () => {\n  return 1;\n};\n\nexport class Widget {\n  draw() {}\n}\n";
    let x = extraction("web/widget.ts", &txt(ts));
    assert_eq!(
        names(&x, TYPE),
        vec!["web/widget.ts::Id", "web/widget.ts::Props"]
    );
    assert_eq!(
        names(&x, FUNCTION),
        vec![
            "web/widget.ts::Widget.draw",
            "web/widget.ts::render",
            "web/widget.ts::useThing"
        ]
    );
    assert_eq!(names(&x, CLASS), vec!["web/widget.ts::Widget"]);
    assert_eq!(names(&x, DEPENDENCY), vec!["react", "zod"]);

    let go = "package main\n\nimport (\n\t\"fmt\"\n\t\"github.com/spf13/cobra\"\n)\n\ntype Server struct{}\n\nfunc (s *Server) Start() {}\n\nfunc main() {\n\tfmt.Println(\"x\")\n}\n";
    let x = extraction("cmd/main.go", &txt(go));
    assert_eq!(names(&x, TYPE), vec!["cmd/main.go::Server"]);
    assert_eq!(
        names(&x, FUNCTION),
        vec!["cmd/main.go::Server.Start", "cmd/main.go::main"]
    );
    assert_eq!(names(&x, DEPENDENCY), vec!["github.com/spf13/cobra"]);
}

#[test]
fn indentation_lost_at_a_blank_line_does_not_break_the_tree() {
    // Methods separated by blank lines are separate paragraph blocks whose first line lost its indentation in the block text.
    let py = "class A:\n    def one(self):\n        return 1\n\n    def two(self):\n        return 2\n\n    def three(self):\n        return 3\n";
    let x = extraction("a.py", &txt(py));
    assert_eq!(
        names(&x, FUNCTION),
        vec!["a.py::A.one", "a.py::A.three", "a.py::A.two"]
    );
}

#[test]
fn manifests_name_the_package_and_its_dependencies() {
    let pkg = "{\n  \"name\": \"@acme/web\",\n  \"version\": \"1.2.0\",\n  \"dependencies\": {\"react\": \"^19\", \"zod\": \"^4\"},\n  \"devDependencies\": {\"vitest\": \"^4\"}\n}\n";
    let x = extraction("package.json", &txt(pkg));
    assert_eq!(names(&x, PACKAGE), vec!["@acme/web"]);
    assert_eq!(names(&x, DEPENDENCY), vec!["react", "vitest", "zod"]);
    let cargo = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = { version = \"1\" }\ntokio = \"1\"\n\n[dev-dependencies]\nproptest = \"1\"\n";
    let x = extraction("Cargo.toml", &txt(cargo));
    assert_eq!(names(&x, PACKAGE), vec!["demo"]);
    assert_eq!(names(&x, DEPENDENCY), vec!["proptest", "serde", "tokio"]);
    let py = "[project]\nname = \"pkg\"\ndependencies = [\n  \"requests>=2\",\n  \"rich\",\n]\n";
    let x = extraction("pyproject.toml", &txt(py));
    assert_eq!(names(&x, PACKAGE), vec!["pkg"]);
    assert_eq!(names(&x, DEPENDENCY), vec!["requests", "rich"]);
    let gomod =
        "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/spf13/cobra v1.8.0\n)\n";
    let x = extraction("go.mod", &txt(gomod));
    assert_eq!(names(&x, PACKAGE), vec!["example.com/app"]);
    assert_eq!(names(&x, DEPENDENCY), vec!["github.com/spf13/cobra"]);
}

#[test]
fn a_big_source_is_indexed_but_not_parsed() {
    let mut s = String::new();
    for i in 0..60000 {
        s.push_str(&format!("fn f{i}() {{}}\n\n"));
    }
    let ir = txt(&s);
    let x = extraction("big.rs", &ir);
    assert!(names(&x, FUNCTION).len() <= code::MAX_SYMBOLS);
}

const PAPER: &str = "# Sparse Mixing for Efficient Retrieval\n\nAda Lovelace, Alan Turing and Grace Hopper\n\nProceedings of NeurIPS 2023. arXiv:2301.01234\n\n## Abstract\n\nWe propose SparseMix, a retrieval method. We evaluate on the MS MARCO dataset and BEIR benchmark and report nDCG and MRR. Accuracy improves.\n\n## 1 Introduction\n\nPrior work such as Attention Is All You Need shaped the field.\n\n## References\n\n[1] A. Vaswani, N. Shazeer. Attention Is All You Need. NeurIPS, 2017. doi:10.5555/3295222.3295349\n\n[2] J. Devlin, M. Chang. BERT: Pre-training of Deep Bidirectional Transformers. NAACL, 2019.\n";

#[test]
fn a_paper_yields_authors_venue_methods_datasets_metrics_and_cites() {
    let ir = md(PAPER);
    assert_eq!(classify("p.md", &ir).profile, Profile::Paper);
    let x = extraction("p.md", &ir);
    let people: Vec<String> = x
        .entities
        .iter()
        .filter(|e| e.entity.ty == EntityType::Person)
        .map(|e| e.entity.name.clone())
        .collect();
    assert_eq!(people.len(), 3, "{people:?}");
    assert!(people.contains(&"Grace Hopper".to_owned()));
    assert_eq!(names(&x, VENUE), vec!["NeurIPS"]);
    assert_eq!(names(&x, METHOD), vec!["SparseMix"]);
    assert_eq!(names(&x, DATASET), vec!["BEIR", "MS MARCO"]);
    assert_eq!(names(&x, METRIC), vec!["Accuracy", "MRR"]);
    let papers = names(&x, super::PAPER);
    assert!(papers.contains(&"Sparse Mixing for Efficient Retrieval".to_owned()));
    assert!(
        papers.contains(&"Attention Is All You Need".to_owned()),
        "{papers:?}"
    );
    assert!(papers.iter().any(|p| p.starts_with("BERT")), "{papers:?}");
    let year = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("year".into()))
        .and_then(|f| f.value.as_ref())
        .and_then(|v| v["text"].as_str().map(str::to_owned));
    assert_eq!(year.as_deref(), Some("2023"));
    let doi = x.facts.iter().any(|f| {
        f.predicate == Predicate::Attr("doi".into())
            && f.subject.name == "Attention Is All You Need"
    });
    assert!(doi);
}

#[test]
fn text_without_structure_has_no_entities() {
    let x = extraction("notes/other.log", &txt("started\nstopped\n"));
    assert!(x.entities.is_empty());
    assert!(
        Profile::Other.as_str() == "other"
            && !Profile::Other.uses_model()
            && !Profile::Code.uses_model()
            && Profile::Business.uses_model()
    );
}

#[test]
fn every_label_is_registered() {
    assert!(is_profile_label("Rule") && is_profile_label("Metric") && !is_profile_label("Amount"));
}

#[test]
fn a_style_guide_states_rules_in_every_list_but_the_background() {
    let ir = md(
        "# Style guide\n\n## Formatting\n\n- Rust code is formatted by rustfmt with the default settings.\n- Python code follows black with a line length of 100.\n\n## Background\n\n- The guide was written in 2025.\n",
    );
    let x = extraction("docs/STYLE_GUIDE.md", &ir);
    let rules = names(&x, RULE);
    assert_eq!(rules.len(), 2, "{rules:?}");
    // `Python code follows black` is a sentence, not the command `python`; a version is not an argument.
    let commands = names(&x, COMMAND);
    assert!(commands.is_empty(), "{commands:?}");
}

#[test]
fn commands_are_found_in_running_text_and_a_verb_makes_an_ordinary_word_a_tool() {
    let ir = md(
        "# Notes\n\n- Run make generate after changing the schema.\n- Do not run cargo test -p harborq-core on CI without --locked.\n- Node.js 22 is required, see CHANGELOG.md.\n",
    );
    let x = extraction("AGENTS.md", &ir);
    let commands = names(&x, COMMAND);
    assert!(
        commands.contains(&"make generate".to_owned()),
        "{commands:?}"
    );
    assert!(
        commands.contains(&"cargo test -p harborq-core".to_owned()),
        "{commands:?}"
    );
    assert!(names(&x, PATH).contains(&"CHANGELOG.md".to_owned()));
    assert!(
        !names(&x, PATH).iter().any(|p| p.starts_with("Node")),
        "{:?}",
        names(&x, PATH)
    );
}

#[test]
fn trait_methods_and_metric_suffixes() {
    let ir = txt("pub trait Store {\n    fn load(&self, id: u64) -> Option<String>;\n}\n");
    let x = extraction("lib.rs", &ir);
    assert_eq!(names(&x, FUNCTION), vec!["lib.rs::Store::load"]);
    let ir = md(
        "# A Paper Title Here\n\nAda Lovelace\n\n## Abstract\n\nWe report MRR@10 and Recall@100 and we propose MedCL, a method.\n\n## References\n\n[1] B. Author. Some Reference Title Here. ACL, 2020.\n",
    );
    let x = extraction("p.md", &ir);
    assert_eq!(names(&x, METRIC), vec!["MRR", "Recall"]);
    assert_eq!(names(&x, METHOD), vec!["MedCL"]);
}
