//! One-shot SQL guard worker.
//!
//! The parent writes one request frame on stdin and closes it. This process
//! requires end of file after that frame, then writes one plan or refusal
//! frame on stdout. A trailing request byte exits 1 with no reply. Extra
//! arguments exit 2. `OO_PARSER_WORKER_SLOW`, when set, exits 2 and does not
//! change the deadline.

fn main() {
    std::process::exit(oo_gateway::parser_worker::worker_main());
}
