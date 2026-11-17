//! OOXML pre-flight (10 §4.4.2): read only the ZIP central directory and
//! refuse bombs before any parser sees the file.
//!
//! Limits: at most 10,000 entries; declared uncompressed total at most 1 GiB;
//! one entry at most 256 MiB uncompressed and, above 1 MiB, a compression
//! ratio of at most 200; no `..`, absolute or drive-letter entry names (names
//! never touch the disk, but such a package is not one we want); no duplicate
//! names; no entry-level encryption. Nested archives are never expanded, so
//! there is nothing to limit there. Declarations can lie: [`DeclaredReader`]
//! enforces the declared size on the bytes actually read.
//!
//! No decompression happens here, which is why this module has no `zip`
//! dependency: the central directory is parsed by hand.

use std::io::{self, Read, Seek, SeekFrom};

pub const MAX_ENTRIES: u64 = 10_000;
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 1 << 30;
pub const MAX_ENTRY_UNCOMPRESSED: u64 = 256 << 20;
pub const MAX_RATIO: u64 = 200;
/// The ratio rule only applies to entries larger than this.
pub const RATIO_FLOOR: u64 = 1 << 20;
const MAX_CENTRAL_DIRECTORY: u64 = 64 << 20;

/// `DOCS_PARSE_*` codes of this module.
pub const CODES: &[&str] = &[
    "DOCS_PARSE_ZIP_BOMB",
    "DOCS_PARSE_CORRUPT",
    "DOCS_PARSE_ENCRYPTED",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub compressed: u64,
    pub uncompressed: u64,
    pub flags: u16,
    pub method: u16,
    pub offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Package {
    Docx,
    Xlsx,
    Pptx,
    /// A ZIP, but not an OOXML package.
    Other,
}

#[derive(Clone, Debug)]
pub struct ZipReport {
    pub entries: Vec<ZipEntry>,
    pub total_uncompressed: u64,
    pub package: Package,
    pub has_macros: bool,
    /// Largest declared `xl/worksheets/*.xml`, for the xlsx cell estimate.
    pub max_sheet_xml: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// `DOCS_PARSE_ZIP_BOMB`.
    Bomb(&'static str),
    /// `DOCS_PARSE_CORRUPT`.
    Corrupt(&'static str),
    /// `DOCS_PARSE_ENCRYPTED`: an entry is encrypted.
    Encrypted,
}

impl ZipError {
    pub fn code(&self) -> &'static str {
        match self {
            ZipError::Bomb(_) => "DOCS_PARSE_ZIP_BOMB",
            ZipError::Corrupt(_) => "DOCS_PARSE_CORRUPT",
            ZipError::Encrypted => "DOCS_PARSE_ENCRYPTED",
        }
    }
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipError::Bomb(w) | ZipError::Corrupt(w) => f.write_str(w),
            ZipError::Encrypted => f.write_str("zip entry is encrypted"),
        }
    }
}

impl std::error::Error for ZipError {}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn io_corrupt(_: io::Error) -> ZipError {
    ZipError::Corrupt("zip is truncated or unreadable")
}

