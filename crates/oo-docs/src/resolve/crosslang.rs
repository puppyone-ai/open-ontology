//! One person, organisation or project named in two scripts (10 §5.4, step 6 widened). The exact resolver binds a name to
//! an entity only when the name is the entity's own or a published alias; a file that says `Wanqing Shao` and another that
//! says `邵婉清` therefore make two entities that merely list each other's name as an alias. This pass finds those pairs
//! after the fact, so the answer does not depend on which file came first.
//!
//! Evidence, per pair of active entities of the same type where one canonical name has Han characters and the other has none:
//! * `roster`: a rule alias (a table row that holds both names, a header such as `English name`) of one is the other's name;
//! * `mutual`: each lists the other's canonical name as an alias (two independent extractions);
//! * `claim`: one lists the other's name as an alias (a model alias, a one-way claim);
//! * `pinyin`: people only, the Latin name is the pinyin of the Chinese one ([`super::pinyin`]).
//!
//! Merge when `roster`, `mutual`, or `claim` together with `pinyin`. A `claim` alone or `pinyin` alone is only a review
//! candidate (`entity_merge`, reason `cross_language`). A name that has more than one candidate partner is never merged
//! (two people can share a pinyin): every pair of it becomes a review candidate. A contradicting identity (another
//! e-mail) refuses the merge and the pair goes to review as well. Provenance stays: the merge keeps the facts' and aliases'
//! originals ([`super::merge`]) and the redirect names reason `merge`.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;

use super::merge::{MergeError, merge_entities};
use super::pinyin::{is_han, matches_latin};
use crate::store::StoreError;

/// An alias of a candidate.
#[derive(Clone, Debug)]
pub struct Al {
    pub text: String,
    pub norm: String,
    /// `rule`, `llm`, `human` or `suggest`.
    pub origin: String,
    pub bp: u16,
}

/// An active entity the pass looks at.
#[derive(Clone, Debug)]
pub struct Cand {
    pub key: String,
    pub ty: String,
    pub name: String,
    pub norm: String,
    pub evidence: i64,
    pub aliases: Vec<Al>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Merge,
    Review,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub kind: Kind,
    /// Merged away (merge) or the first of the pair (review).
    pub from: String,
    pub into: String,
    /// `roster`, `mutual`, `claim+pinyin`, `claim`, `pinyin` or `ambiguous`.
    pub why: &'static str,
    pub shared: Vec<String>,
}

fn has_han(s: &str) -> bool {
    s.chars().any(is_han)
}

/// What `a` says about `b`: the strongest alias of `a` equal to `b`'s canonical name.
fn claim<'a>(a: &'a Cand, b: &Cand) -> Option<&'a Al> {
    a.aliases
        .iter()
        .filter(|al| al.norm == b.norm)
        .max_by_key(|al| (al.origin == "rule" && al.bp >= 9000, al.bp))
}

/// All decisions, in key order. Pure.
pub fn decide(cands: &[Cand]) -> Vec<Decision> {
    // (han side, latin side, why, shared)
    let mut pairs: Vec<(usize, usize, &'static str, Vec<String>)> = Vec::new();
    for (i, h) in cands.iter().enumerate() {
        if !has_han(&h.name) {
            continue;
        }
        for (j, l) in cands.iter().enumerate() {
            if i == j || h.ty != l.ty || has_han(&l.name) {
                continue;
            }
            let hl = claim(h, l);
            let lh = cands[j]
                .aliases
                .iter()
                .filter(|al| al.norm == h.norm)
                .max_by_key(|al| (al.origin == "rule" && al.bp >= 9000, al.bp));
            let pinyin = h.ty == "Person" && matches_latin(&h.name, &l.name);
            let roster = [hl, lh]
                .into_iter()
                .flatten()
                .any(|al| al.origin == "rule" && al.bp >= 9000);
            let mutual = hl.is_some() && lh.is_some();
            let why = if roster {
                "roster"
            } else if mutual {
                "mutual"
            } else if (hl.is_some() || lh.is_some()) && pinyin {
                "claim+pinyin"
            } else if hl.is_some() || lh.is_some() {
                "claim"
            } else if pinyin {
                "pinyin"
            } else {
                continue;
            };
            let shared = vec![h.name.clone(), l.name.clone()];
            pairs.push((i, j, why, shared));
        }
    }
    // A name with more than one partner is never merged.
    let mut partners: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (i, j, ..) in &pairs {
        partners.entry(*i).or_default().insert(*j);
        partners.entry(*j).or_default().insert(*i);
    }
    let mut out = Vec::new();
    for (i, j, why, shared) in pairs {
        let ambiguous = partners[&i].len() > 1 || partners[&j].len() > 1;
        let (kind, why) = if ambiguous {
            (Kind::Review, "ambiguous")
        } else if matches!(why, "roster" | "mutual" | "claim+pinyin") {
            (Kind::Merge, why)
        } else {
            (Kind::Review, why)
        };
        // Keep the entity that rests on more evidence; ties go to the smaller key.
        let (a, b) = (&cands[i], &cands[j]);
        let (into, from) =
            if (a.evidence, std::cmp::Reverse(&a.key)) >= (b.evidence, std::cmp::Reverse(&b.key)) {
                (a, b)
            } else {
                (b, a)
            };
        out.push(Decision {
            kind,
            from: from.key.clone(),
            into: into.key.clone(),
            why,
            shared,
        });
    }
    out.sort_by(|x, y| (&x.into, &x.from).cmp(&(&y.into, &y.from)));
    out
}

/// What a run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub merged: u32,
    pub proposed: u32,
}

