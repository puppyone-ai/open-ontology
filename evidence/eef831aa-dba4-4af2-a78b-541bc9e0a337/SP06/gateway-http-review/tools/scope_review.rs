//! Archive source gate for the HTTP gateway review.
//! Reads the frozen tree and prints hash rows. It does not change files.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

const ACCEPTED: &[(&str, &str)] = &[
    ("src/cache.rs", "23ec81bba03fe71b28c026798966f5f748a5d8932916648962ad4fc3881f4fc1"),
    ("src/canon.rs", "dd395153171400551517f18c61b6955d86c9418d85b3d5e8d8f905918ff57730"),
    ("src/error.rs", "6bd1f3a033381135d070c485d5fdfd56735c44dd01e0a19225f51b0a15fa7ed5"),
    ("src/lib.rs", "6cac5d52aed4f82c6895744ee73fb2ebd6dd07732e753e9506086c8e1eaf14e5"),
    ("src/live.rs", "b58e56f84309de4cbd7d5d59f4f377c2f06d2f7d1ef603517c63482aeb6529b3"),
    ("src/model.rs", "9dc3a64f158f23bbe92b924e5af3934dbdf98a41c3730d2fce34c5b641755de1"),
    ("src/sha256.rs", "eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037"),
    ("src/sql.rs", "46e36bef797c85a64c0d1714eeccc4a3897432d51597aea3c9eb5b7e02956fba"),
    ("src/store.rs", "2cf8e9ead5843d7c8a8da8b415aaea6182b39ba457153741530d0c4cf14c7827"),
    ("src/tls.rs", "1300d55b8067d619f4dbfd812937d3ee60185d4ed563e071d46eb483396c5ec7"),
    ("src/token.rs", "a68cb481d97b34f898ff95fb055f0bfec4447f094d6d07878580c46cad71b8e3"),
    ("src/trusted_control.rs", "d8d9eacf3642931d01a06a71387e365f0c593d3ee02cc43d09c42f8cb903c757"),
    (
        "migrations/oo_token_control_v1.sql",
        "501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597",
    ),
];

const ROOT_FEATURES: &[&str] = &[
    "axum = { version = \"=0.8.9\", default-features = false, features = [\"http1\", \"tokio\"] }",
    "tower = { version = \"=0.5.3\", default-features = false, features = [\"limit\", \"make\", \"timeout\", \"util\"] }",
    "tower-http = { version = \"=0.6.11\", default-features = false, features = [\"limit\", \"timeout\"] }",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("scope_review: {err}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let root = env::args().nth(1).ok_or("usage: scope_review ROOT")?;
    let build = fs::read_to_string(format!("{root}/crates/oo-gateway/build.rs")).map_err(|err| err.to_string())?;
    let lib = fs::read_to_string(format!("{root}/crates/oo-gateway/src/lib.rs")).map_err(|err| err.to_string())?;
    let http = fs::read_to_string(format!("{root}/crates/oo-gateway/src/http_gateway.rs")).map_err(|err| err.to_string())?;
    let pack = fs::read_to_string(format!("{root}/crates/oo-gateway/src/pack_catalog.rs")).map_err(|err| err.to_string())?;
    let bin = fs::read_to_string(format!("{root}/crates/oo-gateway/src/bin/oo-gateway.rs")).map_err(|err| err.to_string())?;
    let gateway_manifest = fs::read_to_string(format!("{root}/crates/oo-gateway/Cargo.toml")).map_err(|err| err.to_string())?;
    let root_manifest = fs::read_to_string(format!("{root}/Cargo.toml")).map_err(|err| err.to_string())?;

    let allow_line = build.matches("#[allow(unused, clippy::all)]").count();
    let clippy_all = build.matches("clippy::all").count();
    println!("allow_line={allow_line}");
    println!("clippy_all={clippy_all}");
    if allow_line != 1 || clippy_all != 1 {
        return Err("embed allow is not the one private module line".to_owned());
    }
    if !build.contains("mod accepted_token_store;") {
        return Err("embed glue does not name the private module".to_owned());
    }
    for source in [&lib, &http, &pack, &bin] {
        if source.contains("clippy::all") || source.contains("#! [allow") || source.contains("#![allow") {
            return Err("http or lib source carries a crate allow".to_owned());
        }
    }
    println!("lib_allow=0");
    if !lib.contains("#! [forbid(unsafe_code)]") && !lib.contains("#![forbid(unsafe_code)]") {
        return Err("lib.rs does not forbid unsafe".to_owned());
    }
    let executor = ["query", "_executor"].concat();
    let trusted = ["execute", "_trusted_select"].concat();
    for source in [&http, &pack, &bin] {
        if source.contains(&executor) || source.contains(&trusted) {
            return Err("http source names the sql executor".to_owned());
        }
    }
    println!("executor_name=0");
    if gateway_manifest.contains("gateway-token-store") {
        return Err("gateway manifest names the token store path".to_owned());
    }
    for line in ["axum = { workspace = true }", "tower = { workspace = true }", "tower-http = { workspace = true }"] {
        if !gateway_manifest.contains(line) {
            return Err(format!("gateway manifest missing {line}"));
        }
    }
    for line in ROOT_FEATURES {
        if !root_manifest.contains(line) {
            return Err(format!("root features missing {line}"));
        }
    }
    println!("features=pass");
    if !http.contains("Duration::from_millis(5_000)") || !http.contains("DeclaredTransport::ExternalClient") {
        return Err("http gateway is missing the cache bound or the external transport".to_owned());
    }
    if !http.contains("GlobalConcurrencyLimitLayer") || !http.contains("RequestBodyLimitLayer") {
        return Err("http gateway is missing a bound layer".to_owned());
    }
    println!("http_bounds=pass");
    println!("store_rows={}", ACCEPTED.len());
    for (relative, digest) in ACCEPTED {
        if build.matches(digest).count() != 1 {
            return Err(format!("build.rs does not pin {relative} once"));
        }
        println!("hash {relative} {digest}");
    }
    Ok(())
}
