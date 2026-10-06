fn main() {
    let code = oo_question_bank_suite::run(
        std::env::args().skip(1),
        std::io::stdout(),
        std::io::stderr(),
    );
    std::process::exit(code);
}
