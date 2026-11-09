//! Advisory vocabulary coverage, never SQL authorization or a claim of join,
//! metric correctness, data completeness, or runtime answerability.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::pack_catalog::{PackCatalog, PackMetadata};

#[derive(Default)]
pub struct CoverageIndex {
    terms: BTreeMap<String, BTreeSet<String>>,
}

fn stem(word: &str) -> String {
    let word = word.to_lowercase();
    match word.as_str() {
        "this" | "his" | "its" | "has" | "was" => word,
        "quantities" | "qty" | "quantity" => "quantity".into(),
        "ordered" | "ordering" => "order".into(),
        "shipped" | "shipping" => "shipment".into(),
        "dates" | "date" | "day" | "days" | "month" | "months" => "calendar".into(),
        "january" | "february" | "march" | "april" | "may" | "june" | "july" | "august"
        | "september" | "october" | "november" | "december" => "calendar".into(),
        _ if word.len() > 4 && word.ends_with("ies") => format!("{}y", &word[..word.len() - 3]),
        _ if word.len() > 3
            && word.ends_with('s')
            && !word.ends_with("ss")
            && !word.ends_with("us") =>
        {
            word[..word.len() - 1].into()
        }
        _ => word,
    }
}

const SYNONYMS: &[(&str, &str)] = &[
    ("订单", "order"),
    ("客户", "customer"),
    ("顾客", "customer"),
    ("运单", "shipment"),
    ("发货", "shipment"),
    ("金额", "amount"),
    ("数量", "quantity"),
    ("区域", "region"),
    ("地区", "region"),
    ("站点", "site"),
    ("状态", "status"),
    ("备注", "note"),
    ("日期", "calendar"),
    ("日历", "calendar"),
    ("月份", "calendar"),
    ("事件", "event"),
    ("明细", "line"),
];

fn normalize(text: &str) -> String {
    let mut text = text
        .to_lowercase()
        .replace("'s", "")
        .replace('\u{2019}', "'");
    // Phrase aliases preserve the business head noun. Unknown nouns beside
    // these phrases still produce gaps (e.g. "bulk event profitability").
    for (from, to) in [
        ("destination site", "site"),
        ("bulk events", "events"),
        ("bulk event", "event"),
    ] {
        text = text.replace(from, to);
    }
    for (from, to) in SYNONYMS {
        text = text.replace(from, &format!(" {to} "));
    }
    text
}

