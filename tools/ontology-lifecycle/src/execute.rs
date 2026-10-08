//! Call a ready producer, or record why this run did not call one.
//!
//! `chain_closed` stays false. Failure loops are not observed here, and a
//! journal file cannot promote a blocked contract into a finished chain.

use std::path::{Path, PathBuf};

use crate::assess::{assess_root, Assessment, Gap, SOURCE_HEAD};
use crate::client::{drive, SavedEdge, Session};
use crate::discover::Operation;
use crate::journal::{self, EdgeLine, Journal};

#[derive(Default)]
pub struct Plan {
    pub session: Option<Session>,
    pub identity: Option<String>,
    pub endpoints: Vec<(String, String)>,
    pub state: Option<PathBuf>,
}

pub struct EdgeProof {
    pub id: &'static str,
    pub response_sha256: &'static str,
    pub prior_sha256: &'static str,
}

#[derive(Clone, Copy)]
pub struct LoopProof {
    pub id: &'static str,
    pub side_effects: bool,
}

pub fn chain_proven(edges: &[EdgeProof], loops: &[LoopProof]) -> bool {
    let order = ["OC-01", "OC-02", "OC-03", "OC-04", "OC-05"];
    let mut previous = "";
    for id in order {
        let Some(edge) = edges.iter().find(|edge| edge.id == id) else {
            return false;
        };
        if !is_sha256(edge.response_sha256) {
            return false;
        }
        if id != "OC-01" && edge.prior_sha256 != previous {
            return false;
        }
        previous = edge.response_sha256;
    }
    ["model_failure", "cas_conflict", "restart", "stale_override", "revoke"]
        .iter()
        .all(|id| loops.iter().any(|item| item.id == *id && !item.side_effects))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn execute(root: &Path, plan: &Plan) -> Result<Assessment, &'static str> {
    let mut assessment = assess_root(root)?;
    let previous = match &plan.state {
        Some(state) => {
            state_names(state)?;
            if let Some(open) = journal::inflight(state)?
                && let Some(gap) = assessment.gaps.iter_mut().find(|gap| gap.id == open)
            {
                gap.status = "unknown_side_effect".to_owned();
                gap.detail = "inflight".to_owned();
            }
            let loaded = journal::load(state)?;
            if loaded.as_ref().is_some_and(|journal| journal.source_head != SOURCE_HEAD) {
                assessment.execution = "stale_journal";
                assessment.chain_closed = false;
                assessment.producer_calls = 0;
                return Ok(assessment);
            }
            loaded
        }
        None => None,
    };
    assessment.producer_calls = drive_ready(&mut assessment.gaps, plan, previous.as_ref())?;
    assessment.chain_closed = false;
    assessment.execution = execution_of(&assessment.gaps);
    if let Some(state) = &plan.state {
        let lines: Vec<EdgeLine> = assessment
            .gaps
            .iter()
            .map(|gap| EdgeLine {
                id: gap.id.clone(),
                status: gap.status.clone(),
                operation: if gap.operation.is_empty() { "-".to_owned() } else { gap.operation.clone() },
                request_sha256: gap.request_sha256.clone(),
                response_sha256: gap.response_sha256.clone(),
            })
            .collect();
        journal::store(state, SOURCE_HEAD, &lines)?;
        journal::cleanup(state)?;
    }
    Ok(assessment)
}

fn execution_of(gaps: &[Gap]) -> &'static str {
    if gaps.iter().any(|gap| gap.status == "unknown_side_effect") {
        "unknown_side_effect"
    } else if gaps.iter().any(|gap| gap.status == "executed" || gap.status == "recovered") {
        "partial"
    } else {
        "blocked_contract"
    }
}

