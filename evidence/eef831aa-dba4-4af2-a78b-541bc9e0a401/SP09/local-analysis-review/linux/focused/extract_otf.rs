//! Extract one zip member. Used only on the private archive copy.
#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(zip_path) = args.next() else {
        return ExitCode::from(2);
    };
    let Some(out_path) = args.next() else {
        return ExitCode::from(2);
    };
    let Some(suffix) = args.next() else {
        return ExitCode::from(2);
    };
    let file = match File::open(zip_path) {
        Ok(file) => file,
        Err(_) => return ExitCode::from(2),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return ExitCode::from(2),
    };
    for index in 0..archive.len() {
        let mut item = match archive.by_index(index) {
            Ok(item) => item,
            Err(_) => return ExitCode::from(2),
        };
        if !item.name().ends_with(&suffix) {
            continue;
        }
        let mut bytes = Vec::new();
        if item.read_to_end(&mut bytes).is_err() {
            return ExitCode::from(2);
        }
        let mut out = match File::create(out_path) {
            Ok(file) => file,
            Err(_) => return ExitCode::from(2),
        };
        if out.write_all(&bytes).is_err() {
            return ExitCode::from(2);
        }
        println!("bytes {}", bytes.len());
        return ExitCode::SUCCESS;
    }
    ExitCode::from(3)
}