/// Check a package on `r` (`len` bytes long).
pub fn check<R: Read + Seek>(r: &mut R, len: u64) -> Result<ZipReport, ZipError> {
    // End of central directory: the last 22..65557 bytes.
    let tail_len = len.min(65_557 + 22);
    if tail_len < 22 {
        return Err(ZipError::Corrupt("too short for a zip"));
    }
    r.seek(SeekFrom::Start(len - tail_len))
        .map_err(io_corrupt)?;
    let mut tail = vec![0u8; tail_len as usize];
    r.read_exact(&mut tail).map_err(io_corrupt)?;
    let eocd = (0..=tail.len() - 22)
        .rev()
        .find(|&i| tail[i..i + 4] == *b"PK\x05\x06")
        .ok_or(ZipError::Corrupt("no end of central directory"))?;
    let e = &tail[eocd..];
    let mut total = u64::from(u16le(e, 10).unwrap_or(0));
    let mut cd_size = u64::from(u32le(e, 12).unwrap_or(0));
    let mut cd_off = u64::from(u32le(e, 16).unwrap_or(0));
    if total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF {
        // Zip64: the locator sits right before the EOCD.
        let loc_at = (eocd as u64 + (len - tail_len))
            .checked_sub(20)
            .ok_or(ZipError::Corrupt("zip64 locator missing"))?;
        r.seek(SeekFrom::Start(loc_at)).map_err(io_corrupt)?;
        let mut loc = [0u8; 20];
        r.read_exact(&mut loc).map_err(io_corrupt)?;
        if loc[..4] != *b"PK\x06\x07" {
            return Err(ZipError::Corrupt("zip64 locator missing"));
        }
        let z64_at = u64le(&loc, 8).unwrap_or(u64::MAX);
        if z64_at >= len {
            return Err(ZipError::Corrupt("zip64 record out of range"));
        }
        r.seek(SeekFrom::Start(z64_at)).map_err(io_corrupt)?;
        let mut z = [0u8; 56];
        r.read_exact(&mut z).map_err(io_corrupt)?;
        if z[..4] != *b"PK\x06\x06" {
            return Err(ZipError::Corrupt("zip64 record missing"));
        }
        total = u64le(&z, 32).unwrap_or(0);
        cd_size = u64le(&z, 40).unwrap_or(0);
        cd_off = u64le(&z, 48).unwrap_or(0);
    }
    if total > MAX_ENTRIES {
        return Err(ZipError::Bomb("too many entries"));
    }
    if cd_size > MAX_CENTRAL_DIRECTORY || cd_off.checked_add(cd_size).is_none_or(|e| e > len) {
        return Err(ZipError::Corrupt("central directory out of range"));
    }
    r.seek(SeekFrom::Start(cd_off)).map_err(io_corrupt)?;
    let mut cd = vec![0u8; cd_size as usize];
    r.read_exact(&mut cd).map_err(io_corrupt)?;

    let mut entries = Vec::with_capacity(total as usize);
    let mut pos = 0usize;
    for _ in 0..total {
        let h = cd
            .get(pos..pos + 46)
            .ok_or(ZipError::Corrupt("central directory entry is cut"))?;
        if h[..4] != *b"PK\x01\x02" {
            return Err(ZipError::Corrupt("bad central directory signature"));
        }
        let flags = u16le(h, 8).unwrap_or(0);
        let method = u16le(h, 10).unwrap_or(0);
        let mut compressed = u64::from(u32le(h, 20).unwrap_or(0));
        let mut uncompressed = u64::from(u32le(h, 24).unwrap_or(0));
        let name_len = u16le(h, 28).unwrap_or(0) as usize;
        let extra_len = u16le(h, 30).unwrap_or(0) as usize;
        let comment_len = u16le(h, 32).unwrap_or(0) as usize;
        let mut offset = u64::from(u32le(h, 42).unwrap_or(0));
        let name_at = pos + 46;
        let name_bytes = cd
            .get(name_at..name_at + name_len)
            .ok_or(ZipError::Corrupt("entry name is cut"))?;
        let extra = cd
            .get(name_at + name_len..name_at + name_len + extra_len)
            .ok_or(ZipError::Corrupt("entry extra is cut"))?;
        pos = name_at + name_len + extra_len + comment_len;

        // zip64 extended information: fields appear only for 0xFFFFFFFF ones.
        let mut x = 0usize;
        while x + 4 <= extra.len() {
            let id = u16le(extra, x).unwrap_or(0);
            let sz = u16le(extra, x + 2).unwrap_or(0) as usize;
            let body = extra.get(x + 4..x + 4 + sz).unwrap_or(&[]);
            if id == 1 {
                let mut at = 0usize;
                let mut take = |slot: &mut u64, wanted: bool| {
                    if wanted && let Some(v) = u64le(body, at) {
                        *slot = v;
                        at += 8;
                    }
                };
                let (wu, wc, wo) = (
                    uncompressed == 0xFFFF_FFFF,
                    compressed == 0xFFFF_FFFF,
                    offset == 0xFFFF_FFFF,
                );
                take(&mut uncompressed, wu);
                take(&mut compressed, wc);
                take(&mut offset, wo);
            }
            x += 4 + sz;
        }

        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| ZipError::Corrupt("entry name is not UTF-8"))?
            .to_owned();
        if name.contains('\0') {
            return Err(ZipError::Corrupt("entry name contains NUL"));
        }
        let norm = name.replace('\\', "/");
        let drive = norm.as_bytes().get(1) == Some(&b':')
            && norm.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
        if norm.starts_with('/') || drive || norm.split('/').any(|seg| seg == "..") {
            return Err(ZipError::Corrupt("entry name escapes the package"));
        }
        if flags & 1 != 0 {
            return Err(ZipError::Encrypted);
        }
        if offset >= len {
            return Err(ZipError::Corrupt("entry offset is out of range"));
        }
        entries.push(ZipEntry {
            name,
            compressed,
            uncompressed,
            flags,
            method,
            offset,
        });
    }

    let mut seen = std::collections::HashSet::new();
    let mut total_uncompressed = 0u64;
    for e in &entries {
        if !seen.insert(e.name.as_str()) {
            return Err(ZipError::Corrupt("duplicate entry name"));
        }
        if e.name.ends_with('/') {
            continue;
        }
        if e.uncompressed > MAX_ENTRY_UNCOMPRESSED {
            return Err(ZipError::Bomb("entry is too large"));
        }
        if e.uncompressed > RATIO_FLOOR
            && (e.compressed == 0 || e.uncompressed / e.compressed > MAX_RATIO)
        {
            return Err(ZipError::Bomb("compression ratio is too high"));
        }
        total_uncompressed = total_uncompressed.saturating_add(e.uncompressed);
    }
    if total_uncompressed > MAX_TOTAL_UNCOMPRESSED {
        return Err(ZipError::Bomb("declared size is too large"));
    }

    let has = |n: &str| entries.iter().any(|e| e.name == n);
    let package = if !has("[Content_Types].xml") {
        Package::Other
    } else if has("word/document.xml") {
        Package::Docx
    } else if has("xl/workbook.xml") {
        Package::Xlsx
    } else if has("ppt/presentation.xml") {
        Package::Pptx
    } else {
        Package::Other
    };
    let has_macros = entries.iter().any(|e| e.name.ends_with("vbaProject.bin"));
    let max_sheet_xml = entries
        .iter()
        .filter(|e| e.name.starts_with("xl/worksheets/") && e.name.ends_with(".xml"))
        .map(|e| e.uncompressed)
        .max()
        .unwrap_or(0);
    Ok(ZipReport {
        entries,
        total_uncompressed,
        package,
        has_macros,
        max_sheet_xml,
    })
}

