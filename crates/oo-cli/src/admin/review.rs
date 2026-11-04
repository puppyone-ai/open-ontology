//! Owner decisions are separate from model output and recompiled before handoff.
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::process::{Command, Stdio};

use oo_core::api::payload_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Failure, Invocation, wizard::Screen};

pub(super) const HELP: &str = "\
oo review --input INPUT.json --out REVIEWED.json [--review-file DECISIONS.json] [--json]
Input: oo.builder.reviewed-community/v1 command, without review.
Non-TTY requires explicit hash-bound oo.owner-review/v1 decisions for every item.
TTY offers accept / edit JSON / reject, then a separate G14 confirmation.
Rejection records the decision but blocks the whole candidate; no partial publication.
Interrupted TTY reviews retain decisions beside --out; rerun the same command.
Output is private reviewed registry + compiled Pack, not authenticated publication.
selfbuild-budget must be installed beside oo. --yes never approves a review.";

type Result<T> = std::result::Result<T, Failure>;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn hash(value: &Value) -> String {
    payload_sha256(value.to_string().as_bytes())
}

pub(super) fn read(path: &Path) -> Result<Value> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))?;
    let meta = file
        .metadata()
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o777 != 0o600
        || meta.len() > MAX_BYTES
    {
        return Err(Failure::local("REVIEW_PRIVATE_FILE_REQUIRED"));
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))?;
    serde_json::from_str(&text).map_err(|_| Failure::local("REVIEW_JSON"))
}

pub(super) fn save_new(path: &Path, value: &Value) -> Result<()> {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Failure::local("REVIEW_OUTPUT_EXISTS_OR_IO"))?;
    out.write_all(value.to_string().as_bytes())
        .and_then(|_| out.sync_all())
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))
}

fn save_pending(path: &Path, session: &Session) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    save_new(&temporary, &json!(session))?;
    let result = fs::rename(&temporary, path).map_err(|_| Failure::local("REVIEW_FILE_IO"));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result?;
    fs::File::open(path.parent().unwrap_or(Path::new(".")))
        .and_then(|f| f.sync_all())
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replacement: Option<Value>,
    #[serde(default)]
    reason: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Gate {
    summary_sha256: String,
    action: String,
    reason: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    protocol: String,
    input_sha256: String,
    reviewed_at: String,
    items: BTreeMap<String, Decision>,
    #[serde(default)]
    g14: Option<Gate>,
}

struct Item {
    id: String,
    pointer: String,
    value: Value,
    editable: &'static [&'static str],
}

fn object(value: &Value) -> Result<&serde_json::Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Failure::local("REVIEW_INPUT_SHAPE"))
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| Failure::local("REVIEW_INPUT_SHAPE"))
}

