#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = xtask::output_format(&args);
    let report = xtask::dispatch(&args);
    print!("{}", report.render(format));
    std::process::exit(report.exit_code);
}
