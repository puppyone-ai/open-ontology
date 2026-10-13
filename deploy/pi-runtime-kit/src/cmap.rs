//! Cmap coverage of one OpenType or TrueType font.
//! The reported spans are the cmap ranges clipped to named Unicode blocks.
//! They are not a vendor coverage claim.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub block: &'static str,
    pub start: u32,
    pub end: u32,
}

const BLOCKS: &[(&str, u32, u32)] = &[
    ("CJK Radicals Supplement", 0x2E80, 0x2EFF),
    ("Kangxi Radicals", 0x2F00, 0x2FDF),
    ("CJK Symbols and Punctuation", 0x3000, 0x303F),
    ("Hiragana", 0x3040, 0x309F),
    ("Katakana", 0x30A0, 0x30FF),
    ("Bopomofo", 0x3100, 0x312F),
    ("Hangul Jamo", 0x1100, 0x11FF),
    ("CJK Unified Ideographs Extension A", 0x3400, 0x4DBF),
    ("CJK Unified Ideographs", 0x4E00, 0x9FFF),
    ("Hangul Syllables", 0xAC00, 0xD7AF),
    ("CJK Compatibility Ideographs", 0xF900, 0xFAFF),
    ("CJK Unified Ideographs Extension B", 0x20000, 0x2A6DF),
    ("CJK Unified Ideographs Extension C", 0x2A700, 0x2B73F),
    ("CJK Unified Ideographs Extension D", 0x2B740, 0x2B81F),
    ("CJK Unified Ideographs Extension E", 0x2B820, 0x2CEAF),
    ("CJK Unified Ideographs Extension F", 0x2CEB0, 0x2EBEF),
    ("CJK Compatibility Ideographs Supplement", 0x2F800, 0x2FA1F),
];

pub fn cjk_spans(font: &[u8]) -> Result<Vec<Span>, String> {
    let ranges = cmap_ranges(font)?;
    let mut spans = Vec::new();
    for (block, block_start, block_end) in BLOCKS {
        let mut clipped = Vec::new();
        for (start, end) in &ranges {
            let lo = (*start).max(*block_start);
            let hi = (*end).min(*block_end);
            if lo <= hi {
                clipped.push((lo, hi));
            }
        }
        merge(&mut clipped);
        for (start, end) in clipped {
            spans.push(Span { block, start, end });
        }
    }
    Ok(spans)
}

pub fn spans_json(spans: &[Span]) -> String {
    let mut out = String::from("{\"ranges\":[");
    for (index, span) in spans.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"block\":");
        out.push_str(&crate::json::escape(span.block));
        out.push_str(",\"start\":\"");
        out.push_str(&format!("{:04X}", span.start));
        out.push_str("\",\"end\":\"");
        out.push_str(&format!("{:04X}", span.end));
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}

fn cmap_ranges(font: &[u8]) -> Result<Vec<(u32, u32)>, String> {
    let table = cmap_table(font)?;
    if table.len() < 4 {
        return Err("cmap is short".to_owned());
    }
    let count = u16_at(table, 2)? as usize;
    let records = 4 + count * 8;
    if table.len() < records {
        return Err("cmap records are short".to_owned());
    }
    let mut ranges = Vec::new();
    for index in 0..count {
        let record = 4 + index * 8;
        let offset = u32_at(table, record + 4)? as usize;
        let sub = table.get(offset..).ok_or("cmap subtable is outside the table")?;
        if sub.len() < 2 {
            return Err("cmap subtable is short".to_owned());
        }
        match u16_at(sub, 0)? {
            4 => ranges.extend(format4(sub)?),
            12 => ranges.extend(format12(sub)?),
            _ => {}
        }
    }
    merge(&mut ranges);
    Ok(ranges)
}

fn cmap_table(font: &[u8]) -> Result<&[u8], String> {
    if font.len() < 12 {
        return Err("font is short".to_owned());
    }
    let scaler = u32_at(font, 0)?;
    if scaler != 0x0001_0000 && scaler != u32::from_be_bytes(*b"OTTO") && scaler != u32::from_be_bytes(*b"true") {
        return Err("font scaler is not OpenType or TrueType".to_owned());
    }
    let num_tables = u16_at(font, 4)? as usize;
    let directory = 12 + num_tables * 16;
    if font.len() < directory {
        return Err("font table directory is short".to_owned());
    }
    for index in 0..num_tables {
        let entry = 12 + index * 16;
        if &font[entry..entry + 4] == b"cmap" {
            let offset = u32_at(font, entry + 8)? as usize;
            let length = u32_at(font, entry + 12)? as usize;
            let end = offset.checked_add(length).ok_or("cmap offset overflow")?;
            return font.get(offset..end).ok_or("cmap is outside the font").map_err(str::to_owned);
        }
    }
    Err("font has no cmap".to_owned())
}

