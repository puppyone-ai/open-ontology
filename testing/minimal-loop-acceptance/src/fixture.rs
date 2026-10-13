//! Operator public-fixture files. Writing these rows is not the source copy.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::gold::{self, account_line, entry_line, party_line, schema_sql, ACCOUNT_ROWS, ENTRY_ROWS, PARTY_ROWS};
use crate::sha256::{sha256_file, sha256_hex};

pub fn write_fixture(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let schema = schema_sql();
    fs::write(dir.join("schema.sql"), schema).map_err(|error| error.to_string())?;
    write_lines(&dir.join("party.copy"), PARTY_ROWS, party_line)?;
    write_lines(&dir.join("account.copy"), ACCOUNT_ROWS, account_line)?;
    write_lines(&dir.join("entry.copy"), ENTRY_ROWS, entry_line)?;
    let party = sha256_file(&dir.join("party.copy"))?;
    let account = sha256_file(&dir.join("account.copy"))?;
    let entry = sha256_file(&dir.join("entry.copy"))?;
    let oracle = format!(
        "mp01b_copy=false\noperator_fixture=public\nparty_rows={PARTY_ROWS}\naccount_rows={ACCOUNT_ROWS}\nentry_rows={ENTRY_ROWS}\nnull_display={}\namount_3={}\nqty_0={}\nqty_1={}\nqty_2={}\nparty_0_display=\\N\nparty_0_active=t\naccount_0_party={}\nschema_sha256={}\nparty_sha256={party}\naccount_sha256={account}\nentry_sha256={entry}\n",
        gold::null_display_count(),
        gold::format_minor(gold::entry_amount_minor(3).expect("amount")),
        gold::entry_qty(0),
        gold::entry_qty(1),
        gold::entry_qty(2),
        gold::account_party(0),
        sha256_hex(schema.as_bytes()),
    );
    fs::write(dir.join("oracle.txt"), oracle).map_err(|error| error.to_string())?;
    Ok(())
}

fn write_lines(path: &Path, rows: i64, line: fn(u32) -> String) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
    for index in 0..rows {
        let text = line(u32::try_from(index).map_err(|_| "row index".to_owned())?);
        file.write_all(text.as_bytes()).map_err(|error| error.to_string())?;
    }
    file.flush().map_err(|error| error.to_string())
}