/// [`check`] on an in-memory file.
pub fn check_bytes(bytes: &[u8]) -> Result<ZipReport, ZipError> {
    check(&mut io::Cursor::new(bytes), bytes.len() as u64)
}

/// Wraps the stream of one decompressed entry and fails once it yields more
/// bytes than the central directory declared (declarations can be forged).
pub struct DeclaredReader<R> {
    inner: R,
    declared: u64,
    seen: u64,
}

impl<R: Read> DeclaredReader<R> {
    pub fn new(inner: R, declared: u64) -> Self {
        Self {
            inner,
            declared,
            seen: 0,
        }
    }
}

impl<R: Read> Read for DeclaredReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.seen += n as u64;
        if self.seen > self.declared {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry exceeds its declared size",
            ));
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with the given (name, flags, comp, uncomp) entries; the local
    /// data is a single filler byte per entry, enough for offset checks.
    fn zip(entries: &[(&str, u16, u32, u32)]) -> Vec<u8> {
        let mut out = vec![b'x'; 64];
        let mut cd = Vec::new();
        for (name, flags, comp, uncomp) in entries {
            cd.extend_from_slice(b"PK\x01\x02");
            cd.extend_from_slice(&[20, 0, 20, 0]);
            cd.extend_from_slice(&flags.to_le_bytes());
            cd.extend_from_slice(&8u16.to_le_bytes());
            cd.extend_from_slice(&[0; 4]); // time, date
            cd.extend_from_slice(&[0; 4]); // crc
            cd.extend_from_slice(&comp.to_le_bytes());
            cd.extend_from_slice(&uncomp.to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 6]); // extra, comment, disk
            cd.extend_from_slice(&[0; 6]); // attrs
            cd.extend_from_slice(&0u32.to_le_bytes()); // offset
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn plain_package_is_classified() {
        let z = zip(&[
            ("[Content_Types].xml", 0, 100, 400),
            ("word/document.xml", 0, 100, 900),
            ("word/vbaProject.bin", 0, 10, 10),
        ]);
        let r = check_bytes(&z).unwrap();
        assert_eq!(r.package, Package::Docx);
        assert!(r.has_macros);
        assert_eq!(r.total_uncompressed, 1310);
    }

    #[test]
    fn bombs_are_refused() {
        let big = 300 << 20;
        let z = zip(&[("a.xml", 0, 1 << 20, big)]);
        assert_eq!(check_bytes(&z).unwrap_err().code(), "DOCS_PARSE_ZIP_BOMB");
        // Ratio above 200 on an entry over 1 MiB.
        let z = zip(&[("a.xml", 0, 4_000, 4 << 20)]);
        assert!(matches!(check_bytes(&z), Err(ZipError::Bomb(_))));
        // The same ratio on a small entry is tolerated.
        let z = zip(&[("a.xml", 0, 10, 4_000)]);
        assert!(check_bytes(&z).is_ok());
        // Total over 1 GiB from many individually legal entries.
        let names: Vec<String> = (0..5).map(|i| format!("p{i}.xml")).collect();
        let entries: Vec<(&str, u16, u32, u32)> = names
            .iter()
            .map(|n| (n.as_str(), 0u16, 2 << 20, 250 << 20))
            .collect();
        assert!(matches!(
            check_bytes(&zip(&entries)),
            Err(ZipError::Bomb("declared size is too large"))
        ));
    }

    #[test]
    fn too_many_entries() {
        let names: Vec<String> = (0..10_001).map(|i| format!("e{i}")).collect();
        let entries: Vec<(&str, u16, u32, u32)> =
            names.iter().map(|n| (n.as_str(), 0u16, 1, 1)).collect();
        assert_eq!(
            check_bytes(&zip(&entries)).unwrap_err(),
            ZipError::Bomb("too many entries")
        );
    }

    #[test]
    fn hostile_names_and_encryption() {
        for bad in ["../x.xml", "/etc/passwd", "a/../../b", "C:/x", "a\\..\\b"] {
            let z = zip(&[(bad, 0, 1, 1)]);
            assert_eq!(check_bytes(&z).unwrap_err().code(), "DOCS_PARSE_CORRUPT");
        }
        let z = zip(&[("a.xml", 1, 10, 10)]);
        assert_eq!(check_bytes(&z).unwrap_err(), ZipError::Encrypted);
        let z = zip(&[("a.xml", 0, 1, 1), ("a.xml", 0, 1, 1)]);
        assert!(matches!(check_bytes(&z), Err(ZipError::Corrupt(_))));
    }

    #[test]
    fn garbage_is_corrupt() {
        assert!(matches!(
            check_bytes(b"PK\x03\x04"),
            Err(ZipError::Corrupt(_))
        ));
        assert!(matches!(
            check_bytes(&[b'x'; 300]),
            Err(ZipError::Corrupt(_))
        ));
        let mut z = zip(&[("a.xml", 0, 1, 1)]);
        let n = z.len();
        z[n - 6] = 0xFF; // central directory offset past the end
        assert!(matches!(check_bytes(&z), Err(ZipError::Corrupt(_))));
    }

    #[test]
    fn declared_reader_stops_forged_sizes() {
        let mut ok = DeclaredReader::new(&[0u8; 10][..], 10);
        let mut v = Vec::new();
        ok.read_to_end(&mut v).unwrap();
        let mut bad = DeclaredReader::new(&[0u8; 11][..], 10);
        assert!(bad.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn codes_are_in_the_contract() {
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
