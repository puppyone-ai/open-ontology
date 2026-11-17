//! Content sniffing (10 §4.4.1): the format comes from the bytes, never from
//! the extension. The extension only separates the text formats (md, txt,
//! csv) from each other.
//!
//! Only the head of the file is looked at. Heuristics that need the whole
//! container (OOXML `[Content_Types].xml`, PDF text-layer ratio) belong to the
//! zip guard and the sidecar probe; here an OOXML package is recognised by the
//! part names its first local headers carry in clear text. [`sniff_reader`]
//! looks at the whole file (central directory, PDF trailer, OLE2 streams).

use std::io::{self, Read, Seek, SeekFrom};

use super::zipguard::{self, Package, ZipError};

/// `_oo_docs.blob.format` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Format {
    /// Markdown text.
    Md,
    /// Plain text.
    Txt,
    /// Comma separated text.
    Csv,
    /// WordprocessingML package.
    Docx,
    /// SpreadsheetML package.
    Xlsx,
    /// PresentationML package.
    Pptx,
    /// PDF with a text layer (the probe may later say `pdf_scanned`).
    Pdf,
    /// PDF without enough text.
    PdfScanned,
    /// Raster image.
    Image,
    /// OLE2 container: .doc, .xls, .ppt.
    LegacyOffice,
    /// ZIP that is not an OOXML package.
    Zip,
    /// Anything else, including audio and video containers.
    Unknown,
}

impl Format {
    /// Database text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Md => "md",
            Self::Txt => "txt",
            Self::Csv => "csv",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
            Self::PdfScanned => "pdf_scanned",
            Self::Image => "image",
            Self::LegacyOffice => "legacy_office",
            Self::Zip => "zip",
            Self::Unknown => "unknown",
        }
    }
}

/// What the file may do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Hand to a parser.
    Parseable,
    /// `unsupported` with this `DOCS_PARSE_*` code.
    Unsupported(&'static str),
    /// `encrypted` (`DOCS_PARSE_ENCRYPTED`).
    Encrypted,
}

/// Sniff result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sniffed {
    /// Detected format.
    pub format: Format,
    /// Routing decision.
    pub verdict: Verdict,
    /// OOXML package with a VBA project.
    pub has_macros: bool,
}

/// `DOCS_PARSE_*` codes this module can return, for the contract test.
pub const CODES: &[&str] = &[
    "DOCS_PARSE_UNSUPPORTED_FORMAT",
    "DOCS_PARSE_LEGACY_OFFICE",
    "DOCS_PARSE_ENCODING",
    "DOCS_PARSE_ENCRYPTED",
    "DOCS_PARSE_IMAGE_OCR_M2",
    "DOCS_PARSE_ZIP_BOMB",
    "DOCS_PARSE_CORRUPT",
];

