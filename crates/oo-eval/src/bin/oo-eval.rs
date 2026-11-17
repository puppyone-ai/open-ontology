fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("docs") => oo_eval::docs::run(args[1..].to_vec()),
        _ => oo_eval::run(args),
    };
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(2))
}
