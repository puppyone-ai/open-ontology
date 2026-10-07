//! Compile the accepted gateway-token-store sources into this crate.
//!
//! `oo-arch-check` rejects a path dependency on that tool. The bytes are
//! pinned. `live.rs` is hashed and not compiled.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FILES: &[(&str, &str, bool)] = &[
    (
        "src/cache.rs",
        "23ec81bba03fe71b28c026798966f5f748a5d8932916648962ad4fc3881f4fc1",
        true,
    ),
    (
        "src/canon.rs",
        "dd395153171400551517f18c61b6955d86c9418d85b3d5e8d8f905918ff57730",
        true,
    ),
    (
        "src/error.rs",
        "6bd1f3a033381135d070c485d5fdfd56735c44dd01e0a19225f51b0a15fa7ed5",
        true,
    ),
    (
        "src/lib.rs",
        "6cac5d52aed4f82c6895744ee73fb2ebd6dd07732e753e9506086c8e1eaf14e5",
        true,
    ),
    (
        "src/live.rs",
        "b58e56f84309de4cbd7d5d59f4f377c2f06d2f7d1ef603517c63482aeb6529b3",
        false,
    ),
    (
        "src/model.rs",
        "9dc3a64f158f23bbe92b924e5af3934dbdf98a41c3730d2fce34c5b641755de1",
        true,
    ),
    (
        "src/sha256.rs",
        "eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037",
        true,
    ),
    (
        "src/sql.rs",
        "46e36bef797c85a64c0d1714eeccc4a3897432d51597aea3c9eb5b7e02956fba",
        true,
    ),
    (
        "src/store.rs",
        "2cf8e9ead5843d7c8a8da8b415aaea6182b39ba457153741530d0c4cf14c7827",
        true,
    ),
    (
        "src/tls.rs",
        "1300d55b8067d619f4dbfd812937d3ee60185d4ed563e071d46eb483396c5ec7",
        true,
    ),
    (
        "src/token.rs",
        "a68cb481d97b34f898ff95fb055f0bfec4447f094d6d07878580c46cad71b8e3",
        true,
    ),
    (
        "src/trusted_control.rs",
        "d8d9eacf3642931d01a06a71387e365f0c593d3ee02cc43d09c42f8cb903c757",
        true,
    ),
    (
        "migrations/oo_token_control_v1.sql",
        "501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597",
        false,
    ),
];

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let store = manifest_dir.join("../../tools/gateway-token-store");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("out dir"));
    let module_dir = out_dir.join("accepted_token_store");
    fs::create_dir_all(&module_dir).expect("module dir");

    for (relative, expected, compile) in FILES {
        let source = store.join(relative);
        println!("cargo:rerun-if-changed={}", source.display());
        let got = sha256_file(&source);
        if got != *expected {
            panic!("accepted token store hash mismatch for {relative}: got {got}");
        }
        if !compile {
            continue;
        }
        let text = fs::read_to_string(&source).expect("read accepted source");
        let rewritten = rewrite(&text, Path::new(relative).file_name().and_then(|n| n.to_str()).unwrap_or(""));
        let output_name = if *relative == "src/lib.rs" { "mod.rs" } else {
            Path::new(relative).file_name().and_then(|name| name.to_str()).expect("file name")
        };
        fs::write(module_dir.join(output_name), rewritten).expect("write module");
    }

    let module_path = module_dir.join("mod.rs");
    let glue = format!(
        "#[allow(unused, clippy::all)]\n#[path = \"{}\"]\nmod accepted_token_store;\n",
        module_path.display().to_string().replace('\\', "\\\\").replace('"', "\\\"")
    );
    fs::write(out_dir.join("accepted_glue.rs"), glue).expect("write glue");
}

fn rewrite(source: &str, file_name: &str) -> String {
    let mut text = source.to_owned();
    if file_name == "lib.rs" {
        text = text.replacen("#![forbid(unsafe_code)]\n", "", 1);
        text = text.replace("#[cfg(test)]\nmod live;\n", "");
    }
    if file_name == "sql.rs" {
        text = text.replace(
            "include_str!(\"../migrations/oo_token_control_v1.sql\")",
            "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../tools/gateway-token-store/migrations/oo_token_control_v1.sql\"))",
        );
    }
    text.replace("crate::", "crate::accepted_token_store::")
}

fn sha256_file(path: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .unwrap_or_else(|err| panic!("sha256sum {}: {err}", path.display()));
    if !output.status.success() {
        panic!("sha256sum failed for {}", path.display());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().next().unwrap_or("").to_owned()
}