fn find(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Extension of the last path segment, lowercase, without the dot.
pub fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Window and overlap of the OLE2 name scan.
const OLE_SCAN_CHUNK: usize = 1 << 20;
/// How much of an OLE2 file is scanned for encryption stream names.
const OLE_SCAN_MAX: u64 = 32 << 20;
const HEAD_BYTES: u64 = 64 * 1024;

/// Sniff a whole file, not just its head. Adds what needs more than the first
/// bytes: the ZIP central directory (package type from entry names, macros,
/// the zip guard verdicts), `/Encrypt` in a PDF head or tail, and the
/// `EncryptedPackage` / `EncryptionInfo` stream names anywhere in the first
/// 32 MiB of an OLE2 file.
pub fn sniff_reader<R: Read + Seek>(r: &mut R, len: u64, ext: &str) -> io::Result<Sniffed> {
    let head_len = len.min(HEAD_BYTES) as usize;
    let mut head = vec![0u8; head_len];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    let mut out = sniff(&head, len, ext);
    let tail = |r: &mut R| -> io::Result<Vec<u8>> {
        let n = len.min(HEAD_BYTES);
        r.seek(SeekFrom::Start(len - n))?;
        let mut t = vec![0u8; n as usize];
        r.read_exact(&mut t)?;
        Ok(t)
    };
    match out.format {
        Format::Pdf => {
            if find(&head, b"/Encrypt") || find(&tail(r)?, b"/Encrypt") {
                out.verdict = Verdict::Encrypted;
            }
        }
        Format::Docx | Format::Xlsx | Format::Pptx | Format::Zip => {
            // A head that starts with PK is a zip, whatever the part names.
            match zipguard::check(r, len) {
                Ok(report) => {
                    let (format, verdict) = match report.package {
                        Package::Docx => (Format::Docx, Verdict::Parseable),
                        Package::Xlsx => (Format::Xlsx, Verdict::Parseable),
                        Package::Pptx => (Format::Pptx, Verdict::Parseable),
                        Package::Other => (
                            Format::Zip,
                            Verdict::Unsupported("DOCS_PARSE_UNSUPPORTED_FORMAT"),
                        ),
                    };
                    out = Sniffed {
                        format,
                        verdict,
                        has_macros: report.has_macros,
                    };
                }
                Err(ZipError::Encrypted) => out.verdict = Verdict::Encrypted,
                Err(e) => out.verdict = Verdict::Unsupported(e.code()),
            }
        }
        Format::LegacyOffice => {
            let names = [utf16le("EncryptedPackage"), utf16le("EncryptionInfo")];
            let keep = names.iter().map(Vec::len).max().unwrap_or(0);
            let mut pos = 0u64;
            let mut carry: Vec<u8> = Vec::new();
            r.seek(SeekFrom::Start(0))?;
            while pos < len.min(OLE_SCAN_MAX) {
                let mut chunk = vec![0u8; OLE_SCAN_CHUNK.min((len - pos) as usize)];
                r.read_exact(&mut chunk)?;
                pos += chunk.len() as u64;
                let mut window = std::mem::take(&mut carry);
                window.extend_from_slice(&chunk);
                if names.iter().any(|n| find(&window, n)) {
                    out.verdict = Verdict::Encrypted;
                    break;
                }
                carry = window[window.len().saturating_sub(keep)..].to_vec();
            }
        }
        _ => {}
    }
    Ok(out)
}

/// Classify `head` (the first bytes of the file, at most 64 KiB is useful).
/// `file_len` tells whether `head` is the whole file, which matters for a
/// multi-byte sequence cut at the end of the head.
pub fn sniff(head: &[u8], file_len: u64, ext: &str) -> Sniffed {
    let make = |format, verdict| Sniffed {
        format,
        verdict,
        has_macros: false,
    };
    if head.starts_with(b"%PDF-") {
        return make(Format::Pdf, Verdict::Parseable);
    }
    if head.starts_with(b"PK\x03\x04") {
        let (format, macros) = if find(head, b"word/") {
            (Format::Docx, find(head, b"vbaProject.bin"))
        } else if find(head, b"xl/") {
            (Format::Xlsx, find(head, b"vbaProject.bin"))
        } else if find(head, b"ppt/") {
            (Format::Pptx, find(head, b"vbaProject.bin"))
        } else {
            return make(
                Format::Zip,
                Verdict::Unsupported("DOCS_PARSE_UNSUPPORTED_FORMAT"),
            );
        };
        return Sniffed {
            format,
            verdict: Verdict::Parseable,
            has_macros: macros,
        };
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        if find(head, &utf16le("EncryptionInfo")) {
            return make(Format::LegacyOffice, Verdict::Encrypted);
        }
        return make(
            Format::LegacyOffice,
            Verdict::Unsupported("DOCS_PARSE_LEGACY_OFFICE"),
        );
    }
    // A UTF-16 BOM shares its first byte with an MPEG sync word: BOM wins.
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return make(text_format(ext), Verdict::Parseable);
    }
    if is_image(head) {
        return make(
            Format::Image,
            Verdict::Unsupported("DOCS_PARSE_IMAGE_OCR_M2"),
        );
    }
    if is_media(head) {
        // The contract has no DOCS_PARSE_MEDIA_L6 code (10 §4.4.1 names it,
        // errors.v1.json does not): report the generic one.
        return make(
            Format::Unknown,
            Verdict::Unsupported("DOCS_PARSE_UNSUPPORTED_FORMAT"),
        );
    }
    match text_kind(head, file_len) {
        TextKind::Binary => make(
            Format::Unknown,
            Verdict::Unsupported("DOCS_PARSE_UNSUPPORTED_FORMAT"),
        ),
        TextKind::BadEncoding => make(
            text_format(ext),
            Verdict::Unsupported("DOCS_PARSE_ENCODING"),
        ),
        TextKind::Text => make(text_format(ext), Verdict::Parseable),
    }
}

fn text_format(ext: &str) -> Format {
    match ext {
        "md" | "markdown" | "mdx" => Format::Md,
        "csv" => Format::Csv,
        _ => Format::Txt,
    }
}

fn is_image(h: &[u8]) -> bool {
    h.starts_with(&[0x89, b'P', b'N', b'G'])
        || h.starts_with(&[0xFF, 0xD8, 0xFF])
        || h.starts_with(b"GIF87a")
        || h.starts_with(b"GIF89a")
        || (h.len() >= 12 && &h[0..4] == b"RIFF" && &h[8..12] == b"WEBP")
        || h.starts_with(b"II*\0")
        || h.starts_with(b"MM\0*")
}

fn is_media(h: &[u8]) -> bool {
    (h.len() >= 8 && &h[4..8] == b"ftyp")
        || (h.len() >= 12 && &h[0..4] == b"RIFF" && &h[8..12] == b"WAVE")
        || h.starts_with(b"OggS")
        || h.starts_with(b"fLaC")
        || h.starts_with(b"ID3")
        || h.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
        || (h.len() >= 2 && h[0] == 0xFF && (h[1] & 0xE0) == 0xE0)
}

