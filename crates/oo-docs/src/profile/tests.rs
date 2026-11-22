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

fn entities(x: &crate::extract::model::Extraction) -> Vec<String> {
    // Everything but the Document itself.
    x.entities
        .iter()
        .filter(|e| e.entity.ty != EntityType::Document)
        .map(|e| {
            format!(
                "{}:{}",
                e.entity
                    .thing_label
                    .as_deref()
                    .unwrap_or(e.entity.ty.as_str()),
                e.entity.name
            )
        })
        .collect()
}

fn label(x: &crate::extract::model::Extraction, l: &str) -> Vec<String> {
    let mut v: Vec<String> = x
        .entities
        .iter()
        .filter(|e| e.entity.thing_label.as_deref() == Some(l))
        .map(|e| e.entity.name.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn classes_by_path_and_format() {
    let p = |path: &str, f: Option<&str>| classify_path(path, f).profile;
    for path in [
        "AGENTS.md",
        "sub/CLAUDE.md",
        "README.md",
        "CONTRIBUTING.md",
        "docs/adr/0003-use-postgres.md",
        "docs/STYLE_GUIDE.md",
        "ops/sop/deploy.md",
        "ops/部署-SOP.md",
        ".cursorrules",
    ] {
        assert_eq!(p(path, Some("md")), Profile::Instruction, "{path}");
    }
    assert_eq!(p("src/main.rs", Some("txt")), Profile::Code);
    assert_eq!(p("CODEOWNERS", Some("txt")), Profile::Code);
    assert_eq!(p("package.json", Some("txt")), Profile::Code);
    assert_eq!(p("data/q1.csv", Some("csv")), Profile::Table);
    assert_eq!(p("a/talk.m4a", Some("audio")), Profile::Media);
    assert_eq!(
        p("meetings/2027-01-12-sync.md", Some("md")),
        Profile::Meeting
    );
    assert_eq!(p("reports/q1.docx", Some("docx")), Profile::Business);
    assert_eq!(
        p("Decisions/D-2026-01-weekly.md", Some("md")),
        Profile::Business
    );
    assert_eq!(p("papers/attention.pdf", Some("pdf")), Profile::Reference);
    assert_eq!(p("books/ddia.epub", None), Profile::Reference);
    assert_eq!(p("CHANGELOG.md", Some("md")), Profile::Other);
    assert_eq!(p("LICENSE", Some("txt")), Profile::Other);
    assert_eq!(p("blob.bin", Some("unknown")), Profile::Other);
    assert_eq!(Profile::Meeting.class(), Class::Record);
    assert_eq!(Profile::Table.class(), Class::Record);
    assert!(
        Class::Record.admits_entities()
            && !Class::Instruction.admits_entities()
            && !Class::Code.admits_entities()
            && !Class::Other.admits_entities()
    );
    assert!(
        Profile::Business.uses_model()
            && !Profile::Instruction.uses_model()
            && !Profile::Reference.uses_model()
            && !Profile::Code.uses_model()
    );
}

#[test]
fn the_rules_decide_before_the_model_is_asked() {
    // A file name or a directory decides.
    assert!(classify_path("AGENTS.md", Some("md")).decided);
    assert!(classify_path("meetings/a.md", Some("md")).decided);
    // Nothing decides a plain document: the default, undecided.
    let c = classify_path("notes/thoughts.md", Some("md"));
    assert_eq!((c.profile, c.decided), (Profile::Business, false));
    // The frontmatter decides.
    let ir = md("---\ntype: sop\n---\n# Deploy\n\nSteps.\n");
    let c = classify("notes/deploy.md", &ir);
    assert_eq!(
        (c.profile, c.reason, c.decided),
        (Profile::Instruction, "frontmatter", true)
    );
    let ir = md("---\ntype: reference\n---\n# A Book\n");
    assert_eq!(classify("notes/a.md", &ir).profile, Profile::Reference);
    // Content: a paper reads as one, a meeting too.
    let ir = md(
        "# Sparse Mixing\n\nAda Lovelace\n\n## Abstract\n\nWe study retrieval.\n\n## References\n\n[1] B. Author. A Title Here. NeurIPS, 2017. doi:10.1234/abcd.5678\n",
    );
    assert_eq!(classify("x/y.md", &ir).profile, Profile::Reference);
    let ir = md("# Sync\n\nAttendees: Ada, Alan\n\nWe agreed.\n");
    assert_eq!(classify("notes/sync.md", &ir).profile, Profile::Meeting);
}

#[test]
fn a_person_overrides_a_file_or_a_directory() {
    let o = vec![
        Override {
            path: "notes".into(),
            dir: true,
            profile: Profile::Instruction,
        },
        Override {
            path: "notes/plan.md".into(),
            dir: false,
            profile: Profile::Business,
        },
        Override {
            path: "notes/deep".into(),
            dir: true,
            profile: Profile::Other,
        },
    ];
    assert_eq!(override_for("notes/a.md", &o), Some(Profile::Instruction));
    assert_eq!(override_for("notes/plan.md", &o), Some(Profile::Business));
    assert_eq!(override_for("notes/deep/x.md", &o), Some(Profile::Other));
    assert_eq!(override_for("notesx/a.md", &o), None);
    let c = classify_path_with("notes/a.md", Some("md"), &o);
    assert_eq!((c.profile, c.reason), (Profile::Instruction, "user"));
    let ir = md("# Plan\n\nText.\n");
    assert_eq!(
        classify_with("notes/plan.md", &ir, &o).profile,
        Profile::Business
    );
}

#[test]
fn an_instruction_file_puts_no_entity_in_and_is_attached_to_its_project() {
    let siblings = vec![
        "harborq/Cargo.toml".to_owned(),
        "harborq/AGENTS.md".to_owned(),
    ];
    let ir = md(
        "# AGENTS.md\n\nOwner: Platform Team\n\n## Rules\n\n- Never edit core/src/generated/ by hand.\n- Run cargo test -p harborq-core before every commit.\n\n```bash\ncargo build\n```\n",
    );
    let x = extraction_in("harborq/AGENTS.md", &ir, &siblings);
    assert!(entities(&x).is_empty(), "{:?}", entities(&x));
    let scope = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("scope".into()))
        .and_then(|f| f.value.clone());
    assert_eq!(scope.unwrap()["text"], "harborq");
    let class = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("class".into()))
        .and_then(|f| f.value.clone());
    assert_eq!(class.unwrap()["enum"], "instruction");
    // A binding hint to a Project of that name, never a new Project.
    assert!(
        x.hints
            .iter()
            .any(|h| h.target_ty == EntityType::Project && h.target_name == "harborq")
    );
    // The nearest project: a nested manifest wins.
    let nested = vec!["a/Cargo.toml".to_owned(), "a/web/package.json".to_owned()];
    assert_eq!(project_dir("a/web/src/x.ts", &nested), "a/web");
    assert_eq!(project_dir("a/core/y.rs", &nested), "a");
}

