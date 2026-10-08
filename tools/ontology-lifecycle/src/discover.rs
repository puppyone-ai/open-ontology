//! Producer readiness is the response schema, not an operationId line.
//!
//! A whole line `operationId: exportGeneratedOos` does not make an operation
//! ready. Any operation name is eligible when its success schema requires the
//! fields for that chain edge and does not carry a disqualifying const.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub id: String,
    pub method: String,
    pub path: String,
}

pub fn matching_operation(
    text: &str,
    schemas: &[(&str, &str)],
    required: &[&str],
    evidence: &[&str],
    forbid: &[(&str, &str)],
) -> Option<Operation> {
    for (id, method, path, section) in operation_sections(text) {
        let Some(reference) = success_ref(&section) else {
            continue;
        };
        let Some(schema) = resolve_schema(text, schemas, &reference) else {
            continue;
        };
        if !schema_matches(schema, required, evidence, forbid) {
            continue;
        }
        return Some(Operation { id, method, path });
    }
    None
}

pub fn matching_contract(
    text: &str,
    required: &[&str],
    evidence: &[&str],
    forbid: &[(&str, &str)],
) -> Option<Operation> {
    for section in sections(text) {
        if let Some(operation) = one_contract(section, required, evidence, forbid) {
            return Some(operation);
        }
    }
    None
}

fn one_contract(
    text: &str,
    required: &[&str],
    evidence: &[&str],
    forbid: &[(&str, &str)],
) -> Option<Operation> {
    let mut method = None;
    let mut path = None;
    let mut fields: Option<Vec<String>> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("method:") {
            method = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("path:") {
            path = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("required_response:") {
            fields = Some(
                rest.split(',')
                    .map(|part| part.trim().to_owned())
                    .filter(|part| !part.is_empty())
                    .collect(),
            );
        }
    }
    let fields = fields?;
    if !required.iter().all(|item| field_satisfies(&fields, item)) {
        return None;
    }
    if !evidence.iter().all(|token| text.contains(token)) {
        return None;
    }
    if forbid.iter().any(|(key, value)| text.contains(&format!("{key}: {value}"))) {
        return None;
    }
    Some(Operation {
        id: "contract-response".to_owned(),
        method: method?,
        path: path?,
    })
}

fn sections(text: &str) -> Vec<&str> {
    let mut cuts = vec![0usize];
    let mut offset = 0usize;
    for line in text.lines() {
        if offset != 0 && line.starts_with("## ") {
            cuts.push(offset);
        }
        offset += line.len() + 1;
    }
    let mut out = Vec::with_capacity(cuts.len());
    for (index, start) in cuts.iter().enumerate() {
        let end = cuts.get(index + 1).copied().unwrap_or(text.len()).min(text.len());
        let start = (*start).min(text.len());
        if start < end {
            out.push(&text[start..end]);
        }
    }
    if out.is_empty() {
        out.push(text);
    }
    out
}

fn field_satisfies(present: &[String], required: &str) -> bool {
    let aliases: &[&str] = match required {
        "commit" => &["commit", "new_commit"],
        "tree" => &["tree", "new_tree"],
        "expected_head" => &["expected_head", "old_commit"],
        "curated_blob" => &["curated_blob", "blobs"],
        other => return present.iter().any(|got| got == other),
    };
    aliases.iter().any(|alias| present.iter().any(|got| got == *alias))
}

fn schema_matches(schema: &str, required: &[&str], evidence: &[&str], forbid: &[(&str, &str)]) -> bool {
    let names = required_names(schema);
    required.iter().all(|item| field_satisfies(&names, item))
        && evidence.iter().all(|token| schema.contains(token))
        && !forbid.iter().any(|(key, value)| has_const(schema, key, value))
}

fn resolve_schema<'a>(text: &'a str, schemas: &'a [(&'a str, &'a str)], reference: &str) -> Option<&'a str> {
    if let Some(name) = reference.strip_prefix("#/components/schemas/") {
        return component(text, name);
    }
    schemas
        .iter()
        .find(|(key, _)| reference.ends_with(key))
        .map(|(_, body)| *body)
}

fn operation_sections(text: &str) -> Vec<(String, String, String, String)> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(id) = trimmed.strip_prefix("operationId:") {
            let id = id.trim();
            if !id.is_empty() && !id.contains(' ') {
                starts.push((offset, id.to_owned()));
            }
        }
        offset += line.len() + 1;
    }
    let mut out = Vec::new();
    for (index, (start, id)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map(|(next, _)| *next).unwrap_or(text.len());
        let section = text.get(*start..end).unwrap_or("");
        let (method, path) = context(&text[..*start]);
        if method.is_empty() || path.is_empty() {
            continue;
        }
        out.push((id.clone(), method, path, section.to_owned()));
    }
    out
}

