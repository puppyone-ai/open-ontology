fn main() {
    let code = oo_result_comparator::run(
        std::env::args().skip(1),
        std::io::stdin(),
        std::io::stdout(),
        std::io::stderr(),
    );
    std::process::exit(code);
}
