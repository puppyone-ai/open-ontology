//! Compare one restore directory with the independent oracle.

use std::process::ExitCode;

use independent_gold::check;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: independent-gold ROOT");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: independent-gold ROOT");
        return ExitCode::from(2);
    }
    let root = check::root_from_arg(&root);
    let report = check::run(&root);
    let verdict = root.join("verdict.txt");
    if let Err(err) = check::write(&report, &verdict) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }
    if report.failed.is_empty() {
        ExitCode::from(0)
    } else {
        eprintln!("independent gold failed {} checks", report.failed.len());
        ExitCode::from(1)
    }
}