fn format4(sub: &[u8]) -> Result<Vec<(u32, u32)>, String> {
    if sub.len() < 14 {
        return Err("format 4 is short".to_owned());
    }
    let length = u16_at(sub, 2)? as usize;
    if sub.len() < length {
        return Err("format 4 length exceeds the subtable".to_owned());
    }
    let seg_x2 = u16_at(sub, 6)? as usize;
    if !seg_x2.is_multiple_of(2) {
        return Err("format 4 segment count is odd".to_owned());
    }
    let segments = seg_x2 / 2;
    let need = 16 + segments * 8;
    if length < need {
        return Err("format 4 segments exceed the subtable".to_owned());
    }
    let mut ranges = Vec::new();
    for index in 0..segments {
        let end = u32::from(u16_at(sub, 14 + index * 2)?);
        let start = u32::from(u16_at(sub, 16 + segments * 2 + index * 2)?);
        if start == 0xFFFF {
            continue;
        }
        if start <= end {
            ranges.push((start, end));
        }
    }
    Ok(ranges)
}

fn format12(sub: &[u8]) -> Result<Vec<(u32, u32)>, String> {
    if sub.len() < 16 {
        return Err("format 12 is short".to_owned());
    }
    let length = u32_at(sub, 4)? as usize;
    if sub.len() < length {
        return Err("format 12 length exceeds the subtable".to_owned());
    }
    let groups = u32_at(sub, 12)? as usize;
    let need = groups.checked_mul(12).and_then(|body| body.checked_add(16)).ok_or("format 12 size overflow")?;
    if length < need {
        return Err("format 12 groups exceed the subtable".to_owned());
    }
    if groups > 100_000 {
        return Err("format 12 has too many groups".to_owned());
    }
    let mut ranges = Vec::new();
    for index in 0..groups {
        let at = 16 + index * 12;
        let start = u32_at(sub, at)?;
        let end = u32_at(sub, at + 4)?;
        if start <= end {
            ranges.push((start, end));
        }
    }
    Ok(ranges)
}

fn merge(ranges: &mut Vec<(u32, u32)>) {
    if ranges.is_empty() {
        return;
    }
    ranges.sort_by_key(|range| (range.0, range.1));
    let mut merged = Vec::with_capacity(ranges.len());
    let mut current = ranges[0];
    for (start, end) in ranges.iter().skip(1).copied() {
        if start <= current.1.saturating_add(1) {
            current.1 = current.1.max(end);
        } else {
            merged.push(current);
            current = (start, end);
        }
    }
    merged.push(current);
    *ranges = merged;
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data.get(offset..offset + 2).ok_or("truncated u16")?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data.get(offset..offset + 4).ok_or("truncated u32")?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub fn synthetic_format4(start: u16, end: u16) -> Vec<u8> {
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&1_u16.to_be_bytes());
    cmap.extend_from_slice(&3_u16.to_be_bytes());
    cmap.extend_from_slice(&1_u16.to_be_bytes());
    cmap.extend_from_slice(&12_u32.to_be_bytes());
    let sub_at = cmap.len();
    cmap.extend_from_slice(&4_u16.to_be_bytes());
    cmap.extend_from_slice(&32_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&4_u16.to_be_bytes());
    cmap.extend_from_slice(&4_u16.to_be_bytes());
    cmap.extend_from_slice(&1_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&end.to_be_bytes());
    cmap.extend_from_slice(&0xFFFF_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&start.to_be_bytes());
    cmap.extend_from_slice(&0xFFFF_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&1_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    debug_assert_eq!(cmap.len() - sub_at, 32);
    let mut font = Vec::new();
    font.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    font.extend_from_slice(&1_u16.to_be_bytes());
    font.extend_from_slice(&16_u16.to_be_bytes());
    font.extend_from_slice(&0_u16.to_be_bytes());
    font.extend_from_slice(&0_u16.to_be_bytes());
    font.extend_from_slice(b"cmap");
    font.extend_from_slice(&0_u32.to_be_bytes());
    font.extend_from_slice(&28_u32.to_be_bytes());
    font.extend_from_slice(&(cmap.len() as u32).to_be_bytes());
    debug_assert_eq!(font.len(), 28);
    font.extend_from_slice(&cmap);
    font
}
