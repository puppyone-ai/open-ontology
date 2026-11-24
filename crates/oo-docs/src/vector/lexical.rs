//! Lexical retrieval channel `bm25/1`: an in-memory inverted index over block
//! text with Okapi BM25 ranking (k1 = 1.2, b = 0.75).
//!
//! It sits next to `hashgram/1` for texts where exact words matter (labels,
//! field names, identifiers in long UI observations) and the 256-bucket
//! hash vector is too coarse. Like the vector channel it uses integers only:
//! the inverse document frequency is a Q16 fixed-point base-2 logarithm
//! computed by repeated squaring, and the term weight is an `i128` ratio, so
//! the same index and query give the same scores and order on every
//! platform. Base 2 instead of e scales every score by the same constant and
//! leaves the ranking unchanged. Ties break on the lower document id, which
//! is insertion order.

use std::collections::{BTreeSet, HashMap};

/// Model name of this channel; part of any parameters that use it.
pub const MODEL_VERSION: &str = "bm25/1";

/// BM25 k1 in thousandths.
const K1_MILLI: i128 = 1200;
/// BM25 b in thousandths.
const B_MILLI: i128 = 750;

/// Words too common to rank on (English function words and question
/// scaffolding). Kept short on purpose: BM25 already discounts frequent terms.
const STOPWORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "an", "and", "any", "are", "as", "at", "be", "been",
    "before", "being", "but", "by", "can", "could", "did", "do", "does", "for", "from", "had",
    "has", "have", "how", "i", "if", "in", "into", "is", "it", "its", "me", "my", "of", "on",
    "one", "or", "our", "should", "so", "than", "that", "the", "their", "them", "then", "there",
    "these", "they", "this", "to", "was", "we", "were", "what", "when", "where", "which", "while",
    "who", "will", "with", "would", "you", "your",
];

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff)
}

fn stem(word: &str) -> String {
    let n = word.chars().count();
    if n > 4 && word.ends_with("ies") {
        return format!("{}y", &word[..word.len() - 3]);
    }
    if n > 3 && word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("us") {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

fn push_word(word: &mut String, out: &mut Vec<String>) {
    if word.is_empty() {
        return;
    }
    let w = std::mem::take(word);
    let keep = w.chars().count() > 1 || w.chars().all(|c| c.is_ascii_digit());
    if keep && STOPWORDS.binary_search(&w.as_str()).is_err() {
        out.push(stem(&w));
    }
}

/// Index terms of `text`: lowercase alphanumeric runs (one term per CJK
/// character), stopwords dropped, a light plural stem applied.
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if is_cjk(c) {
            push_word(&mut word, &mut out);
            out.push(c.to_string());
        } else if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        } else {
            push_word(&mut word, &mut out);
        }
    }
    push_word(&mut word, &mut out);
    out
}

/// `log2(x)` in Q16 fixed point (`x >= 1`), by repeated squaring of the
/// normalized mantissa. Exact to the last bit on every platform.
pub fn log2_q16(x: u64) -> i64 {
    if x == 0 {
        return 0;
    }
    let n = 63 - x.leading_zeros();
    let mut m: u128 = if n <= 62 {
        u128::from(x) << (62 - n)
    } else {
        u128::from(x) >> (n - 62)
    };
    let mut frac: i64 = 0;
    for i in (0..16).rev() {
        m = (m * m) >> 62;
        if m >= (2u128 << 62) {
            m >>= 1;
            frac |= 1 << i;
        }
    }
    (i64::from(n) << 16) | frac
}

/// An append-only BM25 index. Document ids are dense and start at 0.
#[derive(Clone, Debug, Default)]
pub struct Index {
    postings: HashMap<String, Vec<(u32, u32)>>,
    lens: Vec<u32>,
    total: u64,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of documents.
    pub fn len(&self) -> usize {
        self.lens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lens.is_empty()
    }

    /// Add one document and return its id.
    pub fn add(&mut self, text: &str) -> u32 {
        let id = u32::try_from(self.lens.len()).unwrap_or(u32::MAX);
        let words = terms(text);
        let mut tf: HashMap<String, u32> = HashMap::new();
        for w in &words {
            *tf.entry(w.clone()).or_insert(0) += 1;
        }
        for (w, n) in tf {
            self.postings.entry(w).or_default().push((id, n));
        }
        let len = u32::try_from(words.len()).unwrap_or(u32::MAX);
        self.lens.push(len);
        self.total += u64::from(len);
        id
    }

    /// Inverse document frequency of a term with document frequency `df`,
    /// Q16: `log2((2N + 2) / (2 df + 1))`, always positive.
    fn idf_q16(&self, df: usize) -> i64 {
        let n = self.lens.len() as u64;
        log2_q16(2 * n + 2) - log2_q16(2 * df as u64 + 1)
    }

