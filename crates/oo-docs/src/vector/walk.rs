//! `walk/1`: anchored one-hop graph ranking over resolved entities.
//!
//! A question that names entities ("drugs that target CYP3A4 and treat
//! strongyloidiasis") is answered by nodes adjacent to every named entity,
//! not by nodes whose text merely repeats the names. Each named entity is an
//! [`Anchor`]: the node ids the resolver bound the mention to (several when
//! the name is ambiguous) and the relations allowed between the anchor and an
//! answer (empty = any). A candidate scores one `weight` per anchor it is
//! adjacent to over an allowed relation and half a `weight` per anchor it is
//! adjacent to only over another relation (the relation a question implies is
//! a guess; the adjacency is a fact), plus its text score from another channel
//! (`bm25/1`). So the intersection of the walks ranks first and text breaks
//! ties inside each level. Anchor nodes themselves lose one level (a question
//! rarely asks for the entity it names).
//!
//! Same replay rule as the other channels: integers only, results sorted by
//! score descending then node id ascending, and the order of the anchors does
//! not change the result.

use std::collections::{BTreeMap, BTreeSet};

/// Name of this channel; part of any parameters that use it.
pub const MODEL_VERSION: &str = "walk/1";
/// Name of the specificity-weighted variant ([`rank_specific`]).
pub const MODEL_VERSION_SPECIFIC: &str = "walk/2";
/// At most this many anchors count (one bit each).
pub const MAX_ANCHORS: usize = 63;

/// Typed adjacency: `adj[node]` lists `(relation, neighbor)`, sorted, no
/// duplicates. Edges are stored in both directions.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    adj: Vec<Vec<(u16, u32)>>,
}

impl Graph {
    /// An empty graph over `nodes` node ids.
    pub fn new(nodes: usize) -> Self {
        Self {
            adj: vec![Vec::new(); nodes],
        }
    }

    pub fn len(&self) -> usize {
        self.adj.len()
    }

    pub fn is_empty(&self) -> bool {
        self.adj.is_empty()
    }

    /// Add an undirected typed edge; ids out of range are ignored.
    pub fn add_edge(&mut self, a: u32, b: u32, relation: u16) {
        let n = self.adj.len();
        if (a as usize) < n && (b as usize) < n {
            self.adj[a as usize].push((relation, b));
            self.adj[b as usize].push((relation, a));
        }
    }

    /// Sort and deduplicate the adjacency lists; call once after loading.
    pub fn seal(&mut self) {
        for list in &mut self.adj {
            list.sort_unstable();
            list.dedup();
        }
    }

    /// Neighbors of `node` as `(relation, neighbor)`.
    pub fn neighbors(&self, node: u32) -> &[(u16, u32)] {
        self.adj.get(node as usize).map_or(&[], Vec::as_slice)
    }

    /// Number of stored adjacency entries (twice the edge count).
    pub fn entries(&self) -> usize {
        self.adj.iter().map(Vec::len).sum()
    }
}

/// One named entity of a question.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// Nodes the mention resolved to.
    pub nodes: Vec<u32>,
    /// Relations allowed between this anchor and an answer; empty = any.
    pub relations: Vec<u16>,
}

