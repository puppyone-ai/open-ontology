use std::io::{BufReader, Read, Write};

use sha2::{Digest, Sha256};

use crate::digest::{HashWriter, hex};
use crate::error::{Result, err};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field<'a> {
    Null,
    Raw(&'a str),
    Text(&'a str),
}

pub struct CsvOut<W: Write> {
    inner: HashWriter<W>,
    columns: usize,
}

impl<W: Write> CsvOut<W> {
    pub fn new(inner: W, header: &[&str]) -> Result<Self> {
        let mut writer = Self {
            inner: HashWriter::new(inner),
            columns: header.len(),
        };
        writer.write_line(
            &header
                .iter()
                .map(|name| Field::Raw(name))
                .collect::<Vec<_>>(),
        )?;
        Ok(writer)
    }

    pub fn write_row(&mut self, fields: &[Field<'_>]) -> Result<()> {
        if fields.len() != self.columns {
            return Err(err(format!(
                "csv row has {} fields, header has {}",
                fields.len(),
                self.columns
            )));
        }
        self.write_line(fields)
    }

    pub fn finish(self) -> Result<(W, u64, String)> {
        self.inner.finish()
    }

    fn write_line(&mut self, fields: &[Field<'_>]) -> Result<()> {
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                self.inner.write_all(b",")?;
            }
            match field {
                Field::Null => self.inner.write_all(b"\\N")?,
                Field::Raw(value) => {
                    if !raw_ok(value) {
                        return Err(err(format!("refusing non-lexical raw field {value}")));
                    }
                    self.inner.write_all(value.as_bytes())?;
                }
                Field::Text(value) => write_quoted(&mut self.inner, value)?,
            }
        }
        self.inner.write_all(b"\n")?;
        Ok(())
    }
}

fn raw_ok(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn write_quoted(out: &mut impl Write, value: &str) -> Result<()> {
    if value.chars().any(|ch| ch == '\r' || ch == '\0') {
        return Err(err("csv text refuses CR and NUL"));
    }
    out.write_all(b"\"")?;
    for ch in value.chars() {
        if ch == '"' {
            out.write_all(b"\"\"")?;
        } else {
            let mut buf = [0u8; 4];
            out.write_all(ch.encode_utf8(&mut buf).as_bytes())?;
        }
    }
    out.write_all(b"\"")?;
    Ok(())
}

pub fn mysql_string(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

pub struct CsvScan {
    pub rows: u64,
    pub header: Vec<String>,
}

enum ScanState {
    Field,
    Quote,
    QuoteEnd,
}

pub fn for_each_row<R, F>(reader: R, mut on_row: F) -> Result<CsvScan>
where
    R: Read,
    F: FnMut(&[Option<String>]) -> Result<()>,
{
    let mut input = BufReader::new(reader);
    let mut byte = [0u8; 1];
    let mut field: Vec<u8> = Vec::new();
    let mut row: Vec<Option<String>> = Vec::new();
    let mut header: Vec<String> = Vec::new();
    let mut rows = 0u64;
    let mut state = ScanState::Field;
    let mut at_start = true;
    let mut quoted = false;

    let push_field =
        |field: &mut Vec<u8>, row: &mut Vec<Option<String>>, quoted: bool| -> Result<()> {
            if !quoted && field.as_slice() == b"\\N" {
                row.push(None);
            } else {
                let text = String::from_utf8(std::mem::take(field))
                    .map_err(|_| err("csv field is not utf-8"))?;
                row.push(Some(text));
            }
            field.clear();
            Ok(())
        };

    let end_row = |row: &mut Vec<Option<String>>,
                   header: &mut Vec<String>,
                   rows: &mut u64,
                   on_row: &mut F|
     -> Result<()> {
        if header.is_empty() {
            *header = row
                .iter()
                .map(|cell| cell.clone().unwrap_or_default())
                .collect();
        } else {
            if row.len() != header.len() {
                return Err(err("csv row width drift"));
            }
            on_row(row)?;
            *rows += 1;
        }
        row.clear();
        Ok(())
    };

    loop {
        let n = input.read(&mut byte)?;
        if n == 0 {
            match state {
                ScanState::Quote => return Err(err("csv ended inside quotes")),
                ScanState::QuoteEnd => {
                    push_field(&mut field, &mut row, true)?;
                    end_row(&mut row, &mut header, &mut rows, &mut on_row)?;
                }
                ScanState::Field => {
                    if !at_start || !field.is_empty() || !row.is_empty() {
                        push_field(&mut field, &mut row, quoted)?;
                        end_row(&mut row, &mut header, &mut rows, &mut on_row)?;
                    }
                }
            }
            break;
        }
        let b = byte[0];
        if b == b'\r' || b == 0 {
            return Err(err("csv contains CR or NUL"));
        }
        match state {
            ScanState::Field => {
                if at_start && b == b'"' {
                    state = ScanState::Quote;
                    quoted = true;
                    at_start = false;
                } else if b == b',' {
                    push_field(&mut field, &mut row, quoted)?;
                    quoted = false;
                    at_start = true;
                } else if b == b'\n' {
                    push_field(&mut field, &mut row, quoted)?;
                    quoted = false;
                    at_start = true;
                    end_row(&mut row, &mut header, &mut rows, &mut on_row)?;
                } else if b == b'"' {
                    return Err(err("csv quote in an unquoted field"));
                } else {
                    field.push(b);
                    at_start = false;
                }
            }
            ScanState::Quote => {
                if b == b'"' {
                    state = ScanState::QuoteEnd;
                } else {
                    field.push(b);
                }
            }
            ScanState::QuoteEnd => {
                if b == b'"' {
                    field.push(b'"');
                    state = ScanState::Quote;
                } else if b == b',' {
                    push_field(&mut field, &mut row, true)?;
                    quoted = false;
                    at_start = true;
                    state = ScanState::Field;
                } else if b == b'\n' {
                    push_field(&mut field, &mut row, true)?;
                    quoted = false;
                    at_start = true;
                    state = ScanState::Field;
                    end_row(&mut row, &mut header, &mut rows, &mut on_row)?;
                } else {
                    return Err(err("csv quote was not closed or doubled"));
                }
            }
        }
    }
    if header.is_empty() {
        return Err(err("csv missing header"));
    }
    Ok(CsvScan { rows, header })
}

pub fn hash_reader<R: Read>(mut reader: R) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        len += u64::try_from(n).map_err(|_| err("stream longer than u64"))?;
    }
    Ok((len, hex(hash.finalize())))
}

#[cfg(test)]
mod tests {
    use super::{CsvOut, Field, for_each_row, mysql_string};
    use std::io::Cursor;

    #[test]
    fn quotes_comma_newline_and_null() {
        let mut buf = Vec::new();
        {
            let mut csv = CsvOut::new(&mut buf, &["a", "b"]).unwrap();
            csv.write_row(&[Field::Text("x,\"y\"\nz"), Field::Null])
                .unwrap();
            csv.write_row(&[Field::Raw("10.50"), Field::Text("plain")])
                .unwrap();
            csv.finish().unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text, "a,b\n\"x,\"\"y\"\"\nz\",\\N\n10.50,\"plain\"\n");
        assert!(!text.contains('\r'));
        let mut seen = Vec::new();
        let scan = for_each_row(Cursor::new(text.as_bytes()), |row| {
            seen.push(row.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(scan.rows, 2);
        assert_eq!(seen[0][0].as_deref(), Some("x,\"y\"\nz"));
        assert_eq!(seen[0][1], None);
        assert_eq!(seen[1][0].as_deref(), Some("10.50"));
    }

    #[test]
    fn mysql_literal_escapes() {
        assert_eq!(mysql_string("a'b\\c\n"), "'a''b\\\\c\\n'");
    }

    #[test]
    fn rejects_cr() {
        let mut buf = Vec::new();
        let mut csv = CsvOut::new(&mut buf, &["a"]).unwrap();
        let err = csv.write_row(&[Field::Text("a\rb")]).unwrap_err();
        assert!(err.to_string().contains("CR"));
    }
}
