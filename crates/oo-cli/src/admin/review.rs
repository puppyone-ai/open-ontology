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
oo review --input INPUT.json --out REVIEWED.json --adjudication ADJ.json [--confirm CONFIRM.json [--overrides O.json]]
oo review --input PENDING.json --json
With oo.adjudication/v1 (--adjudication, or adjudication.json beside --out) a TTY
shows the ontology graph: override any decision, then confirm once (D28). The
confirmation binds the summary recomputed after overrides and is written beside
--out (.publish-confirm.json, .overrides.json); non-TTY passes --confirm instead.
Every candidate then records the adjudicated default with that summary hash.
Pending tables list their semantic changes; inspection never approves them.
Input: oo.builder.reviewed-community/v1 command, without review.
Non-TTY requires explicit hash-bound oo.owner-review/v1 decisions for every item.
TTY offers accept / edit JSON / reject, then a separate G14 confirmation.
Entity key/display default to the declared primary key: one column as a string,
a composite as its ordered column array. Keyless entities are refused.
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
    serde_json::from_str(&read_text(path)?).map_err(|_| Failure::local("REVIEW_JSON"))
}

fn read_text(path: &Path) -> Result<String> {
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
    Ok(text)
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

/// Declared identity offered to the owner: one column as a string, a composite
/// primary key as its ordered column array, and no key (JSON null) otherwise.
fn declared_key(table: &Value) -> Value {
    match table["primary_key"].as_array() {
        Some(keys) if keys.len() == 1 => keys[0].clone(),
        Some(keys) if keys.len() > 1 => Value::Array(keys.clone()),
        _ => Value::Null,
    }
}

/// Entity key/display: a nonempty column, or at least two distinct nonempty columns.
/// Keyless entities stay refused; the compiler re-checks against the catalog.
fn entity_key(value: &Value) -> Result<()> {
    if value.is_string() {
        return text(value).map(|_| ());
    }
    let columns = array(value)?;
    let distinct: std::collections::BTreeSet<_> =
        columns.iter().map(text).collect::<Result<_>>()?;
    if columns.len() < 2 || distinct.len() != columns.len() {
        return Err(Failure::local("REVIEW_INPUT_SHAPE"));
    }
    Ok(())
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
        let key = declared_key(table);
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
                if matches!(key.as_str(), "key" | "display") {
                    entity_key(replacement)?;
                }
                value[key] = replacement.clone();
            }
            Ok(value)
        }
        _ => Err(Failure::local("REVIEW_DECISION_REQUIRED")),
    }
}

fn decide(screen: &mut Screen, item: &Item) -> Result<Decision> {
    loop {
        let action = screen.prompt(6, &format!(
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
                reason: screen.prompt(6, "Rejection reason (blocks candidate)", "", false)?,
            },
            "edit" => {
                let raw = screen.prompt(
                    6,
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
                    reason: screen.prompt(6, "Edit reason", "", false)?,
                }
            }
            _ => continue,
        };
        if apply(item, &decision).is_ok() {
            return Ok(decision);
        }
    }
}

type Compiler = (Vec<String>, Vec<(String, String)>);
static COMPILER: std::sync::OnceLock<Compiler> = std::sync::OnceLock::new();

/// Run the review compiler through `argv` (the deployed runtime image, locally
/// or over the pinned SSH connection) rather than a `selfbuild-budget` beside
/// `oo`. Review itself stays on this host. `env` holds no secret values.
pub(super) fn use_compiler(argv: Vec<String>, env: Vec<(String, String)>) -> bool {
    !argv.is_empty() && COMPILER.set((argv, env)).is_ok()
}

