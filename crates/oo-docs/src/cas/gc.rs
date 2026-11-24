//! Mark-sweep GC, `orig/` LRU eviction and usage (10 §3.3).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::layout::{CasKind, LruItem, gc_should_delete, key_from_file_name, lru_victims};
use super::{Cas, CasError};

/// Marked objects: every `(kind, b3)` referenced from the database
/// (`parse_result.ir_b3`, `llm_cache.response_b3`, `blob.cached`).
pub type RefSet = BTreeSet<(CasKind, String)>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub scanned: u64,
    pub deleted: u64,
    pub bytes_freed: u64,
    pub kept_referenced: u64,
    pub kept_young: u64,
    /// Files that are not valid CAS object names; left alone.
    pub foreign: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvictReport {
    /// Evicted original keys; the caller sets `blob.cached = false`.
    pub evicted: Vec<String>,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub orig_bytes: u64,
    pub ir_bytes: u64,
    pub llm_bytes: u64,
    pub objects: u64,
}

/// Result of [`Cas::sample_verify`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleReport {
    /// Objects hashed.
    pub checked: u32,
    /// Objects whose content does not hash to their name.
    pub corrupt: u32,
}

struct Found {
    key: String,
    path: std::path::PathBuf,
    size: u64,
    mtime: SystemTime,
}

fn walk(dir: &Path, kind: CasKind, out: &mut Vec<Found>, foreign: &mut u64) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(md) = fs::symlink_metadata(&path) else {
            continue;
        };
        if md.is_dir() {
            walk(&path, kind, out, foreign);
        } else if md.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            match key_from_file_name(kind, &name) {
                Some(key) => out.push(Found {
                    key,
                    path,
                    size: md.len(),
                    mtime: md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                }),
                None => *foreign += 1,
            }
        } else {
            *foreign += 1;
        }
    }
}

fn kind_root(cas: &Cas, kind: CasKind) -> std::path::PathBuf {
    match kind {
        CasKind::Orig => cas.root.join("orig/b3"),
        k => cas.root.join(k.dir()),
    }
}

fn prune_empty(dir: &Path, stop: &Path) {
    let mut cur = dir.to_path_buf();
    while cur != stop && fs::remove_dir(&cur).is_ok() {
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
}

fn ns(t: SystemTime) -> i128 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

impl Cas {
    /// Sweep objects that are not in `refs` and are at least `min_age` old
    /// (default 1 hour in the `gc` job). `now` is injected for tests.
    pub fn gc(
        &self,
        refs: &RefSet,
        now: SystemTime,
        min_age: Duration,
    ) -> Result<GcReport, CasError> {
        let mut report = GcReport::default();
        for kind in CasKind::ALL {
            let mut found = Vec::new();
            walk(
                &kind_root(self, kind),
                kind,
                &mut found,
                &mut report.foreign,
            );
            for obj in found {
                report.scanned += 1;
                let referenced = refs.contains(&(kind, obj.key.clone()));
                let age = now.duration_since(obj.mtime).unwrap_or_default();
                if referenced {
                    report.kept_referenced += 1;
                } else if !gc_should_delete(referenced, age, min_age) {
                    report.kept_young += 1;
                } else if fs::remove_file(&obj.path).is_ok() {
                    report.deleted += 1;
                    report.bytes_freed += obj.size;
                    if let Some(parent) = obj.path.parent() {
                        prune_empty(parent, &kind_root(self, kind));
                    }
                }
            }
        }
        Ok(report)
    }

    /// Keep `orig/` within `budget` bytes by deleting least recently used
    /// originals (recency = mtime, refreshed on every verified read). Keys in
    /// `protected` (for example currently being parsed) are skipped.
    pub fn evict_orig(
        &self,
        budget: u64,
        protected: &BTreeSet<String>,
    ) -> Result<EvictReport, CasError> {
        let mut found = Vec::new();
        let mut foreign = 0;
        walk(
            &kind_root(self, CasKind::Orig),
            CasKind::Orig,
            &mut found,
            &mut foreign,
        );
        let items: Vec<LruItem> = found
            .iter()
            .map(|f| LruItem {
                key: f.key.clone(),
                size: f.size,
                last_used_ns: ns(f.mtime),
            })
            .collect();
        let before: u64 = items.iter().map(|i| i.size).sum();
        let victims = lru_victims(&items, budget, &|k| protected.contains(k));
        let mut report = EvictReport {
            evicted: Vec::new(),
            bytes_before: before,
            bytes_after: before,
        };
        for key in victims {
            if let Some(f) = found.iter().find(|f| f.key == key)
                && fs::remove_file(&f.path).is_ok()
            {
                report.bytes_after = report.bytes_after.saturating_sub(f.size);
                report.evicted.push(key);
                if let Some(parent) = f.path.parent() {
                    prune_empty(parent, &kind_root(self, CasKind::Orig));
                }
            }
        }
        Ok(report)
    }

    /// Hash up to `n` objects spread over the store and count those whose
    /// content is not what their name says (`oo doctor`, 10 §10.4). Read only:
    /// unlike [`Cas::verify`] it deletes nothing. `seed` picks where the
    /// stride starts, so successive samples cover different objects.
    pub fn sample_verify(&self, n: usize, seed: u64) -> SampleReport {
        let mut all: Vec<(CasKind, Found)> = Vec::new();
        for kind in CasKind::ALL {
            let mut found = Vec::new();
            let mut foreign = 0;
            walk(&kind_root(self, kind), kind, &mut found, &mut foreign);
            all.extend(found.into_iter().map(|f| (kind, f)));
        }
        all.sort_by(|a, b| a.1.key.cmp(&b.1.key));
        let mut report = SampleReport::default();
        if all.is_empty() || n == 0 {
            return report;
        }
        let take = n.min(all.len());
        let step = (all.len() / take).max(1);
        let start = (seed % all.len() as u64) as usize;
        for i in 0..take {
            let (_, f) = &all[(start + i * step) % all.len()];
            report.checked += 1;
            match super::file_b3(&f.path) {
                Ok(h) if h == f.key => {}
                _ => report.corrupt += 1,
            }
        }
        report
    }

    /// Disk usage for health and metrics.
    pub fn usage(&self) -> Usage {
        let mut u = Usage::default();
        for kind in CasKind::ALL {
            let mut found = Vec::new();
            let mut foreign = 0;
            walk(&kind_root(self, kind), kind, &mut found, &mut foreign);
            let bytes: u64 = found.iter().map(|f| f.size).sum();
            u.objects += found.len() as u64;
            match kind {
                CasKind::Orig => u.orig_bytes = bytes,
                CasKind::Ir => u.ir_bytes = bytes,
                CasKind::Llm => u.llm_bytes = bytes,
                CasKind::Pack => {}
            }
        }
        u
    }
}