fn words(text: &str) -> Vec<String> {
    normalize(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(stem)
        .collect()
}

fn structural(word: &str) -> bool {
    word.chars().all(|c| c.is_ascii_digit())
        || matches!(
            word,
            "how" | "many" | "much" | "what" | "which" | "who" | "is" | "are" | "was" | "were"
        | "the" | "a" | "an" | "this" | "that" | "these" | "those" | "of" | "for"
        | "in" | "on" | "at" | "by" | "per" | "to" | "from" | "with" | "without"
        | "and" | "or" | "not" | "no" | "all" | "any" | "each" | "as" | "can" | "see"
        | "show" | "list" | "find" | "give" | "get" | "tell" | "me" | "please"
        | "count" | "total" | "sum" | "average" | "minimum" | "maximum" | "min" | "max"
        | "distinct" | "exist" | "existing" | "exact" | "number" | "one"
        | "row" | "join" | "joined" | "through" | "across" | "between" | "under"
        | "null" | "empty" | "nonempty" | "belong" | "have" | "has" | "do" | "does"
        | "answer" | "order" // Order is also an entity; handled before structural filtering.
        | "id" | "key" | "composite" | "role" | "demo" | "source" | "portable"
        | "semantic" | "implicitly" | "reach" | "relation" | "placed" | "translated"
        | "是多少" | "多少" | "有多少" | "有" | "的" | "总" | "总计" | "合计" | "按"
        | "查询" | "显示" | "请问" | "请" | "和" | "与" | "统计"
        )
}

impl CoverageIndex {
    pub fn from_pack(catalog: &PackCatalog, pack: &PackMetadata) -> Self {
        let mut index = Self::default();
        for column in &pack.columns {
            index.add(&column.table, &column.table);
            index.add(&column.name, &column.table);
        }
        for file in &pack.files {
            let Some(body) = catalog.file_text(
                &pack.project,
                &pack.role,
                &pack.policy_version,
                &pack.knowledge_version,
                &file.path,
            ) else {
                continue;
            };
            let text = body.text;
            if file.path.starts_with("relationships/graph") && file.path.ends_with(".json") {
                if let Ok(graph) = serde_json::from_str::<Value>(&text)
                    && let Some(entities) = graph["entities"].as_array()
                {
                    for entity in entities {
                        let Some(table) = entity["main_table"]
                            .as_str()
                            .filter(|table| pack.columns.iter().any(|c| c.table == *table))
                        else {
                            continue;
                        };
                        for key in ["id", "canonical_name"] {
                            if let Some(name) = entity[key].as_str() {
                                index.add(name, table);
                            }
                        }
                        if let Some(aliases) = entity["aliases"].as_array() {
                            for alias in aliases.iter().filter_map(Value::as_str) {
                                index.add(alias, table);
                            }
                        }
                    }
                }
            } else if file.path.starts_with("summaries/") && file.path.ends_with(".json") {
                if let Ok(doc) = serde_json::from_str::<Value>(&text)
                    && let Some(items) = doc["items"].as_array()
                {
                    for item in items {
                        let Some(table) = item["dataset"].as_str() else {
                            continue;
                        };
                        let visible = pack.columns.iter().any(|c| {
                            c.table == table
                                && item["name"].as_str().is_none_or(|name| name == c.name)
                        });
                        if !visible || item["unreviewed"] == true {
                            continue;
                        }
                        for key in ["title", "description"] {
                            if let Some(value) = item[key].as_str() {
                                index.add(value, table);
                            }
                        }
                    }
                }
            } else if file.path.starts_with("tables/") {
                let table = file
                    .path
                    .trim_start_matches("tables/")
                    .trim_end_matches(".md");
                if !pack.columns.iter().any(|c| c.table == table) {
                    continue;
                }
                if let Some(title) = text.lines().next() {
                    index.add(title, table);
                }
                for line in text.lines().filter(|line| line.starts_with("| `")) {
                    let cells: Vec<_> = line.split('|').map(str::trim).collect();
                    if cells.len() >= 7
                        && pack
                            .columns
                            .iter()
                            .any(|c| c.table == table && c.name == cells[1].trim_matches('`'))
                    {
                        index.add(cells[2], table);
                    }
                }
            } else if file.path.starts_with("entities/") {
                let table = text
                    .lines()
                    .find_map(|line| line.strip_prefix("primary dataset: `"))
                    .and_then(|s| s.strip_suffix('`'));
                if let Some(table) = table.filter(|t| pack.columns.iter().any(|c| c.table == *t)) {
                    if let Some(title) = text.lines().next() {
                        index.add(title, table);
                    }
                    for line in text
                        .lines()
                        .filter(|s| s.starts_with("alias: ") || s.starts_with("synonym: "))
                    {
                        index.add(line.split_once(": ").unwrap().1, table);
                    }
                }
            } else if file.path == "metrics.md"
                || file.path.starts_with("metrics/") && file.path.ends_with(".md")
            {
                let mut certified = false;
                for line in text.lines() {
                    if line.starts_with("## ") {
                        certified = true;
                        index.add(line, &file.path);
                    }
                    if line.starts_with("### Candidate:") {
                        certified = false;
                    }
                    if certified && let Some(synonym) = line.strip_prefix("synonym: ") {
                        index.add(synonym, &file.path);
                    }
                }
            } else if file.path.starts_with("enums/") {
                for line in text.lines().filter(|line| line.starts_with("| `")) {
                    let cells: Vec<_> = line.split('|').map(str::trim).collect();
                    if cells.len() >= 6 && cells[4] == "approved" {
                        for cell in &cells[1..4] {
                            index.add(cell, &file.path);
                        }
                    }
                }
            } else if file.path == "glossary.md" {
                for line in text.lines().filter_map(|line| line.strip_prefix("## ")) {
                    index.add(line, &file.path);
                }
            }
        }
        index
    }

    fn add(&mut self, text: &str, source: &str) {
        for term in words(text) {
            self.terms.entry(term).or_default().insert(source.into());
        }
    }

    pub fn assess(&self, question: &str) -> Value {
        let mut matches = BTreeSet::new();
        let mut missing = BTreeSet::new();
        for term in words(question) {
            if let Some(sources) = self.terms.get(&term) {
                matches.extend(sources.iter().cloned());
            } else if !structural(&term) {
                // Longest known-word segmentation for unspaced CJK labels;
                // unmatched characters remain gaps, not silent stop words.
                let mut rest = term.as_str();
                if !term.is_ascii() {
                    while !rest.is_empty() {
                        let found = self
                            .terms
                            .keys()
                            .filter(|s| !s.is_ascii() && rest.starts_with(s.as_str()))
                            .max_by_key(|s| s.len());
                        if let Some(found) = found {
                            matches.extend(self.terms[found].iter().cloned());
                            rest = &rest[found.len()..];
                        } else {
                            break;
                        }
                    }
                }
                if !rest.is_empty() {
                    missing.insert(rest.to_owned());
                }
            }
        }
        json!({
            "coverage": if matches.is_empty() { "not_covered" } else if missing.is_empty() { "covered" } else { "partial" },
            "advisory": true, "blocking": false, "assessment":"published_semantic_vocabulary",
            "matched_sources":matches, "unmatched_term_count":missing.len(),
            "gaps": if matches.is_empty() { vec!["no_visible_semantic_match"] }
                else if !missing.is_empty() { vec!["unmapped_question_terms"] } else { vec![] },
            "execution_verified":false
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bilingual_terms_stemming_and_unknown_concepts_remain_advisory() {
        let mut index = CoverageIndex::default();
        index.add("orders amount quantity", "demo.orders");
        assert_eq!(
            index.assess("What is the total order amount?")["coverage"],
            "covered"
        );
        assert_eq!(index.assess("订单金额是多少")["coverage"], "covered");
        assert_eq!(
            index.assess("Total order profitability")["coverage"],
            "partial"
        );
        assert_eq!(
            index.assess("List employee salaries")["coverage"],
            "not_covered"
        );
        assert_eq!(index.assess("preorders")["coverage"], "not_covered");
        assert_eq!(index.assess("orders")["blocking"], false);
    }
}