fn text(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Failure::local("REVIEW_INPUT_SHAPE"))
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn items(input: &Value) -> Result<Vec<Item>> {
    if input["protocol"] != "oo.builder.reviewed-community/v1"
        || input["draft"]["protocol"] != "oo.selfbuild.community-draft/v1"
        || !array(&input["draft"]["unresolved"])?.is_empty()
        || input.get("review").is_some()
    {
        return Err(Failure::local("REVIEW_UNRESOLVED_OR_PROTOCOL"));
    }
    let mut items = Vec::new();
    for (id, entity) in object(&input["draft"]["registry"]["entities"])? {
        let mut value = entity.clone();
        let table = array(&input["effective"]["datasets"])?
            .iter()
            .find(|d| d["dataset"] == entity["main_table"])
            .ok_or_else(|| Failure::local("REVIEW_ENTITY_TABLE"))?;
        let key = match table["primary_key"].as_array() {
            Some(keys) if keys.len() == 1 => keys[0].clone(),
            _ => Value::Null,
        };
        value["key"] = key.clone();
        value["display"] = key;
        items.push(Item {
            id: format!("entity/{id}"),
            pointer: format!("/draft/registry/entities/{}", escape(id)),
            value,
            editable: &["canonical_name", "aliases", "key", "display"],
        });
    }
    for (id, value) in object(&input["draft"]["accepted"])? {
        let (kind, editable): (&str, &'static [&'static str]) = match value["kind"].as_str() {
            Some("relation") => (
                "relationship",
                &["name", "cardinality", "decision", "reason"],
            ),
            Some("table") | Some("column") => ("description", &["label", "description"]),
            Some("entity") => continue,
            _ => return Err(Failure::local("REVIEW_ITEM_KIND")),
        };
        items.push(Item {
            id: format!("{kind}/{id}"),
            pointer: format!("/draft/accepted/{}", escape(id)),
            value: value.clone(),
            editable,
        });
    }
    for (i, metric) in input["effective"]
        .get("metrics")
        .map(array)
        .transpose()?
        .into_iter()
        .flatten()
        .enumerate()
    {
        items.push(Item {
            id: format!("metric/{}", text(&metric["metric"])?),
            pointer: format!("/effective/metrics/{i}"),
            value: metric.clone(),
            editable: &["label", "description", "sql", "unit"],
        });
    }
    for (i, dataset) in array(&input["effective"]["datasets"])?.iter().enumerate() {
        let name = text(&dataset["dataset"])?;
        for (j, column) in array(&dataset["columns"])?.iter().enumerate() {
            items.push(Item {
                id: format!("classification/{name}/{}", text(&column["name"])?),
                pointer: format!("/effective/datasets/{i}/columns/{j}"),
                value: column.clone(),
                editable: &["pii", "semantic_type", "unit"],
            });
        }
    }
    let unique: std::collections::BTreeSet<_> = items.iter().map(|i| &i.id).collect();
    if items.is_empty() || items.len() != unique.len() {
        return Err(Failure::local("REVIEW_DUPLICATE_OR_EMPTY"));
    }
    Ok(items)
}

fn apply(item: &Item, decision: &Decision) -> Result<Value> {
    match decision.action.as_str() {
        "accept" if decision.replacement.is_none() => Ok(item.value.clone()),
        "reject" if decision.replacement.is_none() && !decision.reason.trim().is_empty() => {
            Ok(item.value.clone())
        }
        "edit" if !decision.reason.trim().is_empty() => {
            let patch = object(
                decision
                    .replacement
                    .as_ref()
                    .ok_or_else(|| Failure::local("REVIEW_EDIT_REQUIRED"))?,
            )?;
            if patch.is_empty()
                || patch
                    .keys()
                    .any(|key| !item.editable.contains(&key.as_str()))
            {
                return Err(Failure::local("REVIEW_EDIT_IMMUTABLE_FIELD"));
            }
            let mut value = item.value.clone();
            for (key, replacement) in patch {
                value[key] = replacement.clone();
            }
            Ok(value)
        }
        _ => Err(Failure::local("REVIEW_DECISION_REQUIRED")),
    }
}

fn decide(screen: &mut Screen, item: &Item) -> Result<Decision> {
    loop {
        let action = screen.prompt(1, &format!(
            "Owner self-review: {}\n{}\nEditable fields: {}\nType accept, edit, or reject (no default).",
            json!(item.id), serde_json::to_string_pretty(&item.value).unwrap(),
            item.editable.join(", ")), "", false)?;
        let decision = match action.as_str() {
            "accept" => Decision {
                action,
                replacement: None,
                reason: String::new(),
            },
            "reject" => Decision {
                action,
                replacement: None,
                reason: screen.prompt(1, "Rejection reason (blocks candidate)", "", false)?,
            },
            "edit" => {
                let raw = screen.prompt(
                    1,
                    "JSON object containing only changed editable fields",
                    "",
                    false,
                )?;
                let Ok(replacement) = serde_json::from_str(&raw) else {
                    continue;
                };
                Decision {
                    action,
                    replacement: Some(replacement),
                    reason: screen.prompt(1, "Edit reason", "", false)?,
                }
            }
            _ => continue,
        };
        if apply(item, &decision).is_ok() {
            return Ok(decision);
        }
    }
}

