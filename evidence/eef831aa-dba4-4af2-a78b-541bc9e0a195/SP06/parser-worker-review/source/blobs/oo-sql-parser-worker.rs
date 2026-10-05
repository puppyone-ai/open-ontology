//! One-shot SQL guard worker.
//!
//! The parent writes one request frame on stdin. This process writes one
//! plan or refusal frame on stdout and exits. Extra arguments exit 2.
//! `OO_PARSER_WORKER_SLOW`, when set, exits 2 and does not change the deadline.

fn main() {
    std::process::exit(oo_gateway::parser_worker::worker_main());
}
