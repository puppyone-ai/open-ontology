//! Read a sealed spool and decide an offset watermark using the fixed crates.
//! This module does not open a database and does not ACK.

use std::path::Path;

use cdc_offset_store::{
    checkpoint, receipt_hash, Checkpoint, CheckpointRef, DeclaredNext, DeclaredTxn, OrderBinding,
    Receipt, Watermark,
};
use cdc_spool::{recover, Recovery};

use crate::model::{OffsetInput, OffsetReceipt, OffsetReport, SpoolReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolFact {
    pub report: SpoolReport,
    pub commit_position: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetFact {
    pub report: OffsetReport,
    pub applied_pos: Option<String>,
    pub receipts: Vec<Receipt>,
    pub selected_generation: Option<String>,
    pub required_generations: Vec<String>,
}

pub fn read_spool(dir: Option<&str>) -> SpoolFact {
    let Some(dir) = dir else {
        return SpoolFact {
            report: empty_spool(),
            commit_position: None,
        };
    };
    match recover(Path::new(dir)) {
        Ok(Recovery::Committed(committed)) => {
            let manifest = &committed.manifest;
            let attestation = manifest.ack_allowed
                || manifest.target_applied
                || manifest.cross_table_consistent
                || !manifest.applied.is_null()
                || manifest.upstream_commit_lsn_verified
                || manifest.debezium_normalizer
                || manifest.p2_verified
                || manifest.p11_verified
                || committed.ack_allowed();
            if attestation {
                return SpoolFact {
                    report: SpoolReport {
                        present: true,
                        state: "fault",
                        txn_id: Some(manifest.txn_id.clone()),
                        commit_lsn: Some(manifest.commit_lsn.clone()),
                        commit_position: None,
                        ack_allowed: false,
                        target_applied: false,
                        cross_table_consistent: false,
                        applied: None,
                        authorizes_ack: false,
                        authorizes_publish: false,
                        fault: Some("spool_attestation".to_owned()),
                    },
                    commit_position: None,
                };
            }
            SpoolFact {
                report: SpoolReport {
                    present: true,
                    state: "committed",
                    txn_id: Some(manifest.txn_id.clone()),
                    commit_lsn: Some(manifest.commit_lsn.clone()),
                    commit_position: Some(manifest.commit_position.clone()),
                    ack_allowed: false,
                    target_applied: false,
                    cross_table_consistent: false,
                    applied: None,
                    authorizes_ack: false,
                    authorizes_publish: false,
                    fault: None,
                },
                commit_position: Some(manifest.commit_position.clone()),
            }
        }
        Ok(Recovery::RolledBack(rolled)) => SpoolFact {
            report: SpoolReport {
                present: true,
                state: "rolled_back",
                txn_id: rolled.txn_id,
                commit_lsn: None,
                commit_position: None,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
                authorizes_ack: false,
                authorizes_publish: false,
                fault: None,
            },
            commit_position: None,
        },
        Ok(Recovery::Incomplete(_)) => SpoolFact {
            report: SpoolReport {
                present: true,
                state: "incomplete",
                txn_id: None,
                commit_lsn: None,
                commit_position: None,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
                authorizes_ack: false,
                authorizes_publish: false,
                fault: None,
            },
            commit_position: None,
        },
        Ok(Recovery::Unknown { reason }) => SpoolFact {
            report: SpoolReport {
                present: true,
                state: "unknown",
                txn_id: None,
                commit_lsn: None,
                commit_position: None,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
                authorizes_ack: false,
                authorizes_publish: false,
                fault: Some((*reason).to_owned()),
            },
            commit_position: None,
        },
        Err(fault) => SpoolFact {
            report: SpoolReport {
                present: true,
                state: "fault",
                txn_id: None,
                commit_lsn: None,
                commit_position: None,
                ack_allowed: false,
                target_applied: false,
                cross_table_consistent: false,
                applied: None,
                authorizes_ack: false,
                authorizes_publish: false,
                fault: Some(fault.code.to_string()),
            },
            commit_position: None,
        },
    }
}

pub fn read_offset(input: Option<&OffsetInput>) -> OffsetFact {
    let Some(input) = input else {
        return OffsetFact {
            report: empty_offset(),
            applied_pos: None,
            receipts: Vec::new(),
            selected_generation: None,
            required_generations: Vec::new(),
        };
    };
    let receipts: Vec<Receipt> = input.receipts.iter().map(to_receipt).collect();
    let required_generations = input
        .binding
        .declared_transaction
        .required_table_generations
        .clone();
    let Some(selected_index) = input
        .receipts
        .iter()
        .position(|receipt| receipt.table_generation == input.selected_generation)
    else {
        return OffsetFact {
            report: OffsetReport {
                present: true,
                receipt_closed: false,
                receipt_hash: None,
                checkpoint: "refused",
                watermark_hold: None,
                checkpoint_advanced: false,
                source_applied: false,
                fence_proof: false,
                ack_allowed: false,
                durable_control_mirror: false,
                live_mirror: "not_run",
                fault: Some("selected_receipt_absent"),
            },
            applied_pos: None,
            receipts,
            selected_generation: Some(input.selected_generation.clone()),
            required_generations,
        };
    };
    let selected = to_receipt(&input.receipts[selected_index]);
    let siblings: Vec<Receipt> = input
        .receipts
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != selected_index)
        .map(|(_, receipt)| to_receipt(receipt))
        .collect();
    let hash = receipt_hash(&selected);
    let watermark = input.watermark.as_ref().map(|mark| Watermark {
        position_codec: mark.position_codec.clone(),
        position_lineage: mark.position_lineage.clone(),
        source_epoch: mark.source_epoch,
        applied_pos: mark.applied_pos.clone(),
        applied_txn_id: mark.applied_txn_id.clone(),
        declared_next_txn: mark.declared_next_txn.clone(),
        declared_next_pos: mark.declared_next_pos.clone(),
    });
    let binding = to_binding(input);
    // `receipt_closed` is public in rules.rs and is not re-exported.
    // `checkpoint` returns Err("receipt_rule") before any other refusal
    // when the selected receipt fails that check, and Err("input") only
    // after the check has passed. A different-generation sibling that
    // fails the same check is also Err("receipt_rule"); that case is
    // reported closed=false because the crate gives no separate signal.
    let (checkpoint_name, hold, fault, advanced, closed) =
        match checkpoint(&selected, &siblings, watermark.as_ref(), &binding) {
            Ok(Checkpoint::InsertFirst) => ("insert_first", None, None, true, true),
            Ok(Checkpoint::CompareAndSwap) => ("compare_and_swap", None, None, true, true),
            Ok(Checkpoint::Skip(reason)) => ("skip", Some(reason), None, false, true),
            Err("input") => ("refused", None, Some("input"), false, true),
            Err(code) => ("refused", None, Some(code), false, false),
        };
    OffsetFact {
        report: OffsetReport {
            present: true,
            receipt_closed: closed,
            receipt_hash: hash,
            checkpoint: checkpoint_name,
            watermark_hold: hold,
            checkpoint_advanced: advanced,
            source_applied: false,
            fence_proof: false,
            ack_allowed: false,
            durable_control_mirror: false,
            live_mirror: "not_run",
            fault,
        },
        applied_pos: Some(selected.applied_pos),
        receipts,
        selected_generation: Some(input.selected_generation.clone()),
        required_generations,
    }
}

fn to_receipt(input: &OffsetReceipt) -> Receipt {
    Receipt {
        source: input.source.clone(),
        table_generation: input.table_generation.clone(),
        txn_or_batch_id: input.txn_or_batch_id.clone(),
        config_version: input.config_version.clone(),
        config_digest: input.config_digest.clone(),
        type_policy_version: input.type_policy_version.clone(),
        type_digest: input.type_digest.clone(),
        hook_digest: input.hook_digest.clone(),
        schema_digest: input.schema_digest.clone(),
        payload_digest: input.payload_digest.clone(),
        protocol_version: input.protocol_version.clone(),
        position_codec: input.position_codec.clone(),
        position_lineage: input.position_lineage.clone(),
        source_epoch: input.source_epoch,
        applied_pos: input.applied_pos.clone(),
        partial_txn: input.partial_txn,
        cross_table_consistent: input.cross_table_consistent,
        txn_complete: input.txn_complete,
        segment_index: input.segment_index,
        segment_count: input.segment_count,
        segment_bitmap: input.segment_bitmap.clone(),
        fence_shard: input.fence_shard.clone(),
        fence_generation: input.fence_generation,
        target_commit_at: input.target_commit_at.clone(),
    }
}

fn to_binding(input: &OffsetInput) -> OrderBinding {
    let binding = &input.binding;
    OrderBinding {
        provenance: binding.provenance.clone(),
        expected_previous: binding.expected_previous.as_ref().map(|item| CheckpointRef {
            position_codec: item.position_codec.clone(),
            position_lineage: item.position_lineage.clone(),
            source_epoch: item.source_epoch,
            applied_pos: item.applied_pos.clone(),
            txn_or_batch_id: item.txn_or_batch_id.clone(),
        }),
        declared_transaction: DeclaredTxn {
            txn_or_batch_id: binding.declared_transaction.txn_or_batch_id.clone(),
            applied_pos: binding.declared_transaction.applied_pos.clone(),
            required_table_generations: binding
                .declared_transaction
                .required_table_generations
                .clone(),
        },
        declared_next: DeclaredNext {
            txn_or_batch_id: binding.declared_next.txn_or_batch_id.clone(),
            applied_pos: binding.declared_next.applied_pos.clone(),
        },
        source_gap: binding.source_gap,
        epoch_barrier: binding.epoch_barrier,
    }
}

fn empty_spool() -> SpoolReport {
    SpoolReport {
        present: false,
        state: "absent",
        txn_id: None,
        commit_lsn: None,
        commit_position: None,
        ack_allowed: false,
        target_applied: false,
        cross_table_consistent: false,
        applied: None,
        authorizes_ack: false,
        authorizes_publish: false,
        fault: None,
    }
}

fn empty_offset() -> OffsetReport {
    OffsetReport {
        present: false,
        receipt_closed: false,
        receipt_hash: None,
        checkpoint: "absent",
        watermark_hold: None,
        checkpoint_advanced: false,
        source_applied: false,
        fence_proof: false,
        ack_allowed: false,
        durable_control_mirror: false,
        live_mirror: "not_run",
        fault: None,
    }
}
