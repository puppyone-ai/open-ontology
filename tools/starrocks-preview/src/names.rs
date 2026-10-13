//! Physical mapping names. The bytes match `_oo.pm_payload`.

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

pub fn sha256_raw(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// `prefix || first 60 hex of the mapping digest`.
pub fn physical_name(
    level: char,
    source: &str,
    config_digest: &str,
    generation: i64,
    namespace: &str,
    table: &str,
    column: &str,
) -> (String, [u8; 32]) {
    let digest = sha256_raw(&payload(
        level,
        source,
        config_digest,
        generation,
        namespace,
        table,
        column,
    ));
    let prefix = match level {
        'n' => "pn_",
        't' => "pt_",
        'c' => "pc_",
        _ => "px_",
    };
    (format!("{prefix}{}", &hex_encode(&digest)[..60]), digest)
}

fn payload(
    level: char,
    source: &str,
    config_digest: &str,
    generation: i64,
    namespace: &str,
    table: &str,
    column: &str,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"physical-mapping/v1\0");
    push_cstr(&mut out, source);
    push_cstr(&mut out, config_digest);
    push_cstr(&mut out, &generation.to_string());
    out.push(level as u8);
    push_cstr(&mut out, namespace);
    push_cstr(&mut out, table);
    push_cstr(&mut out, column);
    out
}

fn push_cstr(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

pub type PreimageColumn<'a> = (&'a str, i64, i32);
pub type PreimageTable<'a> = (&'a str, &'a str, &'a [PreimageColumn<'a>]);

pub fn schema_preimage(tables: &[PreimageTable<'_>]) -> String {
    let mut out = String::new();
    for (schema, table, columns) in tables {
        out.push_str(schema);
        out.push('.');
        out.push_str(table);
        out.push('\n');
        for (name, oid, typmod) in *columns {
            out.push_str(name);
            out.push('\t');
            out.push_str(&oid.to_string());
            out.push('\t');
            out.push_str(&typmod.to_string());
            out.push('\t');
            out.push('\n');
        }
    }
    out
}

pub fn quality_preimage(batch_id: &str, accepted: u64, rejected: u64, input_digest: &str) -> Vec<u8> {
    format!("full-import/v1 quality\nbatch\t{batch_id}\t{accepted}\t{rejected}\t{input_digest}\n")
        .into_bytes()
}

/// One stored row. Null is `\N`. The line is what the payload digest covers.
pub fn row_line(position: u64, cells: &[Option<&str>]) -> String {
    let mut line = position.to_string();
    for cell in cells {
        line.push('\t');
        match cell {
            Some(text) => line.push_str(text),
            None => line.push_str("\\N"),
        }
    }
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_physical_names_match_pm_payload() {
        let config = "a".repeat(64);
        let (namespace, _) = physical_name('n', "sr_w45", &config, 1, "oo_w45_sr", "", "");
        let (table, _) = physical_name('t', "sr_w45", &config, 1, "oo_w45_sr", "pk_accounts", "");
        let (column, digest) = physical_name(
            'c',
            "sr_w45",
            &config,
            1,
            "oo_w45_sr",
            "pk_accounts",
            "account_id",
        );
        assert_eq!(
            namespace,
            "pn_189e2feaa22604eef0799de452c1e65d7a1107f417553133ebc22e7064f7"
        );
        assert_eq!(
            table,
            "pt_2980eaf4951c5274983fd508bba6b10d8b3dfdca6569e7d91020b9f1f91b"
        );
        assert_eq!(
            column,
            "pc_f2615aca79c61b71766bd02002a7f471b859102393a8bcb0de836a1feb8c"
        );
        assert_eq!(digest.len(), 32);
    }

    #[test]
    fn schema_preimage_digest_matches_one_column() {
        let text = schema_preimage(&[("oo_w45_sr", "pk_accounts", &[("account_id", 20, -1)])]);
        assert_eq!(text, "oo_w45_sr.pk_accounts\naccount_id\t20\t-1\t\n");
        assert_eq!(
            sha256_hex(text.as_bytes()),
            "57e323a32931b41e761ccd385c09d7762d99cbcb65cf819109f5200e0cd44f04"
        );
    }

    #[test]
    fn null_cell_is_escaped_and_empty_string_is_kept() {
        let line = row_line(1, &[Some("300"), None, Some("")]);
        assert_eq!(line, "1\t300\t\\N\t\n");
    }
}