async fn load(conn: &mut PgConnection) -> Result<Vec<Cand>, StoreError> {
    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT e.key, e.type, e.canonical_name, e.norm_name, \
                (SELECT count(*) FROM _oo_docs.evidence ev WHERE ev.target_kind = 'entity' \
                  AND ev.target_id = e.id AND ev.withdrawn_at IS NULL) \
           FROM _oo_docs.entity e \
          WHERE e.status = 'active' AND e.type IN ('Person', 'Organization', 'Project') \
          ORDER BY e.key",
    )
    .fetch_all(&mut *conn)
    .await?;
    let al: Vec<(String, String, String, String, i32)> = sqlx::query_as(
        "SELECT e.key, a.alias, a.norm_alias, a.origin, a.confidence_bp \
           FROM _oo_docs.alias a JOIN _oo_docs.entity e ON e.id = a.entity_id \
          WHERE e.status = 'active' AND a.status IN ('draft', 'published') \
            AND e.type IN ('Person', 'Organization', 'Project') ORDER BY e.key, a.norm_alias",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut by_key: BTreeMap<String, Vec<Al>> = BTreeMap::new();
    for (k, text, n, origin, bp) in al {
        by_key.entry(k).or_default().push(Al {
            text,
            norm: n,
            origin,
            bp: u16::try_from(bp).unwrap_or(0),
        });
    }
    Ok(rows
        .into_iter()
        .map(|(key, ty, name, n, evidence)| Cand {
            aliases: by_key.remove(&key).unwrap_or_default(),
            key,
            ty,
            name,
            norm: n,
            evidence,
        })
        .collect())
}

