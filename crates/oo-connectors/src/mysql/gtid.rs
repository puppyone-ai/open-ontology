//! GTID sets as `@@GLOBAL.gtid_executed` prints them, including the tagged
//! form `UUID:TAG:N` that MySQL 8.3+ accepts (`uuid:1-5:tag:1-3`).
//!
//! The set is parsed only to validate a watermark and to check containment
//! (low ⊆ high, monotonic positions). The printed form is canonical: no
//! whitespace, UUIDs and tags lower case, intervals merged and sorted.

use std::collections::BTreeMap;
use std::fmt;

use crate::SourceError;

/// `(source uuid, tag)` to merged, sorted, inclusive intervals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GtidSet {
    sets: BTreeMap<(String, String), Vec<(u64, u64)>>,
}

fn uuid_ok(uuid: &str) -> bool {
    let groups: Vec<_> = uuid.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// MySQL tag: letter or underscore first, then letters, digits, underscores,
/// at most 32 characters.
fn tag_ok(tag: &str) -> bool {
    let mut bytes = tag.bytes();
    tag.len() <= 32
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn interval(text: &str) -> Option<(u64, u64)> {
    let (lo, hi) = match text.split_once('-') {
        Some((lo, hi)) => (lo.parse().ok()?, hi.parse().ok()?),
        None => {
            let n = text.parse().ok()?;
            (n, n)
        }
    };
    (lo >= 1 && lo <= hi).then_some((lo, hi))
}

fn merge(intervals: &mut Vec<(u64, u64)>) {
    intervals.sort_unstable();
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(intervals.len());
    for &(lo, hi) in intervals.iter() {
        match out.last_mut() {
            Some(last) if lo <= last.1.saturating_add(1) => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    *intervals = out;
}

impl GtidSet {
    pub fn parse(raw: &str) -> Result<Self, SourceError> {
        let text: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        let mut sets: BTreeMap<(String, String), Vec<(u64, u64)>> = BTreeMap::new();
        if text.is_empty() {
            return Ok(Self { sets });
        }
        for member in text.split(',') {
            let mut parts = member.split(':');
            let uuid = parts.next().unwrap_or("").to_ascii_lowercase();
            if !uuid_ok(&uuid) {
                return Err(SourceError::protocol("gtid_uuid"));
            }
            let mut tag = String::new();
            let mut any = false;
            let mut needs_interval = true;
            for part in parts {
                if let Some(iv) = interval(part) {
                    sets.entry((uuid.clone(), tag.clone()))
                        .or_default()
                        .push(iv);
                    any = true;
                    needs_interval = false;
                } else if tag_ok(part) {
                    if needs_interval && !tag.is_empty() {
                        return Err(SourceError::protocol("gtid_interval"));
                    }
                    tag = part.to_ascii_lowercase();
                    needs_interval = true;
                } else {
                    return Err(SourceError::protocol("gtid_element"));
                }
            }
            if !any || needs_interval {
                return Err(SourceError::protocol("gtid_interval"));
            }
        }
        for intervals in sets.values_mut() {
            merge(intervals);
        }
        Ok(Self { sets })
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// Whether any member carries a tag (MySQL 8.3+ only).
    pub fn has_tags(&self) -> bool {
        self.sets.keys().any(|(_, tag)| !tag.is_empty())
    }

    /// Whether every transaction in `other` is also in `self`.
    pub fn contains(&self, other: &GtidSet) -> bool {
        other.sets.iter().all(|(key, theirs)| {
            let Some(ours) = self.sets.get(key) else {
                return false;
            };
            theirs
                .iter()
                .all(|&(lo, hi)| ours.iter().any(|&(a, b)| a <= lo && hi <= b))
        })
    }

    /// Whether the single transaction `uuid[:tag]:n` is in the set.
    pub fn contains_one(&self, uuid: &str, tag: &str, n: u64) -> bool {
        self.sets
            .get(&(uuid.to_ascii_lowercase(), tag.to_ascii_lowercase()))
            .is_some_and(|ivs| ivs.iter().any(|&(a, b)| a <= n && n <= b))
    }

    /// Number of transactions in the set (saturating).
    pub fn len(&self) -> u64 {
        self.sets
            .values()
            .flatten()
            .fold(0u64, |n, (a, b)| n.saturating_add(b - a + 1))
    }
}

impl fmt::Display for GtidSet {
    /// Untagged intervals first, then each tag in order, as MySQL prints them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut uuid_prev: Option<&str> = None;
        for ((uuid, tag), intervals) in &self.sets {
            if uuid_prev == Some(uuid.as_str()) {
                if !tag.is_empty() {
                    write!(f, ":{tag}")?;
                }
            } else {
                if uuid_prev.is_some() {
                    f.write_str(",")?;
                }
                f.write_str(uuid)?;
                if !tag.is_empty() {
                    write!(f, ":{tag}")?;
                }
                uuid_prev = Some(uuid);
            }
            for (lo, hi) in intervals {
                if lo == hi {
                    write!(f, ":{lo}")?;
                } else {
                    write!(f, ":{lo}-{hi}")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "3e11fa47-71ca-11e1-9e33-c80aa9429562";
    const B: &str = "9f0e6c1a-0000-11ef-8000-0242ac110002";

    #[test]
    fn parses_untagged_sets_with_newlines() {
        let s = GtidSet::parse(&format!("{A}:1-5:7,\n{B}:1-3")).unwrap();
        assert_eq!(s.to_string(), format!("{A}:1-5:7,{B}:1-3"));
        assert_eq!(s.len(), 9);
        assert!(!s.has_tags());
        assert!(GtidSet::parse("").unwrap().is_empty());
    }

    #[test]
    fn parses_tagged_gtids_and_keeps_them_apart() {
        let s = GtidSet::parse(&format!("{A}:1-5:Batch_1:1-3:9,{B}:x:4")).unwrap();
        assert!(s.has_tags());
        assert_eq!(s.to_string(), format!("{A}:1-5:batch_1:1-3:9,{B}:x:4"));
        assert!(s.contains_one(A, "", 5));
        assert!(s.contains_one(A, "batch_1", 9));
        assert!(!s.contains_one(A, "", 9));
        assert!(!s.contains_one(B, "", 4));
    }

    #[test]
    fn merges_adjacent_intervals_and_checks_containment() {
        let low = GtidSet::parse(&format!("{A}:1-3:4-5")).unwrap();
        let high = GtidSet::parse(&format!("{A}:1-9,{B}:t:1")).unwrap();
        assert_eq!(low.to_string(), format!("{A}:1-5"));
        assert!(high.contains(&low));
        assert!(!low.contains(&high));
        assert!(high.contains(&GtidSet::default()));
    }

    #[test]
    fn refuses_malformed_members() {
        for bad in [
            "nope:1",
            &format!("{A}"),
            &format!("{A}:0"),
            &format!("{A}:5-3"),
            &format!("{A}:9tag:1"),
            &format!("{A}:1-x"),
            &format!("{A}:1:tag"),
            &format!("{A}:tag:other:1"),
            &format!("{A}:{}:1", "t".repeat(33)),
        ] {
            assert!(GtidSet::parse(bad).is_err(), "{bad}");
        }
    }
}
