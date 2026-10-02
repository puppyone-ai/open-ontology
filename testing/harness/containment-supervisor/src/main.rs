#![forbid(unsafe_code)]

fn main() {
    let code = containment_supervisor::run();
    std::process::exit(code);
}