async fn propose(conn: &mut PgConnection, d: &Decision, reason: &str) -> Result<u32, StoreError> {
    let done = sqlx::query(
        "INSERT INTO _oo_docs.review_item \
           (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, confidence_bp, reason_code) \
         SELECT gen_random_uuid(), 'entity_merge', $1, f.drive_id, f.id, fv.id, \
                jsonb_build_object('entity_keys', jsonb_build_array($2::text, $3::text), 'shared_aliases', $4::jsonb, 'basis', $5::text), \
                NULL, $6 \
           FROM _oo_docs.entity e \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id \
          WHERE e.key = $2 \
          ORDER BY ev.created_at LIMIT 1 \
         ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(format!("entity_merge:{}:{}", d.into, d.from))
    .bind(&d.into)
    .bind(&d.from)
    .bind(serde_json::to_string(&d.shared).unwrap_or_else(|_| "[]".into()))
    .bind(d.why)
    .bind(reason)
    .execute(&mut *conn)
    .await?;
    Ok(u32::try_from(done.rows_affected()).unwrap_or(0))
}

/// Run the pass inside the caller's transaction: merge the sure pairs, queue the rest.
pub async fn run(conn: &mut PgConnection) -> Result<Summary, StoreError> {
    let cands = load(conn).await?;
    let mut sum = Summary::default();
    for d in decide(&cands) {
        match d.kind {
            Kind::Review => sum.proposed += propose(conn, &d, "cross_language").await?,
            Kind::Merge => match merge_entities(conn, &d.from, &d.into, "merge", None).await {
                Ok(_) => sum.merged += 1,
                Err(MergeError::IdentityConflict(..)) => {
                    sum.proposed += propose(conn, &d, "identity_conflict").await?;
                }
                Err(MergeError::NotActive(_) | MergeError::NotFound(_)) => {}
                Err(MergeError::Store(e)) => return Err(e),
                Err(e) => return Err(StoreError::Protocol(e.to_string())),
            },
        }
    }
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::normalize::norm;

    fn al(text: &str, origin: &str, bp: u16) -> Al {
        Al {
            text: text.into(),
            norm: norm(text),
            origin: origin.into(),
            bp,
        }
    }
    fn cand(key: &str, ty: &str, name: &str, evidence: i64, aliases: Vec<Al>) -> Cand {
        Cand {
            key: key.into(),
            ty: ty.into(),
            name: name.into(),
            norm: norm(name),
            evidence,
            aliases,
        }
    }

    #[test]
    fn a_roster_row_with_both_names_merges() {
        let c = vec![
            cand(
                "p:zh",
                "Person",
                "邵婉清",
                3,
                vec![al("Wanqing Shao", "rule", 9500)],
            ),
            cand("p:en", "Person", "Wanqing Shao", 1, vec![]),
        ];
        let d = decide(&c);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].kind, d[0].why), (Kind::Merge, "roster"));
        assert_eq!((d[0].into.as_str(), d[0].from.as_str()), ("p:zh", "p:en"));
    }

    #[test]
    fn two_entities_that_list_each_other_merge() {
        let c = vec![
            cand(
                "p:zh",
                "Person",
                "白若冰",
                2,
                vec![al("Ruobing Bai", "llm", 7000)],
            ),
            cand(
                "p:en",
                "Person",
                "Ruobing Bai",
                2,
                vec![al("白若冰", "llm", 7000)],
            ),
        ];
        let d = decide(&c);
        assert_eq!((d[0].kind, d[0].why), (Kind::Merge, "mutual"));
        assert_eq!(d[0].into, "p:en", "equal evidence: the smaller key stays");
    }

    #[test]
    fn a_one_way_model_claim_alone_is_only_a_candidate() {
        let c = vec![
            cand(
                "p:zh",
                "Person",
                "顾明远",
                2,
                vec![al("Someone Else", "llm", 7000)],
            ),
            cand("p:en", "Person", "Someone Else", 2, vec![]),
        ];
        assert_eq!(decide(&c)[0].kind, Kind::Review);
    }

    #[test]
    fn a_one_way_claim_that_the_pinyin_confirms_merges() {
        let c = vec![
            cand(
                "p:zh",
                "Person",
                "顾明远",
                2,
                vec![al("Mingyuan Gu", "llm", 7000)],
            ),
            cand("p:en", "Person", "Mingyuan Gu", 2, vec![]),
        ];
        assert_eq!(
            (decide(&c)[0].kind, decide(&c)[0].why),
            (Kind::Merge, "claim+pinyin")
        );
    }

    #[test]
    fn the_pinyin_alone_never_merges() {
        let c = vec![
            cand("p:zh", "Person", "白若冰", 2, vec![]),
            cand("p:en", "Person", "Ruobing Bai", 2, vec![]),
        ];
        let d = decide(&c);
        assert_eq!((d[0].kind, d[0].why), (Kind::Review, "pinyin"));
    }

    #[test]
    fn two_people_with_one_pinyin_are_never_merged() {
        // 王伟 and 王薇 both read Wang Wei.
        let c = vec![
            cand("p:1", "Person", "王伟", 2, vec![]),
            cand("p:2", "Person", "王薇", 2, vec![]),
            cand("p:en", "Person", "Wei Wang", 2, vec![]),
        ];
        let d = decide(&c);
        assert_eq!(d.len(), 2);
        assert!(
            d.iter()
                .all(|x| x.kind == Kind::Review && x.why == "ambiguous")
        );
    }

    #[test]
    fn a_roster_claim_does_not_merge_when_the_name_has_two_partners() {
        let c = vec![
            cand(
                "p:1",
                "Person",
                "王伟",
                2,
                vec![al("Wei Wang", "rule", 9500)],
            ),
            cand(
                "p:2",
                "Person",
                "王薇",
                2,
                vec![al("Wei Wang", "rule", 9500)],
            ),
            cand("p:en", "Person", "Wei Wang", 1, vec![]),
        ];
        assert!(decide(&c).iter().all(|x| x.kind == Kind::Review));
    }

    #[test]
    fn different_types_and_same_script_names_are_left_alone() {
        let c = vec![
            cand(
                "p:zh",
                "Person",
                "白若冰",
                2,
                vec![al("Ruobing Bai", "rule", 9500)],
            ),
            cand("o:en", "Organization", "Ruobing Bai", 2, vec![]),
            cand("p:zh2", "Person", "白若水", 2, vec![]),
        ];
        assert!(decide(&c).is_empty());
    }

    #[test]
    fn organisations_merge_on_claims_only_never_on_pinyin() {
        let c = vec![
            cand(
                "o:zh",
                "Organization",
                "恒鼎光电",
                2,
                vec![al("Hengding Optoelectronics", "llm", 7000)],
            ),
            cand(
                "o:en",
                "Organization",
                "Hengding Optoelectronics",
                2,
                vec![al("恒鼎光电", "llm", 7000)],
            ),
        ];
        assert_eq!(decide(&c)[0].kind, Kind::Merge);
        let c2 = vec![
            cand("o:zh", "Organization", "恒鼎光电", 2, vec![]),
            cand(
                "o:en",
                "Organization",
                "Hengding Optoelectronics",
                2,
                vec![],
            ),
        ];
        assert!(decide(&c2).is_empty());
    }
}
