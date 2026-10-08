use crate::audit::{load_body, Pair, PairKind};
use crate::error::Fault;
use crate::json::{parse_json, JsonError};
use crate::model::{
    caller_label, hex_lower, role_name, slug, Admitted, Binding, CodedId, CitationOut, Disclosure,
    Input, ReceiptOut, Report, ANSWER_MAX, BODY_MAX, CITATION_MAX, PROTOCOL, RECEIPT_MAX,
    STDIN_MAX, TRUST, VERSION,
};
use crate::sha256::sha256_hex;
use crate::Outcome;

pub fn validate(bytes: &[u8]) -> Result<Outcome, Fault> {
    if bytes.is_empty() {
        return Err(Fault::request());
    }
    if bytes.len() > STDIN_MAX {
        return Err(Fault::budget());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::request())?;
    let value = match parse_json(text) {
        Ok(value) => value,
        Err(JsonError::Budget) => return Err(Fault::budget()),
        Err(JsonError::Syntax) => return Err(Fault::request()),
    };
    if contains_null(&value) {
        return Err(Fault::request());
    }
    let input: Input = serde_json::from_value(value).map_err(|_| Fault::request())?;
    check_input(&input)?;
    let citations = scan_citations(&input.answer.text);
    if citations.len() > CITATION_MAX {
        return Err(Fault::budget());
    }
    let mut pairs = Vec::new();
    let mut receipts = Vec::with_capacity(input.receipts.len());
    let mut pair_ordinal = 0_u32;
    for (ordinal, receipt) in input.receipts.iter().enumerate() {
        let (loaded, next) = load_body(
            &receipt.body,
            &input.binding,
            u32::try_from(ordinal).map_err(|_| Fault::budget())?,
            pair_ordinal,
        )?;
        pair_ordinal = next;
        pairs.extend(loaded);
        receipts.push(ReceiptOut {
            ordinal: u32::try_from(ordinal).map_err(|_| Fault::budget())?,
            receipt_sha256: receipt.receipt_sha256.clone(),
            status: "bound",
        });
    }
    classify(&input, receipts, &pairs, &citations)
}

fn check_input(input: &Input) -> Result<(), Fault> {
    if input.version != VERSION {
        return Err(Fault::request());
    }
    if input.trust != TRUST {
        return Err(Fault::binding());
    }
    check_binding(&input.binding)?;
    if input.receipts.len() > RECEIPT_MAX {
        return Err(Fault::budget());
    }
    if input.answer.text.len() > ANSWER_MAX {
        return Err(Fault::budget());
    }
    for receipt in &input.receipts {
        if !hex_lower(&receipt.receipt_sha256, 64) {
            return Err(Fault::request());
        }
        let size = receipt.body.len();
        if size > BODY_MAX {
            return Err(Fault::budget());
        }
        if size == 0 {
            return Err(Fault::receipt());
        }
        if sha256_hex(receipt.body.as_bytes()) != receipt.receipt_sha256 {
            return Err(Fault::hash());
        }
    }
    Ok(())
}

fn check_binding(binding: &Binding) -> Result<(), Fault> {
    if !caller_label(&binding.session_id) || !caller_label(&binding.turn_id) {
        return Err(Fault::binding());
    }
    if !role_name(&binding.role) || !slug(&binding.project, 63) {
        return Err(Fault::binding());
    }
    if binding.protocol != PROTOCOL {
        return Err(Fault::binding());
    }
    Ok(())
}

fn contains_null(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(contains_null),
        serde_json::Value::Object(map) => map.values().any(contains_null),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {
            false
        }
    }
}

