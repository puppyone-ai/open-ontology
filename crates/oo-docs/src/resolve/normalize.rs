//! Canonical names, entity keys and fact keys (10 §3.6). Pure.

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::extract::model::EntityType;

/// Prefix of the entity key preimage.
pub const ENTITY_KEY_DOMAIN: &str = "oo.docs.entity/v1\0";
/// Prefix of the fact key preimage.
pub const FACT_KEY_DOMAIN: &str = "oo.docs.fact/v1\0";

const OPENERS: &[char] = &['\u{201C}', '\u{300C}', '\u{300A}', '"', '\'', '\u{2018}'];
const CLOSERS: &[char] = &['\u{201D}', '\u{300D}', '\u{300B}', '"', '\'', '\u{2019}'];

fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}')
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    out
}

/// Canonical name `norm` (10 §3.6): NFKC, zero-width characters removed,
/// trimmed with inner whitespace collapsed to one space, simple lower case,
/// wrapping quotes and book-title marks removed. No pinyin, no Han script
/// conversion. Wrapping is stripped while the first and the last character
/// are an opener and a closer from the set (also for the ASCII quotes, which
/// are their own pair); the result is trimmed again after each strip.
pub fn norm(name: &str) -> String {
    let nfkc: String = name.nfkc().filter(|c| !is_zero_width(*c)).collect();
    let mut chars: Vec<char> = collapse(&nfkc).to_lowercase().chars().collect();
    while chars.len() >= 2
        && OPENERS.contains(&chars[0])
        && chars.last().is_some_and(|c| CLOSERS.contains(c))
    {
        let inner: String = chars[1..chars.len() - 1].iter().collect();
        chars = collapse(&inner).chars().collect();
    }
    chars.into_iter().collect()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `<type_slug>:<hex(BLAKE3("oo.docs.entity/v1\0" ‖ type ‖ "\0" ‖
/// thing_label_norm ‖ "\0" ‖ norm))[..24]>`.
///
/// `type` in the preimage is the type name (`Person`, ...), `thing_label_norm`
/// is empty unless the type is `Thing`. `identity_canonical` (the canonical
/// JSON of the identity attributes, used only to keep same-name different
/// people apart, 5.4 step 5) appends `"\0" ‖ identity_canonical`. `subkind` is
/// not part of the key (10 §3.6), so a preference and a decision with the same
/// name share a key.
pub fn entity_key(
    ty: EntityType,
    thing_label: Option<&str>,
    name_norm: &str,
    identity_canonical: Option<&str>,
) -> String {
    let mut h = blake3::Hasher::new();
    h.update(ENTITY_KEY_DOMAIN.as_bytes());
    h.update(ty.as_str().as_bytes());
    h.update(b"\0");
    if let Some(label) = thing_label {
        h.update(norm(label).as_bytes());
    }
    h.update(b"\0");
    h.update(name_norm.as_bytes());
    if let Some(identity) = identity_canonical {
        h.update(b"\0");
        h.update(identity.as_bytes());
    }
    let digest = h.finalize();
    format!("{}:{}", ty.slug(), &hex(digest.as_bytes())[..24])
}

/// Fact key: `hex(BLAKE3("oo.docs.fact/v1\0" ‖ subject_key ‖ "\0" ‖ predicate
/// ‖ "\0" ‖ (object_key | canonical(value)) ‖ "\0" ‖ (valid_from | "-")))`,
/// the full 64 hex characters. Re-extracting the same content gives the same
/// key, so overlays bind stably.
pub fn fact_key(
    subject_key: &str,
    predicate: &str,
    object_key: Option<&str>,
    value: Option<&Value>,
    valid_from: Option<&str>,
) -> String {
    let mut h = blake3::Hasher::new();
    h.update(FACT_KEY_DOMAIN.as_bytes());
    h.update(subject_key.as_bytes());
    h.update(b"\0");
    h.update(predicate.as_bytes());
    h.update(b"\0");
    match (object_key, value) {
        (Some(key), _) => h.update(key.as_bytes()),
        (None, Some(v)) => h.update(oo_builder::canonical_json(v).as_bytes()),
        (None, None) => &mut h,
    };
    h.update(b"\0");
    h.update(valid_from.unwrap_or("-").as_bytes());
    hex(h.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_rules() {
        assert_eq!(norm("  Evan\u{3000} Lin \t"), "evan lin");
        assert_eq!(norm("ＡＢＣ１２３"), "abc123");
        assert_eq!(norm("星\u{200B}图\u{FEFF}看板"), "星图看板");
        assert_eq!(norm("《星图看板》"), "星图看板");
        assert_eq!(norm("“「星图」”"), "星图");
        assert_eq!(norm("\"Quoted\""), "quoted");
        assert_eq!(norm("'x'"), "x");
        assert_eq!(norm("“"), "“");
        assert_eq!(norm(""), "");
        // No Han conversion, no pinyin.
        assert_ne!(norm("開源"), norm("开源"));
    }

    #[test]
    fn norm_is_idempotent_on_samples() {
        for s in [
            "《 A  b 》",
            "ＦＵＬＬ ｗｉｄｔｈ",
            "x\u{200D}y",
            "\u{FEFF}",
            "“a”“b”",
        ] {
            assert_eq!(norm(&norm(s)), norm(s), "{s:?}");
        }
    }

    #[test]
    fn entity_key_shape_and_stability() {
        let k = entity_key(EntityType::Person, None, &norm("陈子墨"), None);
        assert!(k.starts_with("person:"));
        assert_eq!(k.len(), "person:".len() + 24);
        assert!(k["person:".len()..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            k,
            entity_key(EntityType::Person, None, &norm("陈子墨"), None)
        );
        assert_ne!(
            k,
            entity_key(EntityType::Project, None, &norm("陈子墨"), None)
        );
        assert_ne!(
            k,
            entity_key(
                EntityType::Person,
                None,
                &norm("陈子墨"),
                Some("{\"email\":\"a@b\"}")
            )
        );
        let t1 = entity_key(EntityType::Thing, Some("Vendor"), "x", None);
        let t2 = entity_key(EntityType::Thing, Some("vendor"), "x", None);
        assert_eq!(t1, t2, "label is normalized");
        assert!(t1.starts_with("thing:"));
    }

    #[test]
    fn entity_key_matches_independent_preimage() {
        let mut h = blake3::Hasher::new();
        h.update(b"oo.docs.entity/v1\0Project\0\0xingtu");
        let expect = format!("project:{}", &h.finalize().to_hex()[..24]);
        assert_eq!(
            entity_key(EntityType::Project, None, "xingtu", None),
            expect
        );
    }

    #[test]
    fn fact_key_matches_independent_preimage_and_varies() {
        let a = fact_key(
            "person:aa",
            "owns",
            Some("project:bb"),
            None,
            Some("2026-01-01"),
        );
        let mut h = blake3::Hasher::new();
        h.update(b"oo.docs.fact/v1\0person:aa\0owns\0project:bb\x002026-01-01");
        assert_eq!(a, h.finalize().to_hex().to_string());
        assert_eq!(a.len(), 64);
        let b = fact_key("person:aa", "owns", Some("project:bb"), None, None);
        assert_ne!(a, b);
        let v = serde_json::json!({"text": "active"});
        let c = fact_key("project:bb", "attr.status", None, Some(&v), None);
        let mut h = blake3::Hasher::new();
        h.update(b"oo.docs.fact/v1\0project:bb\0attr.status\0{\"text\":\"active\"}\0-");
        assert_eq!(c, h.finalize().to_hex().to_string());
    }
}