fn context(before: &str) -> (String, String) {
    let mut method = String::new();
    let mut path = String::new();
    for line in before.lines().rev() {
        let trimmed = line.trim();
        if method.is_empty()
            && matches!(trimmed, "get:" | "post:" | "put:" | "delete:")
            && line.starts_with("    ")
            && !line.starts_with("     ")
        {
            method = trimmed.trim_end_matches(':').to_ascii_uppercase();
        }
        if path.is_empty()
            && trimmed.starts_with('/')
            && trimmed.ends_with(':')
            && line.starts_with("  ")
            && !line.starts_with("   ")
        {
            path = trimmed.trim_end_matches(':').to_owned();
        }
        if !method.is_empty() && !path.is_empty() {
            break;
        }
    }
    (method, path)
}

fn success_ref(section: &str) -> Option<String> {
    let marker = ["\"200\"", "\"202\""]
        .iter()
        .filter_map(|token| section.find(token))
        .min()?;
    let rest = &section[marker..];
    let schema_at = rest.find("schema:")?;
    let after = &rest[schema_at..];
    let ref_at = after.find("$ref:")?;
    let value = after[ref_at + 5..].trim_start().lines().next()?.trim().trim_matches('"');
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn component<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let header = format!("    {name}:");
    let mut offset = 0;
    let mut start = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if start.is_none() && line == header {
            start = Some(offset + line.len() + 1);
        } else if start.is_some() && !line.is_empty() && indent <= 4 {
            return Some(&text[start.expect("start")..offset]);
        }
        offset += line.len() + 1;
    }
    start.map(|start| &text[start..])
}

fn required_names(schema: &str) -> Vec<String> {
    let inline = schema.find("required:").map(|start| &schema[start + "required:".len()..]);
    let json = schema.find("\"required\"").map(|start| &schema[start + "\"required\"".len()..]);
    let Some(rest) = inline.or(json) else {
        return Vec::new();
    };
    let Some(begin) = rest.find('[') else {
        return Vec::new();
    };
    let Some(end) = rest[begin..].find(']') else {
        return Vec::new();
    };
    rest[begin + 1..begin + end]
        .split(',')
        .map(|part| part.trim().trim_matches('"').to_owned())
        .filter(|part| !part.is_empty() && !part.contains('{') && !part.contains(':'))
        .collect()
}

fn has_const(schema: &str, key: &str, value: &str) -> bool {
    [
        format!("{key}: {{const: {value}}}"),
        format!("{key}: {{const: \"{value}\"}}"),
        format!("\"{key}\": {{\"const\": \"{value}\"}}"),
        format!("\"{key}\": {{\"const\": {value}}}"),
    ]
    .iter()
    .any(|pattern| schema.contains(pattern))
}

pub fn file_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in text.lines() {
        let Some((_, value)) = line.split_once("$ref:") else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if value.starts_with('#') || value.is_empty() {
            continue;
        }
        refs.push(value.to_owned());
    }
    refs
}

