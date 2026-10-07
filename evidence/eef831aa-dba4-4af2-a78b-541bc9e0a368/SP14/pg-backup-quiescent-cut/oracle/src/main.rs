//! Independent cut check. It does not link the product crate.

mod check;
mod sha256;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: cut-check <dir>");
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("usage: cut-check <dir>");
        std::process::exit(2);
    }
    if let Err(err) = check::check(std::path::Path::new(&root)) {
        eprintln!("cut-check: {err}");
        std::process::exit(1);
    }
}
