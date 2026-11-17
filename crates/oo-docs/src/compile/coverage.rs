//! Freshness coverage and the five guarantees of a pack (00 §5.6.1, 10 §16.2).
//! Pure data: the store fills a [`Coverage`], this module words it.

use serde_json::{Value, json};

/// How far the pack's ontology version covers the Drive at creation time
/// (D54). `pending` and `failed` list in-scope files only (D26).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub drive_rev: i64,
    pub covered_rev: i64,
    pub in_scope: u64,
    pub processed: u64,
    /// `(path, state)`, sorted.
    pub pending: Vec<(String, String)>,
    /// `(path, state_code)`, sorted.
    pub failed: Vec<(String, String)>,
}

impl Coverage {
    /// Everything in scope is processed and the version covers `drive_rev`.
    pub fn complete(&self) -> bool {
        self.pending.is_empty() && self.failed.is_empty() && self.covered_rev >= self.drive_rev
    }

    pub fn to_json(&self) -> Value {
        json!({
            "drive_rev": self.drive_rev,
            "covered_rev": self.covered_rev,
            "in_scope": self.in_scope,
            "processed": self.processed,
            "pending": self.pending.iter().map(|(p, s)| json!({"path": p, "state": s})).collect::<Vec<_>>(),
            "failed": self.failed.iter().map(|(p, s)| json!({"path": p, "state_code": s})).collect::<Vec<_>>(),
        })
    }

    /// `覆盖：39/42 个文件已处理；未完成 3 个（x.pdf 解析中 …）`.
    pub fn brief_line(&self) -> String {
        let mut out = format!("覆盖：{}/{} 个文件已处理", self.processed, self.in_scope);
        let open = self.pending.len() + self.failed.len();
        if open > 0 {
            let shown: Vec<String> = self
                .pending
                .iter()
                .map(|(p, s)| format!("{p} {}", state_cn(s)))
                .chain(self.failed.iter().map(|(p, c)| format!("{p} 失败（{c}）")))
                .take(3)
                .collect();
            out.push_str(&format!("；未完成 {open} 个（{}", shown.join("、")));
            out.push_str(if open > 3 { " …）" } else { "）" });
        } else if self.covered_rev < self.drive_rev {
            out.push_str(&format!(
                "；本体版本只覆盖到变更 {} / {}",
                self.covered_rev, self.drive_rev
            ));
        }
        out
    }
}

fn state_cn(state: &str) -> &str {
    match state {
        "detected" | "stored" => "等待解析",
        "parsing" => "解析中",
        "parsed" | "normalized" => "已解析，待抽取",
        "extracting" => "抽取中",
        "extracted" | "indexed" => "待发布",
        other => other,
    }
}

/// The five promises (00 §5.6.1). `group_same_version` is a promise of the
/// share, true for a pack on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guarantees {
    pub pack_consistent: bool,
    pub freshness_floor: bool,
    pub change_delivery: bool,
    pub group_same_version: bool,
    pub constraint_free: bool,
}

impl Guarantees {
    pub fn compute(coverage: Option<&Coverage>, open_conflicts: bool) -> Self {
        Self {
            pack_consistent: true,
            freshness_floor: coverage.is_none_or(Coverage::complete),
            change_delivery: true,
            group_same_version: true,
            constraint_free: !open_conflicts,
        }
    }

    pub fn to_json(self) -> Value {
        json!({
            "pack_consistent": self.pack_consistent,
            "freshness_floor": self.freshness_floor,
            "change_delivery": self.change_delivery,
            "group_same_version": self.group_same_version,
            "constraint_free": self.constraint_free,
        })
    }

    /// One line for the BRIEF header.
    pub fn brief_line(self) -> String {
        let mark = |ok: bool| if ok { "是" } else { "否（见上文）" };
        format!(
            "一致性承诺：包内一致 {} · 新鲜度下界 {} · 变更必达 {} · 并行同源 {} · 本体无矛盾 {}",
            mark(self.pack_consistent),
            mark(self.freshness_floor),
            mark(self.change_delivery),
            mark(self.group_same_version),
            mark(self.constraint_free)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_partial_wording() {
        let mut c = Coverage {
            drive_rev: 5,
            covered_rev: 5,
            in_scope: 42,
            processed: 42,
            ..Coverage::default()
        };
        assert_eq!(c.brief_line(), "覆盖：42/42 个文件已处理");
        assert!(c.complete());
        c.processed = 39;
        c.pending = vec![("x.pdf".into(), "parsing".into())];
        c.failed = vec![("b.docx".into(), "DOCS_PARSE_CORRUPT".into())];
        assert_eq!(
            c.brief_line(),
            "覆盖：39/42 个文件已处理；未完成 2 个（x.pdf 解析中、b.docx 失败（DOCS_PARSE_CORRUPT））"
        );
        assert!(!c.complete());
        let g = Guarantees::compute(Some(&c), true);
        assert!(!g.freshness_floor && !g.constraint_free && g.pack_consistent);
        assert!(g.brief_line().contains("新鲜度下界 否"));
    }

    #[test]
    fn rev_gap_is_named() {
        let c = Coverage {
            drive_rev: 7,
            covered_rev: 4,
            in_scope: 3,
            processed: 3,
            ..Coverage::default()
        };
        assert!(c.brief_line().contains("4 / 7"));
        assert!(!c.complete());
    }
}