fn bridge(request: &Value) -> Result<Value> {
    let mut command = if let Some((argv, env)) = COMPILER.get() {
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .envs(env.iter().map(|(k, v)| (k, v)));
        command
    } else {
        let exe = std::env::current_exe().map_err(|_| Failure::local("REVIEW_COMPILER_INSTALL"))?;
        Command::new(exe.with_file_name("selfbuild-budget"))
    };
    let mut child = command
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
            // The declared key (scalar or ordered composite) is offered; no guess is made.
            let key = value
                .get("key")
                .cloned()
                .unwrap_or_else(|| declared_key(table));
            let display = value.get("display").cloned().unwrap_or_else(|| key.clone());
            entity_key(&key)?;
            entity_key(&display)?;
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

/// Recheck a recovered output before the publisher consumes it; file existence is not a receipt.
pub(super) fn verify_handoff(input: &Value, reviewed: &Value) -> Result<()> {
    if reviewed["review_decision"]["protocol"] == "oo.publish-confirm/v1" {
        let command = &reviewed["command"];
        let original: Value = serde_json::from_str(
            command["adjudication_input"]
                .as_str()
                .ok_or_else(|| Failure::local("REVIEW_HANDOFF_SHAPE"))?,
        )
        .map_err(|_| Failure::local("REVIEW_HANDOFF_SHAPE"))?;
        if reviewed["protocol"] != "oo.owner-reviewed-registry/v1"
            || reviewed["status"] != "reviewed"
            || reviewed["published"] != false
            || reviewed["authentication"] != "not_claimed"
            || original != *input
            || reviewed["review_decision"] != command["confirmation"]
            || bridge(&json!({"operation":"reviewed","command":command}))? != reviewed["compiled"]
        {
            return Err(Failure::local("REVIEW_HANDOFF_BINDING"));
        }
        return Ok(());
    }
    let session: Session = serde_json::from_value(reviewed["review_decision"]["decisions"].clone())
        .map_err(|_| Failure::local("REVIEW_HANDOFF_SHAPE"))?;
    if reviewed["protocol"] != "oo.owner-reviewed-registry/v1"
        || reviewed["status"] != "reviewed"
        || reviewed["published"] != false
        || reviewed["authentication"] != "not_claimed"
        || reviewed["review_decision"]["reviewer"] != "owner"
        || reviewed["review_decision"]["mode"] != "self"
        || session.items.values().any(|d| d.action == "reject")
    {
        return Err(Failure::local("REVIEW_HANDOFF_BINDING"));
    }
    let mut expected = prepare(input, &items(input)?, &session)?;
    let report = summary(&expected)?;
    let gate = session
        .g14
        .as_ref()
        .ok_or_else(|| Failure::local("REVIEW_G14_REQUIRED"))?;
    if gate.action != "accept"
        || gate.reason.trim().is_empty()
        || gate.summary_sha256 != hash(&report["summary"])
        || reviewed["summary"] != report["summary"]
    {
        return Err(Failure::local("REVIEW_G14_BINDING"));
    }
    let mut required = array(&report["summary"]["naming_conflicts"])?.clone();
    if report["summary"]["orphan_review_required"] == true {
        required.push(json!("orphan_entities"));
    }
    for id in required {
        expected["review"]["decisions"][text(&id)?] = json!({"reviewer":"owner",
            "decision":"approved","summary_sha256":gate.summary_sha256});
    }
    expected["review"]["summary_sha256"] = json!(gate.summary_sha256);
    expected["review"]["draft_sha256"] = json!(hash(&expected["draft"]));
    expected["review"]["effective_sha256"] = json!(hash(&expected["effective"]));
    if expected != reviewed["command"]
        || bridge(&json!({"operation":"reviewed","command":expected}))? != reviewed["compiled"]
    {
        return Err(Failure::local("REVIEW_HANDOFF_BINDING"));
    }
    Ok(())
}

/// Where the product's adjudication and the one confirmation come from.
#[derive(Default)]
pub(super) struct Adjudicated<'a> {
    pub raw_input: Option<&'a str>,
    /// `oo.adjudication/v1`; defaults to `adjudication.json` beside `--out`.
    pub adjudication: Option<&'a Path>,
    /// Non-TTY: an `oo.publish-confirm/v1` file instead of the confirm screen.
    pub confirm: Option<&'a Path>,
    pub overrides: Option<&'a Path>,
}

pub(super) fn review(input: &Value, decision_file: Option<&Path>, out: &Path) -> Result<Value> {
    review_with(input, decision_file, out, &Adjudicated::default())
}