fn bridge(request: &Value) -> Result<Value> {
    let exe = std::env::current_exe().map_err(|_| Failure::local("REVIEW_COMPILER_INSTALL"))?;
    let mut child = Command::new(exe.with_file_name("selfbuild-budget"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Failure::local("REVIEW_COMPILER_INSTALL"))?;
    // Write concurrently: a large registry can fill both process pipes.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Failure::local("REVIEW_COMPILER_IO"))?;
    let body = request.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(body.as_bytes()));
    let output = child
        .wait_with_output()
        .map_err(|_| Failure::local("REVIEW_COMPILER_IO"))?;
    if !writer.join().is_ok_and(|r| r.is_ok())
        || !output.status.success()
        || output.stdout.len() as u64 > MAX_BYTES
    {
        return Err(Failure::local("REVIEW_COMPILER_REJECTED"));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| Failure::local("REVIEW_COMPILER_PROTOCOL"))
}

fn prepare(input: &Value, items: &[Item], session: &Session) -> Result<Value> {
    if session.protocol != "oo.owner-review/v1"
        || session.input_sha256 != hash(input)
        || session.items.len() != items.len()
        || serde_json::from_value::<oo_core::oos::CivilDate>(json!(session.reviewed_at)).is_err()
    {
        return Err(Failure::local("REVIEW_BINDING"));
    }
    let mut command = input.clone();
    let mut selections = serde_json::Map::new();
    let mut classifications = serde_json::Map::new();
    for item in items {
        let decision = session
            .items
            .get(&item.id)
            .ok_or_else(|| Failure::local("REVIEW_MISSING_ITEM"))?;
        let mut value = apply(item, decision)?;
        if item.id.starts_with("entity/") {
            let table = array(&input["effective"]["datasets"])?
                .iter()
                .find(|d| d["dataset"] == value["main_table"])
                .ok_or_else(|| Failure::local("REVIEW_ENTITY_TABLE"))?;
            // A single declared key may be offered; composite/display guesses need an edit.
            let key = value.get("key").cloned().unwrap_or_else(|| {
                match table["primary_key"].as_array() {
                    Some(keys) if keys.len() == 1 => keys[0].clone(),
                    _ => Value::Null,
                }
            });
            let display = value.get("display").cloned().unwrap_or_else(|| key.clone());
            text(&key)?;
            text(&display)?;
            selections.insert(
                text(&value["id"])?.to_owned(),
                json!({"key":key,"display":display}),
            );
            value.as_object_mut().unwrap().remove("key");
            value.as_object_mut().unwrap().remove("display");
        }
        if item.id.starts_with("classification/") {
            serde_json::from_value::<oo_core::config::PiiLevel>(value["pii"].clone())
                .map_err(|_| Failure::local("REVIEW_PII_REQUIRED"))?;
            serde_json::from_value::<oo_core::oos::SemanticType>(value["semantic_type"].clone())
                .map_err(|_| Failure::local("REVIEW_SEMANTIC_TYPE_REQUIRED"))?;
            value["classification"] =
                json!({"state":"confirmed","by":"owner","at":session.reviewed_at});
            value["review"] = json!({"state":"approved","by":"owner","at":session.reviewed_at});
            classifications.insert(
                item.id.clone(),
                json!({
                "pii":value["pii"],"semantic_type":value["semantic_type"],
                "unit":value["unit"],"reviewer":"owner"}),
            );
        }
        *command
            .pointer_mut(&item.pointer)
            .ok_or_else(|| Failure::local("REVIEW_ITEM_POINTER"))? = value;
    }
    command["review"] = json!({"reviewer":"owner","reviewed_at":session.reviewed_at,
        "mode":"self","review_origin":"operator_pinned_file",
        "entities":selections,"decisions":{}});
    command["classification_review"] = json!({"review_origin":"operator_pinned_file",
        "project":command["effective"]["project"],"role":command["visibility"]["role"],
        "principal":command["visibility"]["principal"],"columns":classifications});
    Ok(command)
}