#[test]
fn the_core_rules_read_no_entity_from_instruction_code_and_other_files() {
    // Through the whole rules/1 pipeline, not only the profile extractor: an `Owner:` line and a `key: value` line are no entities.
    let ir = md("# AGENTS.md\n\nOwner: Platform Team\n\n负责人：林川\n\nstatus: active\n");
    let x = crate::extract::rules::extract("harborq/AGENTS.md", &ir);
    let others: Vec<_> = x
        .entities
        .iter()
        .filter(|e| e.entity.ty != EntityType::Document)
        .collect();
    assert!(others.is_empty(), "{others:?}");
    let ir = txt("def f():\n    return {'name': 'x', 'owner': 'a@b.com'}\n");
    let x = crate::extract::rules::extract("src/a.py", &ir);
    assert!(
        x.entities
            .iter()
            .all(|e| e.entity.ty == EntityType::Document)
    );
    let ir = txt("started\nstopped by admin@example.com\n");
    let x = crate::extract::rules::extract("ops/worker.log", &ir);
    assert!(
        x.entities
            .iter()
            .all(|e| e.entity.ty == EntityType::Document)
    );
    // A record still enters.
    let ir = md("---\ntype: person\nname: 林川\n---\n# 林川\n");
    let x = crate::extract::rules::extract("People/林川.md", &ir);
    assert!(x.entities.iter().any(|e| e.entity.ty == EntityType::Person));
}