/// D28: with an adjudication, the owner confirms once (graph screen or a
/// confirm file). The compiler applies the bound evidence decisions.
fn confirm_once(
    adjudication: &Path,
    source: &Adjudicated,
    out: &Path,
    screen: &mut Option<Screen>,
) -> Result<Option<Value>> {
    use crate::tui::adjudication as adj;
    let base = adj::load(adjudication).map_err(Failure::local)?;
    if let Some(path) = source.confirm {
        let doc = adj::read_json(path).map_err(Failure::local)?;
        let overrides: Option<adj::Overrides> = match source.overrides {
            Some(p) => Some(
                serde_json::from_value(adj::read_json(p).map_err(Failure::local)?)
                    .map_err(|_| Failure::local("OVERRIDE_SHAPE"))?,
            ),
            None => None,
        };
        let expected = adj::confirm(
            &base,
            overrides.as_ref(),
            doc["summary_sha256"].as_str().unwrap_or_default(),
            doc["reason"].as_str().unwrap_or_default(),
            doc["confirmed_at"].as_str().unwrap_or_default(),
        )
        .map_err(Failure::local)?;
        if expected != doc {
            return Err(Failure::local("REVIEW_CONFIRM_BINDING"));
        }
        return Ok(Some(doc));
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Ok(None);
    }
    let screen = screen.get_or_insert(Screen::open()?);
    let Some((doc, overrides)) = crate::tui::app::confirm(screen.term(), base, &adj::now_rfc3339())
        .map_err(Failure::local)?
    else {
        return Err(Failure::local("TUI_CANCELLED"));
    };
    if !overrides.items.is_empty() {
        save_new(&out.with_extension("overrides.json"), &json!(overrides))?;
    }
    save_new(&out.with_extension("publish-confirm.json"), &doc)?;
    Ok(Some(doc))
}