fn summary(command: &Value) -> Result<Value> {
    let mut mapped = Vec::new();
    let mut rejections = serde_json::Map::new();
    for edge in array(&command["draft"]["graph"]["edges"])? {
        let id = text(&edge["id"])?;
        let value = &command["draft"]["accepted"][id];
        match value["decision"].as_str() {
            Some("proposed") => mapped.push(id),
            Some("rejected") => {
                rejections.insert(id.into(), json!(text(&value["reason"])?));
            }
            _ => return Err(Failure::local("REVIEW_EDGE_DECISION")),
        }
    }
    bridge(
        &json!({"operation":"review","graph":command["draft"]["graph"],
        "registry":command["draft"]["registry"],"mapped":mapped,"rejections":rejections,
        "core":command["draft"]["review"]["summary"]["core_entities"],
        "orphan_threshold":command["orphan_threshold"]}),
    )
}

pub(super) fn review(input: &Value, decision_file: Option<&Path>, out: &Path) -> Result<Value> {
    let candidates = items(input)?;
    if out
        .try_exists()
        .map_err(|_| Failure::local("REVIEW_FILE_IO"))?
    {
        return Err(Failure::local("REVIEW_OUTPUT_EXISTS_OR_IO"));
    }
    let pending = out.with_extension("pending.json");
    let lock_path = out.with_extension("review.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(lock_path)
        .map_err(|_| Failure::local("REVIEW_LOCK"))?;
    if !lock
        .metadata()
        .is_ok_and(|m| m.is_file() && m.nlink() == 1 && m.mode() & 0o777 == 0o600)
        || lock.try_lock().is_err()
    {
        return Err(Failure::local("REVIEW_LOCK"));
    }
    let mut screen = None;
    let mut session: Session = if let Some(path) = decision_file {
        serde_json::from_value(read(path)?).map_err(|_| Failure::local("REVIEW_FILE_SHAPE"))?
    } else {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Err(Failure::usage(
                "REVIEW_FILE_REQUIRED: non-TTY never auto-accepts",
            ));
        }
        screen = Some(Screen::open()?);
        if pending
            .try_exists()
            .map_err(|_| Failure::local("REVIEW_FILE_IO"))?
        {
            serde_json::from_value(read(&pending)?)
                .map_err(|_| Failure::local("REVIEW_FILE_SHAPE"))?
        } else {
            Session {
                protocol: "oo.owner-review/v1".into(),
                input_sha256: hash(input),
                reviewed_at: screen.as_mut().unwrap().prompt(
                    1,
                    "Review date (YYYY-MM-DD)",
                    "",
                    false,
                )?,
                items: BTreeMap::new(),
                g14: None,
            }
        }
    };
    if session.protocol != "oo.owner-review/v1"
        || session.input_sha256 != hash(input)
        || session
            .items
            .keys()
            .any(|id| !candidates.iter().any(|i| &i.id == id))
        || serde_json::from_value::<oo_core::oos::CivilDate>(json!(session.reviewed_at)).is_err()
    {
        return Err(Failure::local("REVIEW_BINDING"));
    }
    if let Some(screen) = screen.as_mut() {
        for item in &candidates {
            if !session.items.contains_key(&item.id) {
                session.items.insert(item.id.clone(), decide(screen, item)?);
                save_pending(&pending, &session)?;
            }
        }
    }
    // Rejected candidates remain auditable but can never acquire a compiled Pack.
    for item in &candidates {
        apply(
            item,
            session
                .items
                .get(&item.id)
                .ok_or_else(|| Failure::local("REVIEW_MISSING_ITEM"))?,
        )?;
    }
    let rejected = session.items.values().any(|d| d.action == "reject");
    let mut result = json!({"protocol":"oo.owner-reviewed-registry/v1",
        "status":"rejected","published":false,"authentication":"not_claimed",
        "review_decision":{"reviewer":"owner","mode":"self","decisions":session},
        "compiled":null});
    if !rejected {
        let mut command = prepare(input, &candidates, &session)?;
        let report = summary(&command)?;
        if let Some(screen) = screen.as_mut() {
            let reason = screen.prompt(2, &format!(
                "G14 connectivity review\n{}\nType an approval reason, or REJECT. Empty is not approval.",
                serde_json::to_string_pretty(&report["summary"]).unwrap()), "", false)?;
            if reason.trim().is_empty() {
                return Err(Failure::local("REVIEW_G14_REQUIRED"));
            }
            session.g14 = Some(Gate {
                summary_sha256: hash(&report["summary"]),
                action: if reason == "REJECT" {
                    "reject"
                } else {
                    "accept"
                }
                .into(),
                reason,
            });
            save_pending(&pending, &session)?;
        }
        let gate = session
            .g14
            .as_ref()
            .ok_or_else(|| Failure::local("REVIEW_G14_REQUIRED"))?;
        if gate.summary_sha256 != hash(&report["summary"])
            || gate.reason.trim().is_empty()
            || !matches!(gate.action.as_str(), "accept" | "reject")
        {
            return Err(Failure::local("REVIEW_G14_BINDING"));
        }
        result["review_decision"]["decisions"] = json!(session);
        result["summary"] = report["summary"].clone();
        if gate.action == "accept" {
            let mut required = array(&report["summary"]["naming_conflicts"])?.clone();
            if report["summary"]["orphan_review_required"] == true {
                required.push(json!("orphan_entities"));
            }
            let mut decisions = serde_json::Map::new();
            for id in required {
                decisions.insert(
                    text(&id)?.into(),
                    json!({"reviewer":"owner","decision":"approved",
                    "summary_sha256":gate.summary_sha256}),
                );
            }
            command["review"]["decisions"] = json!(decisions);
            command["review"]["summary_sha256"] = json!(gate.summary_sha256);
            command["review"]["draft_sha256"] = json!(hash(&command["draft"]));
            command["review"]["effective_sha256"] = json!(hash(&command["effective"]));
            let compiled = bridge(&json!({"operation":"reviewed","command":command}))?;
            if compiled["protocol"] != "oo.builder.reviewed-community-result/v1"
                || compiled["published"] != false
            {
                return Err(Failure::local("REVIEW_COMPILER_PROTOCOL"));
            }
            result["status"] = json!("reviewed");
            result["command"] = command;
            result["compiled"] = compiled;
        }
    }
    save_new(out, &result)?;
    Ok(result)
}

