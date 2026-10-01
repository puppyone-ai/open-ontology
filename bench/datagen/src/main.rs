fn main() {
    let code = match oo_datagen::cli::run(std::env::args().skip(1)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("oo-datagen: {error}");
            1
        }
    };
    std::process::exit(code);
}
