//! Fixed-schema report `oo.eval.docs-report/v1` (JSON and Markdown). Deterministic: no clocks, no paths.

use super::json::quote;
use super::labels::{CORE_TYPES, PREDICATES};
use super::match_::{
    BOOTSTRAP_RESAMPLES, BOOTSTRAP_SEED, Kind, Metric, Outcome, SupersedeScore, metric,
};

pub const PROTOCOL: &str = "oo.eval.docs-report/v1";
/// Formats of the golden set; items whose anchor format is not listed only count in the overall metrics.
pub const FORMATS: [&str; 7] = ["md", "txt", "csv", "xlsx", "docx", "pptx", "pdf"];

pub struct Counts {
    pub entities: usize,
    pub facts: usize,
    pub supersedes: usize,
    pub conflicts: usize,
}

pub struct Coverage {
    pub covered: usize,
    pub predicted: usize,
}

pub struct Redteam {
    pub canary_hits: usize,
    pub forbidden_name_hits: usize,
    pub canaries: usize,
    pub forbidden_names: usize,
    pub files_scanned: usize,
}

pub struct Report {
    pub golden: Counts,
    pub predictions: Counts,
    pub entities: Vec<Outcome>,
    pub facts: Vec<Outcome>,
    pub entity_cov: Coverage,
    /// Type precision inputs: `(identity matched, type matches)` (30 section 4).
    pub type_check: (usize, usize),
    pub fact_cov: Coverage,
    pub supersede: SupersedeScore,
    pub redteam: Redteam,
}

fn kinds<'a>(o: impl Iterator<Item = &'a Outcome>) -> Vec<Kind> {
    o.map(|x| x.kind).collect()
}

fn f4(x: f64) -> String {
    format!("{x:.4}")
}

fn metric_json(m: &Metric) -> String {
    format!(
        "{{\"tp\":{},\"fp\":{},\"fn\":{},\"precision\":{},\"recall\":{},\"f1\":{},\"f1_ci95\":[{},{}]}}",
        m.tp,
        m.fp,
        m.fn_,
        f4(m.precision),
        f4(m.recall),
        f4(m.f1),
        f4(m.f1_ci.0),
        f4(m.f1_ci.1)
    )
}

