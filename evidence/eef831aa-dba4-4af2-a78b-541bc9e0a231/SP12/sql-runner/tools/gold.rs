//! Slice the handwritten desk gold into the live suite files.
//!
//! The input is the fixed-archive fixture. This program does not read adapter
//! stdout and does not print cell values.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let src = args.next().expect("source");
    let out = args.next().expect("out");
    let bytes = fs::read(&src).expect("read gold");
    let text = String::from_utf8(bytes).expect("utf8");
    let out = Path::new(&out);
    fs::create_dir_all(out).expect("outdir");
    fs::write(out.join("gold-exact.json"), text.as_bytes()).expect("exact");
    let changed = text.replacen("\"beta\"", "\"CHANGED-CELL-a231\"", 1);
    if changed == text || changed.matches("CHANGED-CELL-a231").count() != 1 {
        eprintln!("gold: changed slice failed");
        return ExitCode::from(2);
    }
    fs::write(out.join("gold-changed.json"), changed.as_bytes()).expect("changed");
    let set_gold = text.replacen(
        "\"row_comparator\":\"ordered\"",
        "\"row_comparator\":\"set\"",
        1,
    );
    if set_gold == text {
        eprintln!("gold: set slice failed");
        return ExitCode::from(2);
    }
    fs::write(out.join("gold-set.json"), set_gold.as_bytes()).expect("set");
    let columns = json_value(&text, "\"columns\":").expect("columns");
    let rows = json_value(&text, "\"rows\":").expect("rows");
    let first = first_element(rows).expect("first row");
    let prefix = format!(
        "{{\"row_comparator\":\"ordered\",\"expected\":{{\"kind\":\"rows\",\"completion\":\"complete\",\"warnings\":[],\"columns\":{columns},\"rows\":[{first}]}}}}\n"
    );
    if prefix.contains("CHANGED-CELL-a231") || prefix.matches("\"beta\"").count() != 0 {
        eprintln!("gold: prefix slice kept the wrong row");
        return ExitCode::from(2);
    }
    fs::write(out.join("gold-prefix.json"), prefix.as_bytes()).expect("prefix");
    ExitCode::from(0)
}

fn json_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(key)? + key.len();
    bounded(&text[at..])
}

fn first_element(array: &str) -> Option<&str> {
    let start = array.find('[')? + 1;
    let rest = array[start..].trim_start();
    bounded(rest)
}

fn bounded(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if *byte == b'\\' {
                esc = true;
            } else if *byte == b'"' {
                in_str = false;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
            }
            continue;
        }
        match *byte {
            b'"' => {
                in_str = true;
                if depth == 0 && index > 0 {
                    return None;
                }
            }
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}
