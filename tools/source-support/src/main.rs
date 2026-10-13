//! `source-support` reads the support matrix and prints one versioned JSON object.
//! It is not the `oo` command. Parent API integration is pending.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let outcome = source_support::execute(&args);
    println!("{}", source_support::json::stringify(&outcome.to_json()));
    std::process::exit(outcome.exit_code);
}
