//! Pure parts of the CAS: key validation, object paths, GC and LRU decisions.
//! No I/O and no panics, so the KP8 proof (V-S2-03) can reach them.

use std::time::Duration;

/// Kinds of hash-addressed objects (10 §3.3). All are addressed by the
/// BLAKE3 of the stored bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CasKind {
    /// `orig/b3/<aa>/<bb>/<b3>`: cached original (remote backends only).
    Orig,
    /// `ir/<aa>/<ir_b3>.json`: canonical `oo.doc-ir/v1`.
    Ir,
    /// `llm/<aa>/<response_b3>.json`: raw LLM response.
    Llm,
    /// `packs/<aa>/<b3>`: one file of a compiled Context Pack. Not in
    /// [`CasKind::ALL`]: packs are immutable and referenced by
    /// `context_pack_file`, so the GC sweep leaves them alone until a pack GC
    /// exists (share card).
    Pack,
}

impl CasKind {
    pub const ALL: [CasKind; 3] = [CasKind::Orig, CasKind::Ir, CasKind::Llm];

    /// Top-level directory of the kind.
    pub fn dir(self) -> &'static str {
        match self {
            CasKind::Orig => "orig",
            CasKind::Ir => "ir",
            CasKind::Llm => "llm",
            CasKind::Pack => "packs",
        }
    }
}

/// `true` for exactly 64 lowercase hex characters.
pub fn is_b3_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Path of an object relative to the CAS root, `None` for a malformed key.
///
/// The mapping is injective (the 64 hex characters appear verbatim in the
/// file name, shards are prefixes of them) and only produces characters from
/// `[0-9a-f/._]` plus the fixed directory names.
///
/// Formal: KP8
pub fn object_rel_path(kind: CasKind, b3: &str) -> Option<String> {
    if !is_b3_hex(b3) {
        return None;
    }
    let (aa, rest) = b3.split_at(2);
    let bb = &rest[..2];
    Some(match kind {
        CasKind::Orig => format!("orig/b3/{aa}/{bb}/{b3}"),
        CasKind::Ir => format!("ir/{aa}/{b3}.json"),
        CasKind::Llm => format!("llm/{aa}/{b3}.json"),
        CasKind::Pack => format!("packs/{aa}/{b3}"),
    })
}

/// Inverse of the file-name part: the key stored in `file_name`, if valid for
/// `kind`.
pub fn key_from_file_name(kind: CasKind, file_name: &str) -> Option<String> {
    let hex = match kind {
        CasKind::Orig | CasKind::Pack => file_name,
        CasKind::Ir | CasKind::Llm => file_name.strip_suffix(".json")?,
    };
    is_b3_hex(hex).then(|| hex.to_owned())
}

/// GC sweep decision: delete only when unreferenced AND at least `min_age`
/// old (10 §3.3: unmarked and mtime older than one hour).
pub fn gc_should_delete(referenced: bool, age: Duration, min_age: Duration) -> bool {
    !referenced && age >= min_age
}

/// One cached original, for LRU ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LruItem {
    pub key: String,
    pub size: u64,
    /// Last use in nanoseconds since the epoch (mtime, refreshed on read).
    pub last_used_ns: i128,
}

/// Keys to evict, least recently used first, until the remaining bytes fit
/// `budget`. `protected` keys are never evicted. Ties break on the key so the
/// result is deterministic.
pub fn lru_victims(
    items: &[LruItem],
    budget: u64,
    protected: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut total: u64 = items.iter().fold(0u64, |a, i| a.saturating_add(i.size));
    let mut order: Vec<&LruItem> = items.iter().collect();
    order.sort_by(|a, b| (a.last_used_ns, &a.key).cmp(&(b.last_used_ns, &b.key)));
    let mut out = Vec::new();
    for item in order {
        if total <= budget {
            break;
        }
        if protected(&item.key) {
            continue;
        }
        total = total.saturating_sub(item.size);
        out.push(item.key.clone());
    }
    out
}
