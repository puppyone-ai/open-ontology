//! Arrow IPC file for non-null or nullable utf8 columns.
//!
//! The framing matches the native loader's utf8 fixture file: `ARROW1`, one
//! schema message, one record batch, then the footer. This is not a general
//! Arrow writer.

use sha2::{Digest, Sha256};

enum Cell {
    Text(String),
    Null,
}

pub struct Utf8Column {
    pub name: String,
    pub nullable: bool,
}

pub enum Utf8Cell {
    Text(String),
    Null,
}

pub struct ArrowFile {
    pub bytes: Vec<u8>,
    pub schema_sha256: String,
    pub file_sha256: String,
}

pub fn write_utf8_file(
    columns: &[Utf8Column],
    rows: &[Vec<Utf8Cell>],
) -> Result<ArrowFile, &'static str> {
    if columns.is_empty() || rows.is_empty() || rows.iter().any(|row| row.len() != columns.len()) {
        return Err("empty");
    }
    let columns: Vec<Column> = columns
        .iter()
        .map(|column| Column {
            name: column.name.clone(),
            nullable: column.nullable,
        })
        .collect();
    let rows: Vec<Vec<Cell>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    Utf8Cell::Text(text) => Cell::Text(text.clone()),
                    Utf8Cell::Null => Cell::Null,
                })
                .collect()
        })
        .collect();
    let body = encode_body(&columns, &rows)?;
    let schema = schema_meta(&columns)?;
    let batch = batch_meta(&columns, &rows, body.len())?;
    let bytes = wrap_file(&schema, &batch, &body);
    Ok(ArrowFile {
        schema_sha256: hex_sha(&schema),
        file_sha256: hex_sha(&bytes),
        bytes,
    })
}

pub fn hex_sha(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

struct Column {
    name: String,
    nullable: bool,
}

struct Buf {
    data: Vec<u8>,
}

impl Buf {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn align(&mut self, n: usize) {
        while !self.len().is_multiple_of(n) {
            self.data.push(0);
        }
    }

    fn u8(&mut self, value: u8) {
        self.data.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn patch(&mut self, at: usize, target: usize) -> Result<(), &'static str> {
        let rel =
            u32::try_from(target.checked_sub(at).ok_or("bad_type")?).map_err(|_| "bad_type")?;
        self.data[at..at + 4].copy_from_slice(&rel.to_le_bytes());
        Ok(())
    }
}

fn schema_meta(columns: &[Column]) -> Result<Vec<u8>, &'static str> {
    let mut buf = Buf::new();
    buf.u32(0);
    buf.align(8);
    if buf.len() % 8 != 4 {
        buf.u32(0);
    }
    let vtable = buf.len();
    buf.u16(12);
    buf.u16(24);
    buf.u16(0);
    buf.u16(4);
    buf.u16(8);
    buf.u16(16);
    let table = buf.len();
    buf.i32(i32::try_from(table - vtable).map_err(|_| "bad_type")?);
    buf.u8(1);
    buf.u8(0);
    buf.u8(0);
    buf.u8(0);
    let header_slot = buf.len();
    buf.u32(0);
    buf.u32(0);
    buf.i64(0);

    let schema_vtable = buf.len();
    buf.u16(8);
    buf.u16(8);
    buf.u16(0);
    buf.u16(4);
    let schema_table = buf.len();
    buf.i32(i32::try_from(schema_table - schema_vtable).map_err(|_| "bad_type")?);
    let fields_slot = buf.len();
    buf.u32(0);

    buf.align(4);
    let vector_at = buf.len();
    buf.u32(u32::try_from(columns.len()).map_err(|_| "bad_type")?);
    let mut field_slots = Vec::new();
    for _ in columns {
        field_slots.push(buf.len());
        buf.u32(0);
    }
    buf.patch(fields_slot, vector_at)?;

    let mut name_slots = Vec::new();
    let mut type_slots = Vec::new();
    let mut field_tables = Vec::new();
    for column in columns {
        buf.align(4);
        let field_vtable = buf.len();
        buf.u16(12);
        buf.u16(16);
        buf.u16(4);
        buf.u16(8);
        buf.u16(9);
        buf.u16(12);
        let field_table = buf.len();
        buf.i32(i32::try_from(field_table - field_vtable).map_err(|_| "bad_type")?);
        name_slots.push(buf.len());
        buf.u32(0);
        buf.u8(u8::from(column.nullable));
        buf.u8(5);
        buf.u8(0);
        buf.u8(0);
        type_slots.push(buf.len());
        buf.u32(0);
        field_tables.push(field_table);
    }
    for (slot, start) in field_slots.iter().zip(&field_tables) {
        buf.patch(*slot, *start)?;
    }
    for (index, column) in columns.iter().enumerate() {
        buf.align(4);
        let string_at = buf.len();
        buf.u32(u32::try_from(column.name.len()).map_err(|_| "bad_type")?);
        buf.data.extend(column.name.as_bytes());
        buf.patch(name_slots[index], string_at)?;
        buf.align(4);
        let type_vtable = buf.len();
        buf.u16(4);
        buf.u16(4);
        let type_table = buf.len();
        buf.i32(i32::try_from(type_table - type_vtable).map_err(|_| "bad_type")?);
        buf.patch(type_slots[index], type_table)?;
    }
    buf.patch(header_slot, schema_table)?;
    buf.patch(0, table)?;
    Ok(buf.data)
}