    /// Score every document that shares a term with `query`; the best
    /// `limit` as `(id, score)` by score descending, then id ascending.
    /// Repeated query terms count once.
    pub fn search(&self, query: &str, limit: usize) -> Vec<(u32, i64)> {
        if self.lens.is_empty() || limit == 0 {
            return Vec::new();
        }
        let n = self.lens.len() as i128;
        let total = i128::from(self.total.max(1));
        let qterms: BTreeSet<String> = terms(query).into_iter().collect();
        let mut acc: Vec<i64> = vec![0; self.lens.len()];
        let mut touched: Vec<u32> = Vec::new();
        for t in &qterms {
            let Some(list) = self.postings.get(t) else {
                continue;
            };
            let idf = i128::from(self.idf_q16(list.len()));
            for &(doc, tf) in list {
                let tf = i128::from(tf);
                let dl = i128::from(self.lens[doc as usize]);
                // tf (k1 + 1) / (tf + k1 (1 - b + b dl / avgdl)), avgdl = total / n,
                // with numerator and denominator scaled by 1e6 * total.
                let den = tf * 1_000_000 * total
                    + K1_MILLI * ((1000 - B_MILLI) * total + B_MILLI * dl * n);
                let num = idf * tf * (1_000 * (1000 + K1_MILLI)) * total;
                let add = i64::try_from(num / den).unwrap_or(i64::MAX);
                let slot = &mut acc[doc as usize];
                if *slot == 0 {
                    touched.push(doc);
                }
                *slot = slot.saturating_add(add);
            }
        }
        let mut hits: Vec<(u32, i64)> = touched
            .into_iter()
            .map(|d| (d, acc[d as usize]))
            .filter(|(_, s)| *s > 0)
            .collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopwords_are_sorted_for_binary_search() {
        let mut s = STOPWORDS.to_vec();
        s.sort_unstable();
        assert_eq!(s, STOPWORDS);
    }

    #[test]
    fn terms_fold_case_drop_stopwords_and_stem() {
        assert_eq!(
            terms("The Filters dropdown, of Incidents!"),
            vec!["filter", "dropdown", "incident"]
        );
        assert_eq!(terms("x 7 ab"), vec!["7", "ab"]);
        assert_eq!(terms("项目a"), vec!["项", "目"]);
        assert_eq!(
            terms("categories class status"),
            vec!["category", "class", "status"]
        );
    }

    #[test]
    fn log2_is_exact_on_powers_and_monotone() {
        assert_eq!(log2_q16(1), 0);
        assert_eq!(log2_q16(2), 1 << 16);
        assert_eq!(log2_q16(1024), 10 << 16);
        assert_eq!(log2_q16(u64::MAX) >> 16, 63);
        // log2(3) = 1.58496..., Q16 = 103872.
        assert_eq!(log2_q16(3), 103_872);
        let mut prev = -1;
        for x in 1..5000u64 {
            let v = log2_q16(x);
            assert!(v >= prev);
            prev = v;
        }
    }

    #[test]
    fn rare_terms_and_matches_rank_first() {
        let mut ix = Index::new();
        ix.add("toolbox moderation log wiki");
        ix.add("create forum button submit");
        ix.add("submit post title body submit");
        ix.add("unrelated words entirely");
        let hits = ix.search("Which link is in the Toolbox section?", 10);
        assert_eq!(hits[0].0, 0);
        assert_eq!(hits.len(), 1);
        let hits = ix.search("submit forum", 10);
        assert_eq!(hits[0].0, 1);
        assert!(hits.iter().all(|h| h.0 != 3));
        assert!(ix.search("", 10).is_empty());
        assert!(ix.search("toolbox", 0).is_empty());
    }

    #[test]
    fn ties_break_on_insertion_order_and_scores_replay() {
        let mut ix = Index::new();
        ix.add("alpha beta");
        ix.add("alpha beta");
        ix.add("gamma");
        let a = ix.search("alpha", 5);
        assert_eq!(a.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(a[0].1, a[1].1);
        let mut again = Index::new();
        for t in ["alpha beta", "alpha beta", "gamma"] {
            again.add(t);
        }
        assert_eq!(again.search("alpha", 5), a);
        // Pinned value: a change of formula or constants is a new model version.
        assert_eq!(a[0].1, 41_078);
    }

    #[test]
    fn longer_documents_score_lower_for_the_same_tf() {
        let mut ix = Index::new();
        ix.add("needle");
        ix.add("needle hay hay hay hay hay hay hay hay hay");
        ix.add("other");
        let h = ix.search("needle", 5);
        assert_eq!(h[0].0, 0);
        assert!(h[0].1 > h[1].1);
    }
}