pub(super) fn review_with(
    input: &Value,
    decision_file: Option<&Path>,
    out: &Path,
    source: &Adjudicated,
) -> Result<Value> {
    // Build's existing review-file option also accepts the single D28 confirmation.
    let confirm_source = Adjudicated {
        raw_input: source.raw_input,
        adjudication: source.adjudication,
        confirm: decision_file
            .filter(|path| read(path).is_ok_and(|v| v["protocol"] == "oo.publish-confirm/v1")),
        overrides: source.overrides,
    };
    let (decision_file, source) = if confirm_source.confirm.is_some() {
        (None, &confirm_source)
    } else {
        (decision_file, source)
    };
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
    let beside = out
        .parent()
        .unwrap_or(Path::new("."))
        .join("adjudication.json");
    let adjudication = source
        .adjudication
        .map(Path::to_path_buf)
        .or_else(|| beside.exists().then_some(beside));
    let confirmed = match (&adjudication, decision_file) {
        (Some(path), None) => confirm_once(path, source, out, &mut screen)?,
        _ => None,
    };
    if let Some(doc) = &confirmed {
        let document = read(adjudication.as_ref().unwrap())?;
        let raw = source
            .raw_input
            .map(str::to_owned)
            .unwrap_or_else(|| input.to_string());
        if document["input_sha256"] != payload_sha256(raw.as_bytes())
            || serde_json::from_str::<Value>(&raw).ok().as_ref() != Some(input)
        {
            return Err(Failure::local("REVIEW_ADJUDICATION_INPUT_BINDING"));
        }
        let overrides_path = out.with_extension("overrides.json");
        let overrides = match source.overrides {
            Some(path) => read(path)?,
            None if overrides_path.exists() => read(&overrides_path)?,
            None => json!({"protocol":"oo.adjudication-override/v1",
                "summary_sha256":document["summary_sha256"],"items":{}}),
        };
        let command = json!({"adjudication_input":raw,"adjudication":document,
            "overrides":overrides,"confirmation":doc});
        // Apply evidence decisions in the compiler, never convert them to accept-all.
        let compiled = bridge(&json!({"operation":"reviewed","command":command}))?;
        let result = json!({"protocol":"oo.owner-reviewed-registry/v1","status":"reviewed",
            "published":false,"authentication":"not_claimed","review_decision":doc,
            "summary":compiled["summary"],"command":command,"compiled":compiled});
        save_new(out, &result)?;
        return Ok(result);
    }
    let candidates = items(input)?;
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
                    6,
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
            let reason = screen.prompt(6, &format!(
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
            "--input" | "--out" | "--review-file" | "--adjudication" | "--confirm"
            | "--overrides" => {
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
    let raw_input = read_text(Path::new(
        opts.get("--input")
            .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT"))?,
    ))?;
    let input: Value =
        serde_json::from_str(&raw_input).map_err(|_| Failure::local("REVIEW_JSON"))?;
    if !opts.contains_key("--out") {
        let pending = input
            .get("review_pending")
            .and_then(Value::as_array)
            .ok_or_else(|| Failure::usage("REVIEW_PENDING_REQUIRED"))?;
        return Ok(Invocation::AdminReport(
            json!({"review_pending":pending,"published":false}).to_string(),
            0,
        ));
    }
    let out = Path::new(
        opts.get("--out")
            .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT"))?,
    );
    if opts.contains_key("--review-file")
        && (opts.contains_key("--confirm") || opts.contains_key("--adjudication"))
    {
        return Err(Failure::usage(
            "ADMIN_ARGUMENT: --review-file excludes --adjudication/--confirm",
        ));
    }
    if (opts.contains_key("--confirm") || opts.contains_key("--overrides"))
        && !opts.contains_key("--adjudication")
    {
        return Err(Failure::usage(
            "ADMIN_ARGUMENT: --confirm needs --adjudication",
        ));
    }
    let source = Adjudicated {
        raw_input: Some(&raw_input),
        adjudication: opts.get("--adjudication").map(Path::new),
        confirm: opts.get("--confirm").map(Path::new),
        overrides: opts.get("--overrides").map(Path::new),
    };
    let result = review_with(
        &input,
        opts.get("--review-file").map(Path::new),
        out,
        &source,
    )?;
    Ok(Invocation::AdminReport(
        json!({"protocol":"oo.owner-review-report/v1","status":result["status"],
            "review_pending":input.get("review_pending").cloned().unwrap_or_else(|| json!([])),
            "published":false,"authentication":"not_claimed"})
        .to_string(),
        if result["status"] == "reviewed" { 0 } else { 1 },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjudicated_confirm_file_must_match_the_recomputed_summary() {
        use crate::tui::adjudication as adj;
        let dir = std::env::temp_dir().join(format!("oo-review-adj-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adjudication.json");
        let base = adj::load(&fixture).unwrap();
        let good = adj::confirm(
            &base,
            None,
            &base.summary_sha256,
            "ok",
            "2026-11-09T00:00:00Z",
        )
        .unwrap();
        let path = dir.join("confirm.json");
        fs::write(&path, good.to_string()).unwrap();
        let source = Adjudicated {
            raw_input: None,
            adjudication: Some(&fixture),
            confirm: Some(&path),
            overrides: None,
        };
        let doc = confirm_once(&fixture, &source, &dir.join("out.json"), &mut None)
            .ok()
            .unwrap()
            .unwrap();
        assert_eq!(doc, good);
        let mut forged = good.clone();
        forged["reason"] = json!("");
        fs::write(&path, forged.to_string()).unwrap();
        assert!(confirm_once(&fixture, &source, &dir.join("out.json"), &mut None).is_err());
        let mut stale = good;
        stale["summary_sha256"] = json!("0".repeat(64));
        fs::write(&path, stale.to_string()).unwrap();
        assert!(confirm_once(&fixture, &source, &dir.join("out.json"), &mut None).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pending_inspection_never_creates_approval() {
        let root = std::env::temp_dir().join(format!("oo-review-pending-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("builder.json");
        save_new(
            &path,
            &json!({"stage":"data_only","review_pending":[
            {"table":"app.items","changes":["comment","unreviewed_column:new"]}]}),
        )
        .ok()
        .unwrap();
        let result = run(&[
            "--input".into(),
            path.to_string_lossy().into_owned(),
            "--json".into(),
        ])
        .ok()
        .unwrap();
        let Invocation::AdminReport(body, code) = result else {
            panic!("report")
        };
        let report: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(code, 0);
        assert_eq!(report["published"], false);
        assert_eq!(report["review_pending"][0]["table"], "app.items");
        assert!(report.get("review_decision").is_none());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    // Real Builder (replay model) inputs and `oo review` outputs; see retained_fixture.py.
    const SCALAR_INPUT: &str = include_str!(concat!(
        "../../../../tools/release-coordinator/tests/fixtures/",
        "retained_review_input.json"
    ));
    const COMPOSITE_INPUT: &str = include_str!(concat!(
        "../../../../tools/release-coordinator/tests/fixtures/",
        "retained_review_input_composite.json"
    ));
    const COMPOSITE_REVIEWED: &str = include_str!(concat!(
        "../../../../tools/release-coordinator/tests/fixtures/",
        "retained_reviewed_composite.json"
    ));

    fn accept_all(input: &Value, items: &[Item]) -> Session {
        Session {
            protocol: "oo.owner-review/v1".into(),
            input_sha256: hash(input),
            reviewed_at: "2026-11-04".into(),
            items: items
                .iter()
                .map(|i| {
                    let decision = Decision {
                        action: "accept".into(),
                        replacement: None,
                        reason: String::new(),
                    };
                    (i.id.clone(), decision)
                })
                .collect(),
            g14: None,
        }
    }

    fn entity_item(items: &[Item]) -> &Item {
        items.iter().find(|i| i.id.starts_with("entity/")).unwrap()
    }

    #[test]
    fn declared_scalar_and_composite_keys_are_offered_and_accepted() {
        let scalar: Value = serde_json::from_str(SCALAR_INPUT).unwrap();
        let found = items(&scalar).ok().unwrap();
        assert_eq!(entity_item(&found).value["key"], json!("amount"));
        let command = prepare(&scalar, &found, &accept_all(&scalar, &found))
            .ok()
            .unwrap();
        let selection = command["review"]["entities"].as_object().unwrap();
        assert_eq!(
            selection.values().next().unwrap(),
            &json!({"key":"amount","display":"amount"})
        );

        let composite: Value = serde_json::from_str(COMPOSITE_INPUT).unwrap();
        let found = items(&composite).ok().unwrap();
        let entity = entity_item(&found);
        assert_eq!(entity.value["key"], json!(["order_id", "line_no"]));
        assert_eq!(entity.value["display"], json!(["order_id", "line_no"]));
        let command = prepare(&composite, &found, &accept_all(&composite, &found))
            .ok()
            .unwrap();
        let reviewed: Value = serde_json::from_str(COMPOSITE_REVIEWED).unwrap();
        // Everything the compiler hashes later matches the real `oo review` output.
        for key in ["draft", "effective", "classification_review"] {
            assert_eq!(command[key], reviewed["command"][key], "{key}");
        }
        assert_eq!(
            command["review"]["entities"],
            reviewed["command"]["review"]["entities"]
        );
    }

    #[test]
    fn keyless_entities_and_malformed_composite_edits_stay_refused() {
        let mut keyless: Value = serde_json::from_str(COMPOSITE_INPUT).unwrap();
        keyless["effective"]["datasets"][0]["primary_key"] = json!([]);
        let found = items(&keyless).ok().unwrap();
        assert_eq!(entity_item(&found).value["key"], Value::Null);
        let refused = prepare(&keyless, &found, &accept_all(&keyless, &found))
            .err()
            .unwrap();
        assert!(String::from_utf8_lossy(&refused.json_bytes()).contains("REVIEW_INPUT_SHAPE"));

        let composite: Value = serde_json::from_str(COMPOSITE_INPUT).unwrap();
        let found = items(&composite).ok().unwrap();
        let entity = entity_item(&found);
        for bad in [
            json!(["order_id"]),
            json!(["order_id", "order_id"]),
            json!(["order_id", ""]),
            json!([]),
            json!(""),
            json!(7),
        ] {
            let decision = Decision {
                action: "edit".into(),
                replacement: Some(json!({ "key": bad })),
                reason: "owner".into(),
            };
            assert!(apply(entity, &decision).is_err(), "{bad}");
        }
        let decision = Decision {
            action: "edit".into(),
            replacement: Some(json!({"display":"order_id"})),
            reason: "owner label".into(),
        };
        let edited = apply(entity, &decision).ok().unwrap();
        assert_eq!(edited["key"], json!(["order_id", "line_no"]));
        assert_eq!(edited["display"], json!("order_id"));
    }

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