fn scan_citations(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        if let Some(body) = citation_body(&bytes[index..]) {
            let start = index + 1;
            let end = start + body;
            let Ok(id) = std::str::from_utf8(&bytes[start..end]) else {
                index += 1;
                continue;
            };
            if !found.iter().any(|item| item == id) {
                found.push(id.to_string());
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    found
}

fn citation_body(bytes: &[u8]) -> Option<usize> {
    let rest = bytes.get(1..)?;
    if rest.len() < 8 || rest[0] != b'q' || rest[1] != b'_' {
        return None;
    }
    let mut length = 2;
    while length < rest.len() && length <= 42 {
        let byte = rest[length];
        if byte == b']' {
            let tail = length - 2;
            if (6..=40).contains(&tail)
                && rest[2..length]
                    .iter()
                    .all(|item| item.is_ascii_alphanumeric())
            {
                return Some(length);
            }
            return None;
        }
        if !byte.is_ascii_alphanumeric() {
            return None;
        }
        length += 1;
    }
    None
}

struct Group {
    id: String,
    indexes: Vec<usize>,
}

fn classify(
    input: &Input,
    receipts: Vec<ReceiptOut>,
    pairs: &[Pair],
    cited: &[String],
) -> Result<Outcome, Fault> {
    let mut groups: Vec<Group> = Vec::new();
    for (index, pair) in pairs.iter().enumerate() {
        let Some(id) = &pair.query_id else {
            continue;
        };
        if let Some(group) = groups.iter_mut().find(|group| group.id == *id) {
            group.indexes.push(index);
        } else {
            groups.push(Group {
                id: id.clone(),
                indexes: vec![index],
            });
        }
    }
    let mut admitted = Vec::new();
    let mut warnings = Vec::new();
    let mut disclosures = Vec::new();
    let mut classes = Vec::with_capacity(groups.len());
    for group in &groups {
        let decision = decide(pairs, &group.indexes);
        if group.indexes.len() > 1 {
            warnings.push(CodedId {
                code: "duplicate",
                query_id: group.id.clone(),
            });
        }
        if let Some(row) = decision.admitted {
            if row.truncated {
                warnings.push(CodedId {
                    code: "querytruncated",
                    query_id: group.id.clone(),
                });
                disclosures.push(Disclosure {
                    code: "querytruncated",
                    query_id: group.id.clone(),
                    required: true,
                    comprehended: "not_judged",
                });
            }
            admitted.push(row);
            classes.push("admitted");
        } else {
            classes.push(decision.class_if_absent);
        }
    }
    let mut citations = Vec::with_capacity(cited.len());
    for (ordinal, id) in cited.iter().enumerate() {
        let classification = groups
            .iter()
            .position(|group| group.id == *id)
            .map_or("unknown", |pos| classes[pos]);
        if classification != "admitted" {
            warnings.push(CodedId {
                code: classification,
                query_id: id.clone(),
            });
        }
        let ordinal = u32::try_from(ordinal).map_err(|_| Fault::budget())?;
        citations.push(CitationOut {
            query_id: id.clone(),
            ordinal,
            classification,
        });
    }
    let missing_citation =
        input.answer.declares_data && !citations.iter().any(|item| item.classification == "admitted");
    let valid = !missing_citation && citations.iter().all(|item| item.classification == "admitted");
    let report = Report {
        version: VERSION,
        trust: TRUST,
        authenticated_audit: false,
        entailment: "not_judged",
        warning_comprehension: "not_judged",
        session_id: input.binding.session_id.clone(),
        turn_id: input.binding.turn_id.clone(),
        role: input.binding.role.clone(),
        project: input.binding.project.clone(),
        protocol: PROTOCOL,
        citation_status: if valid { "ok" } else { "invalid" },
        declares_data: input.answer.declares_data,
        missing_citation,
        receipts,
        admitted,
        citations,
        warnings,
        disclosures,
    };
    Ok(if valid {
        Outcome::Valid(report)
    } else {
        Outcome::Invalid(report)
    })
}

struct Decision {
    admitted: Option<Admitted>,
    class_if_absent: &'static str,
}

fn decide(pairs: &[Pair], indexes: &[usize]) -> Decision {
    let mut successes = Vec::new();
    for index in indexes {
        if pairs[*index].in_scope && matches!(pairs[*index].kind, PairKind::Success { .. }) {
            successes.push(*index);
        }
    }
    if !successes.is_empty() {
        let PairKind::Success { truncated } = pairs[successes[0]].kind else {
            return Decision {
                admitted: None,
                class_if_absent: "incomplete",
            };
        };
        let conflict = successes.iter().any(|index| {
            !matches!(
                pairs[*index].kind,
                PairKind::Success {
                    truncated: same
                } if same == truncated
            )
        });
        if conflict {
            return Decision {
                admitted: None,
                class_if_absent: "rejected",
            };
        }
        let first = &pairs[successes[0]];
        let Some(query_id) = first.query_id.clone() else {
            return Decision {
                admitted: None,
                class_if_absent: "incomplete",
            };
        };
        return Decision {
            admitted: Some(Admitted {
                query_id,
                receipt_ordinal: first.receipt_ordinal,
                pair_ordinal: first.pair_ordinal,
                status: "succeeded",
                truncated,
            }),
            class_if_absent: "admitted",
        };
    }
    let mut saw_in_scope = false;
    let mut saw_rejected = false;
    for index in indexes {
        if pairs[*index].in_scope {
            saw_in_scope = true;
            if matches!(pairs[*index].kind, PairKind::Rejected) {
                saw_rejected = true;
            }
        }
    }
    if !saw_in_scope {
        return Decision {
            admitted: None,
            class_if_absent: "crossdeclaredscope",
        };
    }
    Decision {
        admitted: None,
        class_if_absent: if saw_rejected {
            "rejected"
        } else {
            "incomplete"
        },
    }
}

