//! Curated Git apply.
//!
//! An approved description review and an expected Git head become one commit,
//! or no commit. The reviewer label is not an authenticated identity.

#![forbid(unsafe_code)]

mod canonical;
mod error;
mod git;
mod identity;
mod json;
mod layout;
mod plan;
mod receipt;
mod request;

use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;

use oo_core::oos::DatasetFile;

pub use error::CuratedGitError;
pub use identity::{SERVICE_DATE, SERVICE_EMAIL, SERVICE_NAME};
pub use request::APPROVED_REF;

use error::PROTOCOL;
use git::GitSession;
use layout::{check_checkout, strict_directory};
use plan::{assert_delta, dataset_path, encode_dataset, load_before, review_plan};
use receipt::{BlobRow, CommitIds, commit_message, marker_conflict, render};
use request::parse_request;

/// Trusted operator inputs. The request JSON cannot choose a different root or Git binary.
pub struct OperatorRegistry {
    pub project_root: PathBuf,
    pub git_binary: PathBuf,
}

/// Apply one curated Git request. `deadline` is the operator clock, not a request field.
pub fn apply_curated_git(
    registry: &OperatorRegistry,
    input: &str,
    deadline: Instant,
) -> Result<String, CuratedGitError> {
    let request = parse_request(input)?;
    let registered = strict_directory(&registry.project_root)?;
    let asked = strict_directory(&request.project_root)?;
    if registered != asked {
        return Err(CuratedGitError::new(
            "root.unregistered",
            "the project root is not the registered checkout",
        ));
    }
    check_checkout(&registered)?;
    let reviewed = review_plan(&request)?;
    let command: Value = serde_json::from_str(&request.review_command).map_err(|_| {
        CuratedGitError::new(
            "review.refused",
            "the description review compiler refused the command",
        )
    })?;
    let curated = command
        .get("curated")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CuratedGitError::new(
                "review.refused",
                "the description review compiler refused the command",
            )
        })?;
    let git = GitSession::open(&registry.git_binary, &registered, deadline)?;
    git.prepare()?;
    git.require_commit(&request.expected_head)?;
    git.reject_embedded(&request.expected_head)?;
    let mut writes: Vec<WriteFile> = Vec::new();
    for binding in &request.bindings {
        let path = dataset_path(&binding.dataset)?;
        let entry = git.entry(&request.expected_head, &path)?;
        let Some(entry) = entry else {
            return Err(CuratedGitError::new(
                "blob.missing",
                "the curated dataset blob is not in the tree",
            ));
        };
        if entry.mode != "100644" {
            return Err(CuratedGitError::new(
                "blob.mode",
                "the curated path is not a regular file",
            ));
        }
        if entry.object != binding.expected_blob {
            return Err(CuratedGitError::new(
                "blob.cas",
                "the curated blob does not match the expected blob",
            ));
        }
        let bytes = git.blob(&entry.object)?;
        let curated_value = curated.get(&binding.dataset).ok_or_else(|| {
            CuratedGitError::new(
                "binding.unknown",
                "a bound dataset is not in the review command",
            )
        })?;
        let before = load_before(&bytes, binding, curated_value)?;
        if let Some(planned) = reviewed
            .files
            .iter()
            .find(|file| file.dataset == binding.dataset)
        {
            assert_delta(&before, &planned.document, &binding.columns)?;
            let after: DatasetFile =
                serde_json::from_value(planned.document.clone()).map_err(|_| {
                    CuratedGitError::new(
                        "field.unapproved",
                        "a change is outside compiler-approved descriptive fields",
                    )
                })?;
            if after != before {
                writes.push(WriteFile {
                    dataset: binding.dataset.clone(),
                    path,
                    old_blob: entry.object,
                    bytes: encode_dataset(&after)?,
                });
            }
        }
    }
    let dirty = git.worktree_dirty(&request.expected_head)?;
    let old_tree = git.tree_of(&request.expected_head)?;
    if writes.is_empty() {
        let tip = git.tip()?;
        if tip != request.expected_head {
            return Err(CuratedGitError::new(
                "ref.cas",
                "the ref does not match the expected commit",
            ));
        }
        return Ok(render(
            &request,
            &reviewed,
            &CommitIds {
                old_commit: &request.expected_head,
                new_commit: None,
                old_tree: &old_tree,
                new_tree: &old_tree,
                dirty,
                blobs: &[],
            },
            false,
        ));
    }
    writes.sort_by(|left, right| left.path.cmp(&right.path));
    let payload: Vec<(String, Vec<u8>)> = writes
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect();
    let message = commit_message(&request, &reviewed);
    let (new_tree, new_commit, new_blobs) =
        git.build(&request.expected_head, &payload, &message)?;
    if new_blobs.len() != writes.len() {
        return Err(CuratedGitError::new(
            "git.exit",
            "git returned a non-zero status",
        ));
    }
    let mut rows = Vec::with_capacity(writes.len());
    for (file, blob) in writes.iter().zip(&new_blobs) {
        rows.push(BlobRow {
            dataset: file.dataset.clone(),
            path: file.path.clone(),
            old_blob: file.old_blob.clone(),
            new_blob: blob.clone(),
        });
    }
    let receipt = render(
        &request,
        &reviewed,
        &CommitIds {
            old_commit: &request.expected_head,
            new_commit: Some(&new_commit),
            old_tree: &old_tree,
            new_tree: &new_tree,
            dirty,
            blobs: &rows,
        },
        true,
    );
    publish(&git, &request, &new_commit, receipt)
}

fn publish(
    git: &GitSession,
    request: &request::Request,
    new_commit: &str,
    receipt: String,
) -> Result<String, CuratedGitError> {
    let tip = git.tip()?;
    if tip == new_commit || git.first_parents(&tip)?.iter().any(|sha| sha == new_commit) {
        return Ok(receipt);
    }
    if tip == request.expected_head {
        match git.cas(new_commit, &request.expected_head) {
            Ok(()) => return Ok(receipt),
            Err(error) if error.code == "ref.cas" => {
                let tip = git.tip()?;
                if tip == new_commit {
                    return Ok(receipt);
                }
                return conflicting(git, request, &tip, error);
            }
            Err(error) => return Err(error),
        }
    }
    conflicting(
        git,
        request,
        &tip,
        CuratedGitError::new("ref.cas", "the ref does not match the expected commit"),
    )
}

fn conflicting(
    git: &GitSession,
    request: &request::Request,
    tip: &str,
    fallback: CuratedGitError,
) -> Result<String, CuratedGitError> {
    let message = git.message(tip)?;
    if marker_conflict(&message, request) {
        Err(CuratedGitError::new(
            "idempotency.conflict",
            "the idempotency key was used for a different request",
        ))
    } else {
        Err(fallback)
    }
}

struct WriteFile {
    dataset: String,
    path: String,
    old_blob: String,
    bytes: Vec<u8>,
}

/// Resolve `git` from `PATH` without using a shell.
pub fn find_git(path: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path) {
        let candidate = dir.join("git");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The CLI deadline. Requests cannot raise it.
pub fn cli_deadline() -> Instant {
    Instant::now() + std::time::Duration::from_secs(30)
}

pub fn protocol() -> &'static str {
    PROTOCOL
}