fn batch_meta(
    columns: &[Column],
    rows: &[Vec<Cell>],
    body_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut buf = Buf::new();
    buf.u32(0);
    buf.align(8);
    if buf.len() % 8 != 4 {
        buf.u32(0);
    }
    let vtable = buf.len();
    buf.u16(12);
    buf.u16(24);
    buf.u16(0);
    buf.u16(4);
    buf.u16(8);
    buf.u16(16);
    let table = buf.len();
    buf.i32(i32::try_from(table - vtable).map_err(|_| "bad_type")?);
    buf.u8(3);
    buf.u8(0);
    buf.u8(0);
    buf.u8(0);
    let header_slot = buf.len();
    buf.u32(0);
    buf.u32(0);
    buf.i64(i64::try_from(body_len).map_err(|_| "read_budget")?);

    buf.align(2);
    let batch_vtable = buf.len();
    buf.u16(10);
    buf.u16(24);
    buf.u16(8);
    buf.u16(16);
    buf.u16(20);
    while !buf.len().is_multiple_of(8) {
        buf.u8(0);
    }
    let batch_table = buf.len();
    buf.i32(i32::try_from(batch_table - batch_vtable).map_err(|_| "bad_type")?);
    buf.u32(0);
    buf.i64(i64::try_from(rows.len()).map_err(|_| "bad_type")?);
    let nodes_slot = buf.len();
    buf.u32(0);
    let buffers_slot = buf.len();
    buf.u32(0);

    while buf.len() % 8 != 4 {
        buf.u8(0);
    }
    let nodes_at = buf.len();
    buf.u32(u32::try_from(columns.len()).map_err(|_| "bad_type")?);
    for (index, _column) in columns.iter().enumerate() {
        let nulls = null_count(columns, index, rows)?;
        buf.i64(i64::try_from(rows.len()).map_err(|_| "bad_type")?);
        buf.i64(nulls);
    }
    while buf.len() % 8 != 4 {
        buf.u8(0);
    }
    let buffers_at = buf.len();
    buf.u32(u32::try_from(columns.len() * 3).map_err(|_| "bad_type")?);
    let specs = buffer_specs(columns, rows)?;
    for (offset, length) in specs {
        buf.i64(offset);
        buf.i64(length);
    }
    buf.patch(nodes_slot, nodes_at)?;
    buf.patch(buffers_slot, buffers_at)?;
    buf.patch(header_slot, batch_table)?;
    buf.patch(0, table)?;
    Ok(buf.data)
}