#[test]
fn code_gives_structure_only() {
    let ir = txt("pub fn open() {}\nstruct Queue;\nuse serde::Serialize;\n");
    let x = extraction_in(
        "harborq/core/src/queue.rs",
        &ir,
        &["harborq/Cargo.toml".to_owned()],
    );
    assert!(entities(&x).is_empty(), "{:?}", entities(&x));
    assert_eq!(
        x.facts
            .iter()
            .find(|f| f.predicate == Predicate::Attr("scope".into()))
            .unwrap()
            .value
            .as_ref()
            .unwrap()["text"],
        "harborq"
    );
    let ir = txt("# owners\n*       @platform-team\ncore/   @ada alice@example.com\n");
    let x = extraction_in("CODEOWNERS", &ir, &[]);
    assert!(entities(&x).is_empty());
    let owners = x
        .facts
        .iter()
        .find(|f| f.predicate == Predicate::Attr("owners".into()))
        .unwrap()
        .value
        .as_ref()
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(owners, "@platform-team, @ada, alice@example.com");
}

const REF: &str = "# Sparse Mixing for Efficient Retrieval\n\nAda Lovelace, Alan Turing and Grace Hopper\n\nProceedings of NeurIPS 2023. arXiv:2301.01234\n\n## Abstract\n\nWe propose SpaMix, a method. We evaluate on the MS MARCO dataset and report MRR.\n\n## References\n\n[1] A. Vaswani, N. Shazeer. Attention Is All You Need. NeurIPS, 2017. doi:10.5555/3295222.3295349\n\n[2] J. Devlin, M. Chang. BERT: Pre-training of Deep Bidirectional Transformers. NAACL, 2019.\n";

#[test]
fn a_reference_is_one_instance_with_authors_and_cites() {
    let ir = md(REF);
    assert_eq!(classify("p.md", &ir).profile, Profile::Reference);
    let x = extraction("p.md", &ir);
    let mut kinds: Vec<String> = entities(&x)
        .into_iter()
        .map(|e| e.split(':').next().unwrap().to_owned())
        .collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(
        kinds,
        vec!["Person", "Reference"],
        "no method, dataset, metric or venue entity: {:?}",
        entities(&x)
    );
    assert_eq!(label(&x, REFERENCE).len(), 3);
    assert!(label(&x, REFERENCE).contains(&"Sparse Mixing for Efficient Retrieval".to_owned()));
    assert!(label(&x, REFERENCE).contains(&"Attention Is All You Need".to_owned()));
    let authored = x
        .facts
        .iter()
        .filter(|f| f.predicate == Predicate::Authored)
        .count();
    let cites = x
        .facts
        .iter()
        .filter(|f| f.predicate == Predicate::Cites)
        .count();
    assert_eq!((authored, cites), (3, 2));
    let attr = |n: &str| {
        x.facts
            .iter()
            .find(|f| {
                f.predicate == Predicate::Attr(n.into()) && f.subject.name.starts_with("Sparse")
            })
            .and_then(|f| f.value.clone())
    };
    assert_eq!(attr("year").unwrap()["text"], "2023");
    assert_eq!(attr("venue").unwrap()["text"], "NeurIPS");
    assert_eq!(attr("arxiv").unwrap()["text"], "2301.01234");
}

#[test]
fn a_reference_the_way_the_parser_breaks_it_into_blocks() {
    let ir = md(
        "Label-Efficient\n\nSegmentation of Satellite\n\nImagery\n\nPriya Raman, Tomas Novak\n\narXiv preprint 2024. arXiv:2403.01882\n\n# Abstract\n\nPixel labels are expensive.\n\n# References\n\n[1] O. Ronneberger, P. Fischer. U-Net: Convolutional Networks for Biomedical Image Segmentation. MICCAI, 2015.\n\n[2] A. Kirillov, E. Mintun, N. Ravi.\n\nSegment Anything. ICCV, 2023.\n",
    );
    assert_eq!(classify("p.pdf", &ir).profile, Profile::Reference);
    let x = extraction("p.pdf", &ir);
    let r = label(&x, REFERENCE);
    assert!(
        r.contains(&"Label-Efficient Segmentation of Satellite Imagery".to_owned()),
        "{r:?}"
    );
    assert!(r.contains(&"Segment Anything".to_owned()), "{r:?}");
    assert_eq!(
        x.entities
            .iter()
            .filter(|e| e.entity.ty == EntityType::Person)
            .count(),
        2
    );
}

#[test]
fn every_label_is_registered() {
    assert!(is_profile_label("Reference") && !is_profile_label("Amount"));
}