pub fn lexical_join(base: &str, reference: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    if !parts.is_empty() {
        parts.pop();
    }
    for part in reference.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop()?;
            continue;
        }
        if part.contains('\\') {
            return None;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{lexical_join, matching_contract, matching_operation};

    const REQUIRED: &[&str] = &[
        "ontology_yaml_sha256",
        "generated_files",
        "source_seal_sha256",
        "spec_version",
        "table_row_counts",
    ];

    fn yaml_export() -> String {
        "paths:\n  /control/v1/ontology/generated:\n    post:\n      operationId: materializeGeneratedYaml\n      responses:\n        \"200\":\n          content:\n            application/json:\n              schema:\n                $ref: \"#/components/schemas/GeneratedYaml\"\ncomponents:\n  schemas:\n    GeneratedYaml:\n      required: [ontology_yaml_sha256, generated_files, source_seal_sha256, spec_version, table_row_counts]\n      properties:\n        spec_version: {const: oos/v1}\n"
            .to_owned()
    }

    #[test]
    fn an_equivalent_name_matches_when_the_schema_has_the_fields() {
        let found = matching_operation(&yaml_export(), &[], REQUIRED, &[], &[]).expect("operation");
        assert_eq!(found.id, "materializeGeneratedYaml");
        assert_eq!(found.method, "POST");
        assert_eq!(found.path, "/control/v1/ontology/generated");
    }

    #[test]
    fn a_marker_line_is_not_a_schema() {
        let text = "operationId: exportGeneratedOos\n";
        assert!(matching_operation(text, &[], REQUIRED, &[], &[]).is_none());
    }

    #[test]
    fn a_pack_const_hides_the_same_field_names() {
        let text = yaml_export().replace(
            "spec_version: {const: oos/v1}\n",
            "spec_version: {const: oos/v1}\n        compiled_pack: {const: not_published}\n",
        );
        assert!(matching_operation(&text, &[], REQUIRED, &[], &[("compiled_pack", "not_published")]).is_none());
    }

    #[test]
    fn contract_fields_match_without_an_operation_id() {
        let text = "method: POST\npath: /control/v1/curated/tree\nrequired_response: commit, tree, expected_head, ontology_blob, decisions_blob, curated_blob\n";
        let required = [
            "commit",
            "tree",
            "expected_head",
            "ontology_blob",
            "decisions_blob",
            "curated_blob",
        ];
        let found = matching_contract(text, &required, &[], &[]).expect("contract");
        assert_eq!(found.path, "/control/v1/curated/tree");
        let denied = format!("{text}authenticated: false\n");
        assert!(matching_contract(&denied, &required, &[], &[("authenticated", "false")]).is_none());
    }

    #[test]
    fn an_older_unauthenticated_section_does_not_hide_a_later_response() {
        let required = [
            "commit",
            "tree",
            "expected_head",
            "ontology_blob",
            "decisions_blob",
            "curated_blob",
        ];
        let text = "\
## E57
authenticated: false
method: POST
path: /control/v1/curated/description
required_response: commit, tree, expected_head, ontology_blob, decisions_blob, curated_blob
## Reviewed tree
operationId: applyReviewedTree
method: POST
path: /control/v1/curated/reviewed-tree
required_response: old_commit, new_commit, new_tree, ontology_blob, decisions_blob, blobs
";
        let found = matching_contract(text, &required, &[], &[("authenticated", "false")]).expect("later section");
        assert_eq!(found.path, "/control/v1/curated/reviewed-tree");
        let marker_only = "\
## E57
authenticated: false
## Reviewed tree
operationId: applyReviewedTree
This operation does not emit `authenticated` false.
";
        assert!(matching_contract(marker_only, &required, &[], &[("authenticated", "false")]).is_none());
    }

    #[test]
    fn a_schema_ref_cannot_escape_the_producer_directory() {
        assert_eq!(
            lexical_join("tools/release-control-api/openapi/release-control-api.openapi.yaml", "../schema/release.schema.json").as_deref(),
            Some("tools/release-control-api/schema/release.schema.json")
        );
        let escaped = lexical_join("tools/release-control-api/openapi/file.yaml", "../../secrets").expect("join");
        assert_eq!(escaped, "tools/secrets");
        assert!(!escaped.starts_with("tools/release-control-api/"));
    }
}