/// Rank candidates. `accept` filters answer nodes (for example by type);
/// `text` holds the text-channel score of the candidates it knows (others
/// count 0). Candidates are the accepted neighbors of the anchors plus every
/// accepted node in `text`. Returns at most `k` `(node, score)`.
pub fn rank(
    graph: &Graph,
    anchors: &[Anchor],
    accept: impl Fn(u32) -> bool,
    text: &BTreeMap<u32, i64>,
    weight: i64,
    k: usize,
) -> Vec<(u32, i64)> {
    let anchors = canonical(anchors);

    // Per candidate: anchors reached over an allowed relation, and anchors
    // reached over any relation (one bit per anchor each).
    let mut hit: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    for (bit, a) in anchors.iter().enumerate() {
        for &node in &a.nodes {
            for &(r, nb) in graph.neighbors(node) {
                if !accept(nb) {
                    continue;
                }
                let e = hit.entry(nb).or_insert((0, 0));
                e.1 |= 1 << bit;
                if a.relations.is_empty() || a.relations.binary_search(&r).is_ok() {
                    e.0 |= 1 << bit;
                }
            }
        }
    }
    let named: BTreeSet<u32> = anchors
        .iter()
        .flat_map(|a| a.nodes.iter().copied())
        .collect();
    let mut cands: BTreeSet<u32> = hit.keys().copied().collect();
    cands.extend(text.keys().copied().filter(|n| accept(*n)));
    let mut out: Vec<(u32, i64)> = cands
        .into_iter()
        .map(|n| {
            // In half levels: 2 per allowed-relation anchor, 1 per other.
            let (strict, any) = hit.get(&n).copied().unwrap_or((0, 0));
            let halves = i64::from(strict.count_ones()) + i64::from(any.count_ones())
                - 2 * i64::from(named.contains(&n));
            let t = text.get(&n).copied().unwrap_or(0);
            (n, halves.saturating_mul(weight / 2).saturating_add(t))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.truncate(k);
    out
}

/// `walk/2`: [`rank`] with each anchor weighted by how specific it is. An
/// anchor that reaches a quarter of the graph ("color: black", a broad
/// category) says little about which candidate is meant; one that reaches
/// three nodes says a lot. Each anchor's level is scaled by
/// `idf(reach) / idf(1)`, with the BM25 inverse document frequency of
/// `bm25/1` (`log2((2N + 2) / (2 reach + 1))`, Q16, `N` = graph nodes), so a
/// unique anchor still counts one full `weight`. Same integer and ordering
/// rules as `walk/1`.
pub fn rank_specific(
    graph: &Graph,
    anchors: &[Anchor],
    accept: impl Fn(u32) -> bool,
    text: &BTreeMap<u32, i64>,
    weight: i64,
    k: usize,
) -> Vec<(u32, i64)> {
    let anchors = canonical(anchors);
    let n = graph.len() as u64;
    let top = crate::vector::lexical::log2_q16(2 * n + 2);
    let idf = |reach: u64| (top - crate::vector::lexical::log2_q16(2 * reach + 1)).max(0);
    let full = idf(1).max(1);
    let mut score: BTreeMap<u32, i64> = BTreeMap::new();
    for a in &anchors {
        // Best edge per candidate for this anchor: 2 = allowed relation, 1 = other.
        let mut best: BTreeMap<u32, i64> = BTreeMap::new();
        for &node in &a.nodes {
            for &(r, nb) in graph.neighbors(node) {
                if !accept(nb) {
                    continue;
                }
                let h = if a.relations.is_empty() || a.relations.binary_search(&r).is_ok() {
                    2
                } else {
                    1
                };
                let e = best.entry(nb).or_insert(0);
                *e = (*e).max(h);
            }
        }
        let half = i64::try_from(
            i128::from(weight / 2) * i128::from(idf(best.len() as u64)) / i128::from(full),
        )
        .unwrap_or(i64::MAX);
        for (nb, h) in best {
            let e = score.entry(nb).or_insert(0);
            *e = e.saturating_add(h * half);
        }
    }
    let named: BTreeSet<u32> = anchors
        .iter()
        .flat_map(|a| a.nodes.iter().copied())
        .collect();
    let mut cands: BTreeSet<u32> = score.keys().copied().collect();
    cands.extend(text.keys().copied().filter(|n| accept(*n)));
    let mut out: Vec<(u32, i64)> = cands
        .into_iter()
        .map(|c| {
            let g =
                score.get(&c).copied().unwrap_or(0) - if named.contains(&c) { weight } else { 0 };
            (c, g.saturating_add(text.get(&c).copied().unwrap_or(0)))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.truncate(k);
    out
}

/// Canonical anchor order, so the caller's order cannot matter.
fn canonical(anchors: &[Anchor]) -> Vec<Anchor> {
    let mut anchors: Vec<Anchor> = anchors
        .iter()
        .map(|a| {
            let mut a = a.clone();
            a.nodes.sort_unstable();
            a.nodes.dedup();
            a.relations.sort_unstable();
            a.relations.dedup();
            a
        })
        .filter(|a| !a.nodes.is_empty())
        .collect();
    anchors.sort();
    anchors.dedup();
    anchors.truncate(MAX_ANCHORS);
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 0 drug A, 1 drug B, 2 gene G, 3 disease D, 4 drug C.
    /// A targets G and treats D; B targets G; C treats D.
    fn toy() -> Graph {
        let mut g = Graph::new(5);
        g.add_edge(0, 2, 0); // target
        g.add_edge(0, 3, 1); // indication
        g.add_edge(1, 2, 0);
        g.add_edge(4, 3, 1);
        g.add_edge(4, 3, 1); // duplicate edge is dropped by seal
        g.seal();
        g
    }

    fn drugs(n: u32) -> bool {
        matches!(n, 0 | 1 | 4)
    }

    #[test]
    fn intersection_ranks_first_and_text_breaks_ties() {
        let g = toy();
        assert_eq!(g.entries(), 8);
        let anchors = [
            Anchor {
                nodes: vec![2],
                relations: vec![],
            },
            Anchor {
                nodes: vec![3],
                relations: vec![],
            },
        ];
        let text = BTreeMap::from([(4, 900), (1, 100)]);
        let r = rank(&g, &anchors, drugs, &text, 1000, 10);
        assert_eq!(r, vec![(0, 2000), (4, 1900), (1, 1100)]);
    }

    #[test]
    fn relation_filter_and_named_anchor_penalty() {
        let g = toy();
        // From G with only "indication" allowed: A and B are reached over
        // "target" and get half a level.
        let anchors = [Anchor {
            nodes: vec![2],
            relations: vec![1],
        }];
        let r = rank(&g, &anchors, |_| true, &BTreeMap::new(), 1000, 10);
        assert_eq!(r, vec![(0, 500), (1, 500)]);
        // Two anchors, one over the allowed relation: A (both, one strict)
        // ahead of C (one strict) ahead of B (one loose).
        let anchors = [
            Anchor {
                nodes: vec![3],
                relations: vec![1],
            },
            Anchor {
                nodes: vec![2],
                relations: vec![1],
            },
        ];
        let r = rank(&g, &anchors, drugs, &BTreeMap::new(), 1000, 10);
        assert_eq!(r, vec![(0, 1500), (4, 1000), (1, 500)]);
        // An anchor node that is also a text hit drops one level.
        let anchors = [Anchor {
            nodes: vec![0],
            relations: vec![],
        }];
        let r = rank(&g, &anchors, drugs, &BTreeMap::from([(0, 5)]), 1000, 10);
        assert_eq!(r, vec![(0, -995)]);
    }

    #[test]
    fn pinned_digest() {
        // A fixed graph and query; the ranking is part of the replay contract.
        let mut g = Graph::new(50);
        for i in 0..50u32 {
            g.add_edge(i, (i * 7 + 3) % 50, u16::try_from(i % 3).unwrap_or(0));
            g.add_edge(i, (i * 11 + 5) % 50, 1);
        }
        g.seal();
        let anchors = [
            Anchor {
                nodes: vec![3, 10],
                relations: vec![1],
            },
            Anchor {
                nodes: vec![7],
                relations: vec![],
            },
        ];
        let text: BTreeMap<u32, i64> = (0..50u32).map(|i| (i, i64::from((i * 37) % 101))).collect();
        let r = rank(&g, &anchors, |n| n % 2 == 0, &text, 1 << 20, 6);
        let flat: Vec<String> = r.iter().map(|(n, s)| format!("{n}:{s}")).collect();
        assert_eq!(
            flat.join(","),
            "38:1048669,2:1048650,32:1048649,18:1048636,22:1048582,24:524368",
        );
    }

    #[test]
    fn specific_anchor_outweighs_a_hub() {
        // 0..=9 products; 10 a hub category linked to all of them; 11 a
        // brand linked to 3 and 4.
        let mut g = Graph::new(12);
        for p in 0..10 {
            g.add_edge(p, 10, 0);
        }
        g.add_edge(3, 11, 1);
        g.add_edge(4, 11, 1);
        g.seal();
        let products = |n: u32| n < 10;
        let hub = Anchor {
            nodes: vec![10],
            relations: vec![],
        };
        let brand = Anchor {
            nodes: vec![11],
            relations: vec![],
        };
        // Text prefers 7; under walk/1 the hub lifts everyone equally, the
        // brand lifts 3 and 4 by a full level.
        let text = BTreeMap::from([(7, 300), (4, 100)]);
        let one = rank(&g, &[hub.clone(), brand.clone()], products, &text, 1000, 3);
        assert_eq!(one[0], (4, 2100));
        // walk/2: the hub counts little, the brand nearly a full level, and
        // a unique anchor exactly one.
        let two = rank_specific(&g, &[hub, brand], products, &text, 1000, 3);
        assert_eq!(two.iter().map(|x| x.0).collect::<Vec<_>>(), vec![4, 3, 7]);
        let only = Anchor {
            nodes: vec![9],
            relations: vec![],
        };
        let three = rank_specific(&g, &[only], |n| n == 10, &BTreeMap::new(), 1000, 3);
        assert_eq!(three, vec![(10, 1000)]);
    }

    proptest! {
        #[test]
        fn anchor_order_does_not_matter(
            edges in proptest::collection::vec((0u32..30, 0u32..30, 0u16..3), 0..120),
            a in proptest::collection::vec((proptest::collection::vec(0u32..30, 1..3), proptest::collection::vec(0u16..3, 0..2)), 0..4),
            text in proptest::collection::btree_map(0u32..30, 0i64..1000, 0..10),
        ) {
            let mut g = Graph::new(30);
            for (x, y, r) in &edges {
                g.add_edge(*x, *y, *r);
            }
            g.seal();
            let anchors: Vec<Anchor> = a.iter().map(|(n, r)| Anchor { nodes: n.clone(), relations: r.clone() }).collect();
            let mut rev = anchors.clone();
            rev.reverse();
            for x in &mut rev {
                x.nodes.reverse();
            }
            let one = rank(&g, &anchors, |n| n % 3 != 0, &text, 10_000, 15);
            let two = rank(&g, &rev, |n| n % 3 != 0, &text, 10_000, 15);
            prop_assert_eq!(&one, &two);
            let s1 = rank_specific(&g, &anchors, |n| n % 3 != 0, &text, 10_000, 15);
            let s2 = rank_specific(&g, &rev, |n| n % 3 != 0, &text, 10_000, 15);
            prop_assert_eq!(&s1, &s2);
            for w in s1.windows(2) {
                prop_assert!(w[0].1 > w[1].1 || (w[0].1 == w[1].1 && w[0].0 < w[1].0));
            }
            // Sorted by score, then id; every result is accepted.
            for w in one.windows(2) {
                prop_assert!(w[0].1 > w[1].1 || (w[0].1 == w[1].1 && w[0].0 < w[1].0));
            }
            prop_assert!(one.iter().all(|(n, _)| n % 3 != 0));
        }
    }
}
