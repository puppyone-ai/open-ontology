//! Local md/txt parser subprocess: one request frame on stdin, one response
//! frame on stdout (`oo_docs::parse::local`).

fn main() {
    std::process::exit(oo_docs::parse::local::run_stdio());
}
