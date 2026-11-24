//! Byte offset to line number and Unicode-scalar offset mapping (10 §3.5).

use oo_core::docir::Anchor;

pub struct LineIndex<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
    /// Scalar offset of each line start.
    char_prefix: Vec<u64>,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        let mut char_prefix = vec![0u64];
        let mut chars = 0u64;
        for (i, c) in text.char_indices() {
            chars += 1;
            if c == '\n' {
                line_starts.push(i + 1);
                char_prefix.push(chars);
            }
        }
        Self {
            text,
            line_starts,
            char_prefix,
        }
    }

    fn line_idx(&self, byte: usize) -> usize {
        self.line_starts
            .partition_point(|s| *s <= byte)
            .saturating_sub(1)
    }

    /// 1-based line containing `byte` (clamped to the text).
    pub fn line_of(&self, byte: usize) -> u32 {
        (self.line_idx(byte.min(self.text.len())) + 1) as u32
    }

    /// Scalar offset of a byte offset on a char boundary (clamped).
    pub fn char_of(&self, byte: usize) -> u64 {
        let byte = byte.min(self.text.len());
        let idx = self.line_idx(byte);
        let start = self.line_starts[idx];
        let within = self
            .text
            .get(start..byte)
            .map_or(0, |s| s.chars().count() as u64);
        self.char_prefix[idx] + within
    }

    /// Text anchor for the byte range `[start, end)`. Trailing line breaks
    /// are not part of the block, so a block never claims the next line.
    pub fn anchor(&self, start: usize, end: usize) -> Anchor {
        let start = start.min(self.text.len());
        let mut end = end.clamp(start, self.text.len());
        let bytes = self.text.as_bytes();
        while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
            end -= 1;
        }
        let last = if end > start { end - 1 } else { start };
        Anchor::Text {
            line_start: self.line_of(start),
            line_end: self.line_of(last),
            char_start: self.char_of(start),
            char_end: self.char_of(end),
        }
    }
}