fn null_count(columns: &[Column], index: usize, rows: &[Vec<Cell>]) -> Result<i64, &'static str> {
    let column = columns.get(index).ok_or("bad_type")?;
    let mut nulls = 0i64;
    for row in rows {
        if matches!(row.get(index), Some(Cell::Null)) {
            if !column.nullable {
                return Err("bad_type");
            }
            nulls += 1;
        }
    }
    Ok(nulls)
}

fn encode_body(columns: &[Column], rows: &[Vec<Cell>]) -> Result<Vec<u8>, &'static str> {
    let mut body = Vec::new();
    for (index, _column) in columns.iter().enumerate() {
        let nulls = rows
            .iter()
            .filter(|row| matches!(row[index], Cell::Null))
            .count();
        if nulls > 0 {
            align_body(&mut body, 8);
            let mut bits = vec![0u8; rows.len().div_ceil(8)];
            for (row_index, row) in rows.iter().enumerate() {
                if !matches!(row[index], Cell::Null) {
                    bits[row_index / 8] |= 1 << (row_index % 8);
                }
            }
            body.extend(bits);
        }
        align_body(&mut body, 8);
        let mut data: Vec<u8> = Vec::new();
        let mut offsets = Vec::with_capacity(rows.len() + 1);
        offsets.push(0i32);
        for row in rows {
            if let Cell::Text(text) = &row[index] {
                data.extend(text.as_bytes());
            }
            offsets.push(i32::try_from(data.len()).map_err(|_| "read_budget")?);
        }
        for offset in offsets {
            body.extend_from_slice(&offset.to_le_bytes());
        }
        align_body(&mut body, 8);
        body.extend(data);
    }
    Ok(body)
}

fn buffer_specs(columns: &[Column], rows: &[Vec<Cell>]) -> Result<Vec<(i64, i64)>, &'static str> {
    let mut cursor = 0usize;
    let mut specs = Vec::new();
    for (index, _column) in columns.iter().enumerate() {
        let nulls = rows
            .iter()
            .filter(|row| matches!(row[index], Cell::Null))
            .count();
        if nulls == 0 {
            specs.push((0, 0));
        } else {
            cursor = align_cursor(cursor, 8);
            let length = rows.len().div_ceil(8);
            specs.push((
                i64::try_from(cursor).map_err(|_| "read_budget")?,
                i64::try_from(length).map_err(|_| "read_budget")?,
            ));
            cursor += length;
        }
        cursor = align_cursor(cursor, 8);
        let offset_len = (rows.len() + 1) * 4;
        specs.push((
            i64::try_from(cursor).map_err(|_| "read_budget")?,
            i64::try_from(offset_len).map_err(|_| "read_budget")?,
        ));
        cursor += offset_len;
        cursor = align_cursor(cursor, 8);
        let data_len: usize = rows
            .iter()
            .map(|row| match &row[index] {
                Cell::Text(text) => text.len(),
                Cell::Null => 0,
            })
            .sum();
        specs.push((
            i64::try_from(cursor).map_err(|_| "read_budget")?,
            i64::try_from(data_len).map_err(|_| "read_budget")?,
        ));
        cursor += data_len;
    }
    Ok(specs)
}

fn align_body(body: &mut Vec<u8>, align: usize) {
    while !body.len().is_multiple_of(align) {
        body.push(0);
    }
}

fn align_cursor(cursor: usize, align: usize) -> usize {
    cursor.saturating_add(align - 1) & !(align - 1)
}

fn wrap_file(schema: &[u8], batch: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ARROW1");
    out.extend_from_slice(&[0, 0]);
    push_message(&mut out, schema, &[]);
    push_message(&mut out, batch, body);
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&8i32.to_le_bytes());
    out.extend_from_slice(b"ARROW1");
    out
}

fn push_message(out: &mut Vec<u8>, meta: &[u8], body: &[u8]) {
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(&i32::try_from(meta.len()).unwrap_or(i32::MAX).to_le_bytes());
    out.extend_from_slice(meta);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    out.extend_from_slice(body);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
}