pub(super) fn run(args: &[String]) -> Result<Invocation> {
    if args.iter().any(|s| s == "--help" || s == "-h") {
        return Ok(Invocation::Help(HELP.into()));
    }
    let mut opts = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        let value = match key {
            "--json" => "",
            "--input" | "--out" | "--review-file" => {
                index += 1;
                args.get(index)
                    .map(String::as_str)
                    .filter(|s| !s.is_empty() && !s.starts_with('-'))
                    .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT"))?
            }
            _ => return Err(Failure::usage("ADMIN_ARGUMENT")),
        };
        if opts.insert(key, value).is_some() {
            return Err(Failure::usage("ADMIN_ARGUMENT"));
        }
        index += 1;
    }
    let input = read(Path::new(
        opts.get("--input")
            .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT"))?,
    ))?;
    let out = Path::new(
        opts.get("--out")
            .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT"))?,
    );
    let result = review(&input, opts.get("--review-file").map(Path::new), out)?;
    Ok(Invocation::AdminReport(
        json!({"protocol":"oo.owner-review-report/v1","status":result["status"],
            "published":false,"authentication":"not_claimed"})
        .to_string(),
        if result["status"] == "reviewed" { 0 } else { 1 },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_cannot_change_physical_identity_or_accept_without_a_decision() {
        let item = Item {
            id: "classification/app.t/id".into(),
            pointer: "".into(),
            value: json!({"name":"id","pii":null}),
            editable: &["pii"],
        };
        for decision in [
            json!({"action":"", "reason":""}),
            json!({"action":"accept","replacement":{"pii":"none"}}),
            json!({"action":"reject","reason":""}),
            json!({"action":"edit","reason":"test","replacement":{"name":"other"}}),
        ] {
            assert!(apply(&item, &serde_json::from_value(decision).unwrap()).is_err());
        }
        let decision = serde_json::from_value(json!({"action":"edit","reason":"owner checked",
            "replacement":{"pii":"personal"}}))
        .unwrap();
        assert_eq!(
            apply(&item, &decision).ok().unwrap(),
            json!({"name":"id","pii":"personal"})
        );
    }
}
