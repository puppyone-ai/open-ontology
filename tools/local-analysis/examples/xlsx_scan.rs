use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        return ExitCode::from(2);
    };
    let Some(needle) = args.next() else {
        return ExitCode::from(2);
    };
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return ExitCode::from(2),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return ExitCode::from(2),
    };
    let mut xml = String::new();
    for index in 0..archive.len() {
        let mut item = match archive.by_index(index) {
            Ok(item) => item,
            Err(_) => return ExitCode::from(2),
        };
        if !item.name().ends_with(".xml") {
            continue;
        }
        if item.read_to_string(&mut xml).is_err() {
            return ExitCode::from(2);
        }
    }
    if xml.contains("<f>") || xml.contains("<f ") {
        return ExitCode::from(3);
    }
    if !xml.contains(&needle) {
        return ExitCode::from(4);
    }
    println!("xlsx_scan_ok");
    ExitCode::SUCCESS
}
