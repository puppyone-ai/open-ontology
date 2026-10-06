fn main() {
    let code = oo_sql_runner::run(
        std::env::args().skip(1),
        std::io::stdout(),
        std::io::stderr(),
    );
    std::process::exit(code);
}