fn drive_ready(gaps: &mut [Gap], plan: &Plan, previous: Option<&Journal>) -> Result<u32, &'static str> {
    let mut calls = 0;
    for gap in gaps.iter_mut() {
        if gap.status != "ready_uncalled" {
            continue;
        }
        let Some(session) = &plan.session else {
            gap.status = "unauthenticated".to_owned();
            gap.detail = "session_missing".to_owned();
            continue;
        };
        let Some(identity) = &plan.identity else {
            gap.status = "unauthenticated".to_owned();
            gap.detail = "identity_missing".to_owned();
            continue;
        };
        let Some(endpoint) = plan.endpoints.iter().find(|(id, _)| id == &gap.id).map(|(_, url)| url.as_str()) else {
            gap.status = "endpoint_missing".to_owned();
            gap.detail = "endpoint_missing".to_owned();
            continue;
        };
        let Some(state) = &plan.state else {
            gap.status = "state_required".to_owned();
            gap.detail = "state_required".to_owned();
            continue;
        };
        let saved = previous.and_then(|journal| {
            journal.edges.iter().find(|edge| {
                edge.id == gap.id
                    && edge.operation == gap.operation
                    && (edge.status == "executed" || edge.status == "recovered")
                    && !edge.response_sha256.is_empty()
            })
        });
        let operation = Operation {
            id: gap.operation.clone(),
            method: gap.method.clone(),
            path: gap.route.clone(),
        };
        if saved.is_none() {
            journal::mark_inflight(state, &gap.id)?;
        }
        let saved_edge = saved.map(|edge| SavedEdge { response_sha256: edge.response_sha256.clone() });
        let driven = drive(&gap.id, &operation, session, endpoint, identity, saved_edge.as_ref());
        calls += driven.mutations + driven.reads;
        if driven.status == "unavailable" && driven.mutations > 0 {
            gap.status = "unknown_side_effect".to_owned();
            gap.detail = "response_missing".to_owned();
            gap.request_sha256 = driven.request_sha256;
            continue;
        }
        if saved.is_none() {
            journal::clear_inflight(state)?;
        }
        gap.status = driven.status.to_owned();
        gap.detail = driven.detail.to_owned();
        gap.request_sha256 = driven.request_sha256;
        gap.response_sha256 = driven.response_sha256;
    }
    Ok(calls)
}

fn state_names(state: &Path) -> Result<(), &'static str> {
    let meta = std::fs::symlink_metadata(state).map_err(|_| "state")?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("state");
    }
    for entry in std::fs::read_dir(state).map_err(|_| "state")? {
        let entry = entry.map_err(|_| "state")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("state");
        };
        if name != "journal.txt" && name != "owned.txt" && name != "inflight" {
            return Err("state");
        }
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|_| "state")?;
        if meta.file_type().is_symlink() {
            return Err("state");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{chain_proven, EdgeProof, LoopProof};

    fn edge(id: &'static str, response: &'static str, prior: &'static str) -> EdgeProof {
        EdgeProof { id, response_sha256: response, prior_sha256: prior }
    }

    #[test]
    fn the_proof_rule_needs_linked_hashes_and_quiet_loops() {
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let next = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let third = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";
        let fourth = "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d";
        let fifth = "2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6";
        let edges = [
            edge("OC-01", hash, ""),
            edge("OC-02", next, hash),
            edge("OC-03", third, next),
            edge("OC-04", fourth, third),
            edge("OC-05", fifth, fourth),
        ];
        let loops = [
            LoopProof { id: "model_failure", side_effects: false },
            LoopProof { id: "cas_conflict", side_effects: false },
            LoopProof { id: "restart", side_effects: false },
            LoopProof { id: "stale_override", side_effects: false },
            LoopProof { id: "revoke", side_effects: false },
        ];
        assert!(chain_proven(&edges, &loops));
        assert!(!chain_proven(&edges, &loops[..4]));
        let mut broken = loops;
        broken[0].side_effects = true;
        assert!(!chain_proven(&edges, &broken));
        assert!(!chain_proven(&[], &loops));
    }
}
