pub fn formula_prone(text: &str) -> bool {
    let rest = text.trim_start_matches(|ch: char| {
        ch.is_whitespace() || ch.is_control() || ch == '\u{FEFF}'
    });
    matches!(rest.chars().next(), Some('=' | '+' | '-' | '@'))
}

pub fn escape_text(text: &str) -> String {
    if formula_prone(text) {
        let mut out = String::with_capacity(text.len() + 1);
        out.push('\'');
        out.push_str(text);
        out
    } else {
        text.to_string()
    }
}

pub fn write_field(out: &mut Vec<u8>, field: &str) {
    let bytes = field.as_bytes();
    let quote = bytes
        .iter()
        .any(|byte| matches!(byte, b'"' | b',' | b'\n' | b'\r'));
    if !quote {
        out.extend_from_slice(bytes);
        return;
    }
    out.push(b'"');
    for &byte in bytes {
        if byte == b'"' {
            out.push(b'"');
        }
        out.push(byte);
    }
    out.push(b'"');
}

pub fn push_record_end(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\r\n");
}

pub fn xml_text(text: &str) -> (String, bool) {
    let mut out = String::new();
    let mut omitted = false;
    for ch in text.chars() {
        if !xml_ok(ch) {
            omitted = true;
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            ch => out.push(ch),
        }
    }
    (out, omitted)
}

fn xml_ok(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | '\u{0020}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}
