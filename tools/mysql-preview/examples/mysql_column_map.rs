use std::io::{self, BufRead};

fn main() {
    let utc = std::env::args().skip(1).any(|arg| arg == "--session-utc");
    for line in io::stdin().lock().lines() {
        let result = match line
            .ok()
            .and_then(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        {
            Some(row) => oo_connectors::mysql::map::column_mapping_conclusion(&row, utc),
            None => serde_json::json!({"accepted":false,"code":"catalog","reason":"column_json"}),
        };
        println!("{result}");
    }
}
