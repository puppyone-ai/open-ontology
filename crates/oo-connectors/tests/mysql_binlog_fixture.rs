//! Test oracle only: ROW/FULL, CRC32, two columns INT + VARCHAR(64) utf8mb4.
//! Not a product CDC decoder. Rejects incompatible fixture row layouts.
use std::collections::BTreeSet;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .enumerate()
        .map(|(i, b)| u64::from(*b) << (8 * i))
        .sum()
}

pub fn affected_keys(mut bytes: &[u8]) -> BTreeSet<i128> {
    let mut table_id = None;
    let mut keys = BTreeSet::new();
    let mut pending = BTreeSet::new();
    let mut commits = 0;
    while !bytes.is_empty() {
        assert!(bytes.len() >= 19);
        let size = uint(&bytes[9..13]) as usize;
        assert!(size >= 23 && size <= bytes.len());
        let event = &bytes[..size];
        assert_eq!(crc32(&event[..size - 4]), uint(&event[size - 4..]) as u32);
        let data = &event[19..size - 4];
        match event[4] {
            19 => {
                // TABLE_MAP_EVENT
                let schema_len = data[8] as usize;
                let name_len_pos = 10 + schema_len;
                let name_len = data[name_len_pos] as usize;
                let column_pos = name_len_pos + 2 + name_len;
                if &data[9..9 + schema_len] == b"fixture"
                    && &data[name_len_pos + 1..name_len_pos + 1 + name_len] == b"int_rows"
                {
                    assert_eq!(&data[column_pos..column_pos + 3], &[2, 3, 15]);
                    assert_eq!(data[column_pos + 3], 2); // metadata length
                    assert_eq!(uint(&data[column_pos + 4..column_pos + 6]), 256);
                    table_id = Some(uint(&data[..6]));
                } else if table_id == Some(uint(&data[..6])) {
                    table_id = None;
                }
            }
            30..=32 if table_id == Some(uint(&data[..6])) => {
                // WRITE/UPDATE/DELETE_ROWS_EVENT_V2, with full column images.
                let extra = uint(&data[8..10]) as usize;
                let mut at = 8 + extra;
                assert_eq!(data[at], 2);
                at += 1;
                assert_eq!(data[at] & 3, 3);
                at += 1;
                if event[4] == 31 {
                    assert_eq!(data[at] & 3, 3);
                    at += 1;
                }
                while at < data.len() {
                    assert_eq!(data[at] & 3, 0); // both columns NOT NULL; padding bits ignored
                    at += 1;
                    let id = i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
                    pending.insert(i128::from(id));
                    at += 4;
                    let len = uint(&data[at..at + 2]) as usize;
                    at += 2 + len;
                    assert!(at <= data.len());
                }
            }
            16 => {
                // XID_EVENT: only committed row changes count.
                keys.append(&mut pending);
                commits += 1;
            }
            _ => {}
        }
        bytes = &bytes[size..];
    }
    assert!(pending.is_empty(), "uncommitted binlog tail");
    assert_eq!(commits, 100, "fixture writer commits");
    keys
}

#[test]
fn crc_matches_standard_vector() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
}

#[test]
#[ignore = "requires an owned fixture binlog window"]
fn parse_saved_window() {
    let path = std::env::var("OO_MYSQL_WINDOW").unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(affected_keys(&bytes).len(), 500);
}
