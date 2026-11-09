fn main() -> std::process::ExitCode {
    let code = oo_eval::run(std::env::args().skip(1));
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(2))
}