enum TextKind {
    Text,
    BadEncoding,
    Binary,
}

fn text_kind(head: &[u8], file_len: u64) -> TextKind {
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return TextKind::Text;
    }
    let body = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    if body.contains(&0) {
        return TextKind::Binary;
    }
    let whole = head.len() as u64 >= file_len;
    match std::str::from_utf8(body) {
        Ok(_) => TextKind::Text,
        // A character cut by the end of the head is not a defect.
        Err(e) if !whole && e.error_len().is_none() => TextKind::Text,
        Err(_) => TextKind::BadEncoding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(head: &[u8], ext: &str) -> Sniffed {
        sniff(head, head.len() as u64, ext)
    }

    #[test]
    fn text_formats_follow_the_extension_only_between_themselves() {
        assert_eq!(s(b"# hi\n", "md").format, Format::Md);
        assert_eq!(s(b"# hi\n", "txt").format, Format::Txt);
        assert_eq!(s(b"a,b\n1,2\n", "csv").format, Format::Csv);
        assert_eq!(s(b"", "md").verdict, Verdict::Parseable);
        // The extension never turns binary into text.
        let r = s(b"\x00\x01\x02", "md");
        assert_eq!(
            r.verdict,
            Verdict::Unsupported("DOCS_PARSE_UNSUPPORTED_FORMAT")
        );
    }

    #[test]
    fn encodings() {
        assert_eq!(s(b"\xEF\xBB\xBFhello", "txt").verdict, Verdict::Parseable);
        assert_eq!(s(b"\xFF\xFEh\x00i\x00", "txt").verdict, Verdict::Parseable);
        // GB18030 bytes are not valid UTF-8 and there is no decoder.
        let r = s(&[0xC4, 0xE3, 0xBA, 0xC3], "txt");
        assert_eq!(r.verdict, Verdict::Unsupported("DOCS_PARSE_ENCODING"));
        // A multi-byte character cut by the head limit is fine.
        let cut = &"héllo".as_bytes()[..2];
        assert_eq!(sniff(cut, 100, "txt").verdict, Verdict::Parseable);
        assert_eq!(
            sniff(cut, 2, "txt").verdict,
            Verdict::Unsupported("DOCS_PARSE_ENCODING")
        );
    }

    #[test]
    fn containers() {
        assert_eq!(s(b"%PDF-1.7\n", "pdf").format, Format::Pdf);
        let mut zip = b"PK\x03\x04....[Content_Types].xml....word/document.xml".to_vec();
        assert_eq!(s(&zip, "docx").format, Format::Docx);
        assert!(!s(&zip, "docx").has_macros);
        zip.extend_from_slice(b"word/vbaProject.bin");
        assert!(s(&zip, "docm").has_macros);
        assert_eq!(s(b"PK\x03\x04xl/workbook.xml", "").format, Format::Xlsx);
        assert_eq!(
            s(b"PK\x03\x04ppt/presentation.xml", "").format,
            Format::Pptx
        );
        let plain = s(b"PK\x03\x04hello.txt", "zip");
        assert_eq!(plain.format, Format::Zip);
        assert!(matches!(plain.verdict, Verdict::Unsupported(_)));
        let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        assert_eq!(
            s(&ole, "doc").verdict,
            Verdict::Unsupported("DOCS_PARSE_LEGACY_OFFICE")
        );
        let mut enc = ole.to_vec();
        enc.extend(utf16le("EncryptionInfo"));
        assert_eq!(s(&enc, "docx").verdict, Verdict::Encrypted);
    }

    #[test]
    fn images_and_media_are_unsupported() {
        assert_eq!(
            s(&[0x89, b'P', b'N', b'G', 0, 0], "png").format,
            Format::Image
        );
        assert_eq!(
            s(b"\xFF\xD8\xFF\xE0", "jpg").verdict,
            Verdict::Unsupported("DOCS_PARSE_IMAGE_OCR_M2")
        );
        assert_eq!(
            s(b"\x00\x00\x00\x18ftypmp42", "mp4").format,
            Format::Unknown
        );
        assert_eq!(
            s(b"RIFF\x00\x00\x00\x00WAVEfmt ", "wav").format,
            Format::Unknown
        );
        assert_eq!(s(b"OggS\x00", "ogg").format, Format::Unknown);
    }

    #[test]
    fn extension_helper() {
        assert_eq!(extension("a/b/Notes.MD"), "md");
        assert_eq!(extension("a.b/noext"), "");
        assert_eq!(extension(".hidden"), "");
    }

    #[test]
    fn every_code_is_in_the_contract() {
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../contracts/docs-api/errors.v1.json"),
        )
        .unwrap();
        for code in CODES {
            assert!(raw.contains(&format!("\"{code}\"")), "{code}");
        }
    }
}