pub struct Computed {
    pub ent_overall: Metric,
    pub ent_by_type: Vec<(&'static str, Metric)>,
    pub fact_overall: Metric,
    pub fact_by_pred: Vec<(&'static str, Metric)>,
    pub by_format: Vec<(&'static str, Metric)>,
    pub sup: (f64, f64, f64),
    pub conflict_recall: f64,
}

pub fn compute(r: &Report) -> Computed {
    let by_type = CORE_TYPES
        .iter()
        .map(|t| {
            (
                *t,
                metric(&kinds(r.entities.iter().filter(|o| o.group == *t))),
            )
        })
        .collect();
    let by_pred = PREDICATES
        .iter()
        .map(|p| (*p, metric(&kinds(r.facts.iter().filter(|o| o.group == *p)))))
        .collect();
    let by_format = FORMATS
        .iter()
        .map(|f| {
            (
                *f,
                metric(&kinds(
                    r.entities
                        .iter()
                        .chain(r.facts.iter())
                        .filter(|o| o.format == *f),
                )),
            )
        })
        .collect();
    let s = &r.supersede;
    let fp = s.predicted.saturating_sub(s.correct);
    let fn_ = s.gold.saturating_sub(s.correct);
    Computed {
        ent_overall: metric(&kinds(r.entities.iter())),
        ent_by_type: by_type,
        fact_overall: metric(&kinds(r.facts.iter())),
        fact_by_pred: by_pred,
        by_format,
        sup: super::match_::prf(s.correct, fp, fn_),
        conflict_recall: if s.conflicts_gold == 0 {
            0.0
        } else {
            s.conflicts_detected as f64 / s.conflicts_gold as f64
        },
    }
}

fn obj(items: &[(&str, &Metric)]) -> String {
    format!(
        "{{{}}}",
        items
            .iter()
            .map(|(k, m)| format!("{}:{}", quote(k), metric_json(m)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn cov_json(c: &Coverage) -> String {
    let cov = if c.predicted == 0 {
        0.0
    } else {
        c.covered as f64 / c.predicted as f64
    };
    format!(
        "{{\"covered\":{},\"predicted\":{},\"coverage\":{}}}",
        c.covered,
        c.predicted,
        f4(cov)
    )
}

fn counts_json(c: &Counts) -> String {
    format!(
        "{{\"entities\":{},\"facts\":{},\"supersedes\":{},\"conflicts\":{}}}",
        c.entities, c.facts, c.supersedes, c.conflicts
    )
}

pub fn to_json(r: &Report) -> String {
    let c = compute(r);
    let t: Vec<(&str, &Metric)> = c.ent_by_type.iter().map(|(k, m)| (*k, m)).collect();
    let p: Vec<(&str, &Metric)> = c.fact_by_pred.iter().map(|(k, m)| (*k, m)).collect();
    let f: Vec<(&str, &Metric)> = c.by_format.iter().map(|(k, m)| (*k, m)).collect();
    let s = &r.supersede;
    let rt = &r.redteam;
    let seed = format!("{BOOTSTRAP_SEED:#x}");
    format!(
        concat!(
            "{{\"protocol\":{},\"golden\":{},\"predictions\":{},",
            "\"bootstrap\":{{\"method\":\"item resampling\",\"seed\":\"{}\",\"resamples\":{},\"confidence\":0.95}},",
            "\"entities\":{{\"overall\":{},\"by_type\":{}}},",
            "\"facts\":{{\"overall\":{},\"by_predicate\":{}}},",
            "\"by_format\":{},",
            "\"type_precision\":{{\"identity_matched\":{},\"type_correct\":{},\"precision\":{}}},\"provenance\":{{\"facts\":{},\"entities\":{}}},",
            "\"supersede\":{{\"gold\":{},\"predicted\":{},\"correct\":{},\"precision\":{},\"recall\":{},\"f1\":{}}},",
            "\"conflicts\":{{\"gold\":{},\"predicted\":{},\"detected\":{},\"recall\":{}}},",
            "\"redteam\":{{\"canary_hits\":{},\"forbidden_name_hits\":{},\"total_hits\":{},\"canaries\":{},\"forbidden_names\":{},\"files_scanned\":{}}}}}\n"
        ),
        quote(PROTOCOL),
        counts_json(&r.golden),
        counts_json(&r.predictions),
        seed,
        BOOTSTRAP_RESAMPLES,
        metric_json(&c.ent_overall),
        obj(&t),
        metric_json(&c.fact_overall),
        obj(&p),
        obj(&f),
        r.type_check.0,
        r.type_check.1,
        f4(if r.type_check.0 == 0 {
            0.0
        } else {
            r.type_check.1 as f64 / r.type_check.0 as f64
        }),
        cov_json(&r.fact_cov),
        cov_json(&r.entity_cov),
        s.gold,
        s.predicted,
        s.correct,
        f4(c.sup.0),
        f4(c.sup.1),
        f4(c.sup.2),
        s.conflicts_gold,
        s.conflicts_predicted,
        s.conflicts_detected,
        f4(c.conflict_recall),
        rt.canary_hits,
        rt.forbidden_name_hits,
        rt.canary_hits + rt.forbidden_name_hits,
        rt.canaries,
        rt.forbidden_names,
        rt.files_scanned,
    )
}

fn row(name: &str, m: &Metric) -> String {
    format!(
        "| {} | {} | {} | {} | {} | {} | {} | [{}, {}] |\n",
        name,
        m.tp,
        m.fp,
        m.fn_,
        f4(m.precision),
        f4(m.recall),
        f4(m.f1),
        f4(m.f1_ci.0),
        f4(m.f1_ci.1)
    )
}

fn table(title: &str, overall: Option<&Metric>, rows: &[(&str, Metric)]) -> String {
    let mut o = format!(
        "## {title}\n\n| 分组 | TP | FP | FN | 精确率 | 召回率 | F1 | F1 95% CI |\n|---|---|---|---|---|---|---|---|\n"
    );
    if let Some(m) = overall {
        o.push_str(&row("**overall**", m));
    }
    for (k, m) in rows {
        o.push_str(&row(k, m));
    }
    o.push('\n');
    o
}

pub fn to_markdown(r: &Report) -> String {
    let c = compute(r);
    let rt = &r.redteam;
    let mut o = format!("# oo-eval docs report ({PROTOCOL})\n\n");
    o.push_str(&format!(
        "标注：{} 实体 / {} 事实 / {} 取代 / {} 冲突；预测：{} 实体 / {} 事实 / {} 取代。\n\n置信区间：按条目自助法，种子 {}，{} 次重采样，95%。\n\n",
        r.golden.entities, r.golden.facts, r.golden.supersedes, r.golden.conflicts, r.predictions.entities, r.predictions.facts, r.predictions.supersedes, BOOTSTRAP_SEED, BOOTSTRAP_RESAMPLES
    ));
    o.push_str(&table(
        "实体（按核心类型）",
        Some(&c.ent_overall),
        &c.ent_by_type,
    ));
    o.push_str(&table(
        "事实（按关系）",
        Some(&c.fact_overall),
        &c.fact_by_pred,
    ));
    o.push_str(&table("按格式（实体与事实合计）", None, &c.by_format));
    let pc = |x: &Coverage| {
        if x.predicted == 0 {
            0.0
        } else {
            x.covered as f64 / x.predicted as f64
        }
    };
    o.push_str(&format!(
        "## 类型精确率\n\n- 身份对上的预测实体中类型也对：{}/{} = {}\n\n## 出处覆盖率\n\n- 事实：{}/{} = {}\n- 实体：{}/{} = {}\n\n",
        r.type_check.1,
        r.type_check.0,
        f4(if r.type_check.0 == 0 { 0.0 } else { r.type_check.1 as f64 / r.type_check.0 as f64 }),
        r.fact_cov.covered,
        r.fact_cov.predicted,
        f4(pc(&r.fact_cov)),
        r.entity_cov.covered,
        r.entity_cov.predicted,
        f4(pc(&r.entity_cov))
    ));
    o.push_str(&format!(
        "## 取代与冲突\n\n- 取代：标注 {}，预测 {}，正确 {}，精确率 {}，召回率 {}，F1 {}\n- 冲突（D57）：标注 {}，预测 {}，命中 {}，召回率 {}\n\n",
        r.supersede.gold, r.supersede.predicted, r.supersede.correct, f4(c.sup.0), f4(c.sup.1), f4(c.sup.2),
        r.supersede.conflicts_gold, r.supersede.conflicts_predicted, r.supersede.conflicts_detected, f4(c.conflict_recall)
    ));
    o.push_str(&format!(
        "## 红队\n\n- 金丝雀命中：{}（{} 个金丝雀）\n- 红队独有名称命中：{}（{} 个名称与别名）\n- 扫描文件数：{}\n",
        rt.canary_hits, rt.canaries, rt.forbidden_name_hits, rt.forbidden_names, rt.files_scanned
    ));
    o
}
