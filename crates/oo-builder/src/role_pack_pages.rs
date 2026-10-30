//! Metric navigation and lossless oversized definitions share the G9 estimator.
use super::*;
use crate::knowledge_budget::unicode_word_tokens;

pub(super) fn metrics(
    files: &mut BTreeMap<String, String>,
    model: &Model,
    selected: &Selected,
) -> Result<(), RolePackError> {
    let budget = u64::from(model.ontology.compile.budgets.readme_tokens
        .min(model.ontology.compile.budgets.table_file_tokens));
    let fits = |text: &str| unicode_word_tokens(text) <= budget
        && text.len() as u64 <= model.limits.max_file_bytes;
    let all = render_metrics(model, selected)?;
    if fits(&all) {
        return add_file(files, "metrics.md", all);
    }
    let mut entries = Vec::new();
    for (metric, candidate) in selected.metrics.iter().map(|m| (m, false))
        .chain(selected.metric_candidates.iter().map(|m| (m, true)))
    {
        let name = metric.metric.as_str();
        let path = format!("metrics/{name}.md");
        let detail = render_metrics_filtered(model, selected, Some(name))?;
        if fits(&detail) {
            add_file(files, &path, detail)?;
        } else {
            fragments(files, name, candidate, &detail, &fits)?;
        }
        entries.push(format!("- [{name}]({path}){}\n",
            if candidate { " (NOT CERTIFIED)" } else { "" }));
    }
    let header = "# metrics\n\nRead the linked definition before querying. Candidate entries are NOT CERTIFIED; file presence does not verify runtime execution or business readiness.\n\n";
    let index_path = |page: usize| if page == 1 {
        "metrics.md".to_owned()
    } else {
        format!("metrics/_index/{page:04}.md")
    };
    let mut page = 1;
    let mut text = header.to_owned();
    let mut has_entry = false;
    for entry in entries {
        let next = format!("\nNext: [{}]({})\n", index_path(page + 1), index_path(page + 1));
        if !fits(&format!("{text}{entry}{next}")) {
            if !has_entry {
                return Err(page_budget(&index_path(page)));
            }
            text.push_str(&next);
            add_file(files, &index_path(page), text)?;
            page += 1;
            text = header.to_owned();
            let next = format!("\nNext: [{}]({})\n", index_path(page + 1), index_path(page + 1));
            if !fits(&format!("{text}{entry}{next}")) {
                return Err(page_budget(&index_path(page)));
            }
        }
        text.push_str(&entry);
        has_entry = true;
    }
    if !fits(&text) {
        return Err(page_budget(&index_path(page)));
    }
    add_file(files, &index_path(page), text)
}

fn page_budget(path: &str) -> RolePackError {
    RolePackError::new("budget.metric_navigation", path,
        "the per-file budget cannot hold a metric navigation entry and continuation metadata")
}

fn fragments(
    files: &mut BTreeMap<String, String>,
    name: &str,
    candidate: bool,
    original: &str,
    fits: &impl Fn(&str) -> bool,
) -> Result<(), RolePackError> {
    let path = |part: usize| format!("metrics/{name}/part-{part:04}.json");
    let digest = sha256_hex(original.as_bytes());
    let status = if candidate { "UNREVIEWED / NOT CERTIFIED" } else {
        "Input certification only; business readiness and runtime execution not verified"
    };
    let index = format!("# Metric {name}\n\n{status}.\n\nThis complete definition spans lossless UTF-8 fragments. Follow `next` until null and concatenate every `fragment` in sequence; verify the SHA-256 below. An individual fragment is not complete SQL and must not be executed as a query.\n\nDefinition SHA-256: `{digest}`\n\nStart: [{}]({})\n", path(1), path(1));
    if !fits(&index) {
        return Err(page_budget(&format!("metrics/{name}.md")));
    }
    add_file(files, &format!("metrics/{name}.md"), index)?;
    let boundaries: Vec<usize> = original.char_indices().map(|(i, _)| i)
        .chain(std::iter::once(original.len())).collect();
    let mut start = 0;
    let mut part = 1;
    while start + 1 < boundaries.len() {
        let render = |end: usize| canonical_json(&serde_json::json!({
            "protocol": "oo.knowledge.metric-fragment/v1", "metric": name,
            "review_status": status, "sequence": part, "definition_sha256": digest,
            "fragment": &original[boundaries[start]..boundaries[end]],
            "next": if end + 1 == boundaries.len() { None } else { Some(path(part + 1)) },
            "complete_query": false,
        }));
        let mut low = start + 1;
        let mut high = boundaries.len() - 1;
        let mut accepted = None;
        while low <= high {
            let middle = low + (high - low) / 2;
            if fits(&render(middle)) {
                accepted = Some(middle);
                low = middle + 1;
            } else {
                high = middle - 1;
            }
        }
        let end = accepted.ok_or_else(|| page_budget(&path(part)))?;
        add_file(files, &path(part), render(end))?;
        start = end;
        part += 1;
    }
    Ok(())
}

pub(super) fn manifest_index(
    files: &mut BTreeMap<String, String>,
    manifest: &mut Value,
    model: &Model,
) -> Result<(), RolePackError> {
    let root_budget = u64::from(model.ontology.compile.budgets.readme_tokens);
    let page_budget = root_budget.min(u64::from(model.ontology.compile.budgets.table_file_tokens));
    let fits = |body: &str, budget: u64| unicode_word_tokens(body) <= budget
        && body.len() as u64 <= model.limits.max_file_bytes;
    if fits(&canonical_json(manifest), root_budget) { return Ok(()) }
    let records = manifest["files"].as_array().expect("compiler file records").clone();
    let path = |number: usize| format!("manifest/files-{number:08}.json");
    let page = |records: &[Value], next: &Value| canonical_json(&serde_json::json!({
        "protocol": "oo.knowledge.file-index-page/v1", "files": records, "next": next,
    }));
    let placeholder = |number| serde_json::json!({
        "path": path(number), "bytes": u64::MAX, "sha256": "0".repeat(64),
    });
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for record in &records {
        group.push(record.clone());
        if !fits(&page(&group, &placeholder(groups.len() + 2)), page_budget) {
            let last = group.pop().expect("new record");
            if group.is_empty() { return Err(self::page_budget("manifest.json")) }
            groups.push(std::mem::take(&mut group));
            group.push(last);
            if !fits(&page(&group, &placeholder(groups.len() + 2)), page_budget) {
                return Err(self::page_budget("manifest.json"));
            }
        }
    }
    if !group.is_empty() { groups.push(group) }
    let mut next = Value::Null;
    for (index, group) in groups.iter().enumerate().rev() {
        let body = page(group, &next);
        let name = path(index + 1);
        next = serde_json::json!({
            "path": name, "bytes": body.len(), "sha256": sha256_hex(body.as_bytes()),
        });
        add_file(files, &name, body)?;
    }
    manifest["files"] = serde_json::json!([]);
    manifest["file_index"] = serde_json::json!({
        "protocol": "oo.knowledge.file-index/v1", "entry_count": records.len(), "first": next,
    });
    if !fits(&canonical_json(manifest), root_budget) {
        return Err(self::page_budget("manifest.json"));
    }
    Ok(())
}
