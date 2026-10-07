//! Operator entry point. The password is read from a file inside the commands.

use std::path::Path;
use std::process::ExitCode;

use pg_backup_restore::ops;
use pg_backup_restore::protocol::load_operator;
use pg_backup_restore::sha256;
use pg_backup_restore::{fail, Fail};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("pg-backup-restore: {}", err.detail);
            ExitCode::from(err.exit)
        }
    }
}

fn run() -> Result<u8, Fail> {
    if !sha256::self_check() {
        return Err(fail(1, "sha256 self check failed"));
    }
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(usage());
    };
    if command == "hash-tree" {
        let Some(root) = args.next() else {
            return Err(usage());
        };
        let Some(out) = args.next() else {
            return Err(usage());
        };
        if args.next().is_some() {
            return Err(usage());
        }
        ops::hash_tree(Path::new(&root), Path::new(&out))?;
        return Ok(0);
    }
    let Some(operator_path) = args.next() else {
        return Err(usage());
    };
    if args.next().is_some() {
        return Err(usage());
    }
    let operator = load_operator(Path::new(&operator_path))?;
    match command.as_str() {
        "load-fixture" => ops::load_fixture(&operator).map(|()| 0),
        "backup" => ops::backup(&operator).map(|()| 0),
        "mark-post-cut" => ops::mark_post_cut(&operator).map(|()| 0),
        "restore" => ops::restore(&operator),
        _ => Err(usage()),
    }
}

fn usage() -> Fail {
    fail(
        2,
        "usage: pg-backup-restore <load-fixture|backup|mark-post-cut|restore> <operator.json> | hash-tree <root> <sums>",
    )
}
