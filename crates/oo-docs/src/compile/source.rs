//! Provenance tags `[src:<path>#<frag>]` (10 §3.5). Pure.

use oo_core::docir::Anchor;

/// Percent-encode a Drive-relative path (NFC): `]`, `#`, `%`, whitespace and
/// control characters become `%XX` per UTF-8 byte, upper-case hex.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, ']' | '#' | '%') || c.is_whitespace() || c.is_control() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Inverse of [`encode_path`]; `None` for malformed escapes or invalid UTF-8.
pub fn decode_path(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn sheet_ref(sheet: &str) -> String {
    if !sheet.is_empty() && sheet.chars().all(char::is_alphanumeric) {
        sheet.to_owned()
    } else {
        format!("'{}'", sheet.replace('\'', "''"))
    }
}

/// `R12` when an A1 range covers exactly one row from column A (`A12:F12`
/// style rows are how the csv parser anchors), else the range itself.
fn csv_frag(range: &str) -> String {
    let Some((a, b)) = range.split_once(':') else {
        return range.to_owned();
    };
    let split = |s: &str| {
        let i = s.find(|c: char| c.is_ascii_digit())?;
        Some((s[..i].to_owned(), s[i..].to_owned()))
    };
    match (split(a), split(b)) {
        (Some((ca, ra)), Some((_, rb))) if ca == "A" && ra == rb => format!("R{ra}"),
        _ => range.to_owned(),
    }
}

/// Fragment of an anchor, starting with `#`.
pub fn frag(anchor: &Anchor) -> String {
    match anchor {
        Anchor::Text {
            line_start,
            line_end,
            ..
        } => format!("#L{line_start}-L{line_end}"),
        Anchor::Cells { sheet: None, range } => format!("#{}", csv_frag(range)),
        Anchor::Cells {
            sheet: Some(sheet),
            range,
        } => format!("#{}!{range}", sheet_ref(sheet)),
        Anchor::Docx {
            para_start,
            para_end,
            table_index,
            cell_range,
        } => match (table_index, cell_range) {
            (Some(t), Some(r)) => format!("#T{t}!{r}"),
            _ => format!("#¶{para_start}-{para_end}"),
        },
        Anchor::Slide { slide, .. } => format!("#s{slide}"),
        Anchor::Pdf { page, .. } => format!("#p{page}"),
        Anchor::Media { .. } | Anchor::Frame { .. } | Anchor::Image { .. } => anchor.fragment(),
    }
}

/// `[src:<encoded path><frag>]`.
pub fn tag(path: &str, anchor: &Anchor) -> String {
    format!("[src:{}{}]", encode_path(path), frag(anchor))
}

/// Every `[src:…]` body in `line`, in order.
pub fn tags_in(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find("[src:") {
        let after = &rest[i + "[src:".len()..];
        let Some(end) = after.find(']') else { break };
        out.push(&after[..end]);
        rest = &after[end + 1..];
    }
    out
}

/// Split a tag body into the decoded path and the fragment (with `#`).
pub fn split_tag(body: &str) -> Option<(String, String)> {
    let i = body.find('#')?;
    Some((decode_path(&body[..i])?, body[i..].to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_encoding_round_trips_and_covers_the_listed_characters() {
        let p = "People/a b#c]d%e\t.md";
        let e = encode_path(p);
        assert_eq!(e, "People/a%20b%23c%5Dd%25e%09.md");
        assert_eq!(decode_path(&e).as_deref(), Some(p));
        assert_eq!(encode_path("人员/陈子墨.md"), "人员/陈子墨.md");
        assert_eq!(encode_path("a\u{3000}b"), "a%E3%80%80b");
        assert_eq!(decode_path("%zz"), None);
        assert_eq!(decode_path("%C3"), None, "invalid utf-8");
    }

    #[test]
    fn fragments() {
        let t = Anchor::Text {
            line_start: 12,
            line_end: 18,
            char_start: 0,
            char_end: 1,
        };
        assert_eq!(frag(&t), "#L12-L18");
        let a1 = Anchor::Cells {
            sheet: Some("Sheet 1".into()),
            range: "A1:C9".into(),
        };
        assert_eq!(frag(&a1), "#'Sheet 1'!A1:C9");
        let plain = Anchor::Cells {
            sheet: Some("Data".into()),
            range: "A1".into(),
        };
        assert_eq!(frag(&plain), "#Data!A1");
        let quote = Anchor::Cells {
            sheet: Some("it's".into()),
            range: "B2".into(),
        };
        assert_eq!(frag(&quote), "#'it''s'!B2");
        let csv = Anchor::Cells {
            sheet: None,
            range: "A12:F12".into(),
        };
        assert_eq!(frag(&csv), "#R12");
        let csv2 = Anchor::Cells {
            sheet: None,
            range: "A1:F40".into(),
        };
        assert_eq!(frag(&csv2), "#A1:F40");
        let d = Anchor::Docx {
            para_start: 12,
            para_end: 14,
            table_index: None,
            cell_range: None,
        };
        assert_eq!(frag(&d), "#¶12-14");
        let dt = Anchor::Docx {
            para_start: 0,
            para_end: 0,
            table_index: Some(2),
            cell_range: Some("R3C1".into()),
        };
        assert_eq!(frag(&dt), "#T2!R3C1");
        let s = Anchor::Slide {
            slide: 3,
            shape_id: None,
            para_start: None,
            para_end: None,
            notes: None,
        };
        assert_eq!(frag(&s), "#s3");
        let p = Anchor::Pdf {
            page: 5,
            bbox: [0, 0, 1, 1],
            charspan: None,
            page_w: None,
            page_h: None,
        };
        assert_eq!(frag(&p), "#p5");
    }

    #[test]
    fn tags_parse_back() {
        let t = Anchor::Text {
            line_start: 1,
            line_end: 2,
            char_start: 0,
            char_end: 1,
        };
        let line = format!("- x {} {} [src:+2]", tag("a b.md", &t), tag("c.md", &t));
        let tags = tags_in(&line);
        assert_eq!(tags, ["a%20b.md#L1-L2", "c.md#L1-L2", "+2"]);
        assert_eq!(split_tag(tags[0]), Some(("a b.md".into(), "#L1-L2".into())));
        assert_eq!(split_tag("+2"), None);
    }
}
